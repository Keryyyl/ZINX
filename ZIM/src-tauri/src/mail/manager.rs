//! Uruchamianie synchronizacji kont: jedna naraz na konto, ponowne uruchomienie, gdy w trakcie przyszła
//! kolejna prośba, przerywanie przy usuwaniu konta i zdarzenia `sync:status` dla interfejsu.

use super::remote::Cancel;
use super::session::ImapSession;
use super::sync::{sync_account, Progress, SyncOptions};
use crate::db::{accounts, accounts::Account, Db};
use crate::error::Result;
use futures::future::BoxFuture;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub account_id: i64,
    /// syncing | idle | error
    pub state: &'static str,
    pub done: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub type Connector = Arc<dyn Fn(Account) -> BoxFuture<'static, Result<Box<dyn ImapSession>>> + Send + Sync>;
pub type Emit = Arc<dyn Fn(SyncStatus) + Send + Sync>;

/// Opóźnienie, które skleja serię kliknięć w jedno wypchnięcie zmian na serwer
const PUSH_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Default)]
struct State {
    running: HashMap<i64, Cancel>,
    rerun: HashSet<i64>,
    synced_once: HashSet<i64>,
}

struct Inner {
    db: Db,
    connect: Connector,
    emit: Emit,
    on_done: Arc<dyn Fn() + Send + Sync>,
    state: Mutex<State>,
    push_generation: AtomicU64,
}

#[derive(Clone)]
pub struct SyncManager(Arc<Inner>);

impl SyncManager {
    /// `on_done` jest wołane po udanej synchronizacji (tu ruszy później kolejka tagowania AI).
    pub fn new(db: Db, connect: Connector, emit: Emit, on_done: Arc<dyn Fn() + Send + Sync>) -> Self {
        SyncManager(Arc::new(Inner { db, connect, emit, on_done, state: Mutex::new(State::default()), push_generation: AtomicU64::new(0) }))
    }

    pub fn start(&self, account: Account) {
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        let first_time = {
            let mut st = self.0.state.lock().unwrap();
            if st.running.contains_key(&account.id) {
                st.rerun.insert(account.id);
                return;
            }
            st.running.insert(account.id, cancel.clone());
            !st.synced_once.contains(&account.id)
        };
        if first_time {
            (self.0.emit)(SyncStatus { account_id: account.id, state: "syncing", done: 0, total: 0, error: None });
        }
        let this = self.clone();
        tauri::async_runtime::spawn(async move { this.run(account, cancel).await });
    }

    async fn run(&self, account: Account, cancel: Cancel) {
        let progress = Arc::new(Mutex::new(Progress { done: 0, total: 0 }));
        let id = account.id;
        let result = self.sync_once(account.clone(), &cancel, &progress).await;
        let last = *progress.lock().unwrap();
        let status = |state, error| SyncStatus { account_id: id, state, done: last.done, total: last.total, error };
        match result {
            Ok(()) if !cancel.load(Ordering::Relaxed) => {
                self.0.state.lock().unwrap().synced_once.insert(id);
                (self.0.emit)(status("idle", None));
                (self.0.on_done)();
            }
            Ok(()) => {}
            Err(_) if cancel.load(Ordering::Relaxed) => {}
            Err(e) => (self.0.emit)(status("error", Some(e.0))),
        }
        let again = {
            let mut st = self.0.state.lock().unwrap();
            st.running.remove(&id);
            st.rerun.remove(&id) && !cancel.load(Ordering::Relaxed)
        };
        if again {
            self.start(account);
        }
    }

    async fn sync_once(&self, account: Account, cancel: &Cancel, progress: &Arc<Mutex<Progress>>) -> Result<()> {
        let mut session = (self.0.connect)(account.clone()).await?;
        let (emit, sink, id) = (self.0.emit.clone(), progress.clone(), account.id);
        let on_progress = Box::new(move |p: Progress| {
            *sink.lock().unwrap() = p;
            emit(SyncStatus { account_id: id, state: "syncing", done: p.done, total: p.total, error: None });
        });
        let opts = SyncOptions { cancel: Some(cancel.clone()), on_progress: Some(on_progress) };
        sync_account(&self.0.db, session.as_mut(), account.id, &opts).await
    }

    pub fn sync_all(&self) {
        match self.0.db.with(accounts::list) {
            Ok(list) => list.into_iter().for_each(|a| self.start(a)),
            Err(e) => eprintln!("synchronizacja: {e}"),
        }
    }

    /// Po akcji w ZIM: wypchnij zmiany na serwer (z krótkim opóźnieniem, żeby złączyć serię kliknięć).
    pub fn schedule_push(&self) {
        let generation = self.0.push_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(PUSH_DEBOUNCE).await;
            if this.0.push_generation.load(Ordering::SeqCst) == generation {
                this.sync_all();
            }
        });
    }

    /// Przerywa synchronizację konta (np. przy jego usuwaniu) i zapomina o nim.
    pub fn abort(&self, account_id: i64) {
        let mut st = self.0.state.lock().unwrap();
        if let Some(cancel) = st.running.get(&account_id) {
            cancel.store(true, Ordering::Relaxed);
        }
        st.rerun.remove(&account_id);
        st.synced_once.remove(&account_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;
    use crate::mail::mock::MockServer;
    use std::sync::atomic::AtomicUsize;

    struct Fixture {
        manager: SyncManager,
        server: MockServer,
        events: Arc<Mutex<Vec<SyncStatus>>>,
        connects: Arc<AtomicUsize>,
        done: Arc<AtomicUsize>,
        db: Db,
    }

    fn fixture() -> Fixture {
        let conn = open_memory();
        conn.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('me@x.pl', 'imap.x.pl', 'me')", []).unwrap();
        let db = Db::new(conn);
        let server = MockServer::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let (connects, done) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (s, c) = (server.clone(), connects.clone());
        let connect: Connector = Arc::new(move |_| {
            let (s, c) = (s.clone(), c.clone());
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(s.session()) as Box<dyn ImapSession>)
            })
        });
        let sink = events.clone();
        let emit: Emit = Arc::new(move |e| sink.lock().unwrap().push(e));
        let d = done.clone();
        let manager = SyncManager::new(db.clone(), connect, emit, Arc::new(move || {
            d.fetch_add(1, Ordering::SeqCst);
        }));
        Fixture { manager, server, events, connects, done, db }
    }

    async fn settle(f: &Fixture) {
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if f.manager.0.state.lock().unwrap().running.is_empty() {
                return;
            }
        }
        panic!("synchronizacja się nie skończyła");
    }

    fn account(f: &Fixture) -> Account {
        f.db.with(accounts::list).unwrap().remove(0)
    }

    #[tokio::test]
    async fn emits_syncing_progress_then_idle_and_downloads() {
        let f = fixture();
        f.server.add("INBOX", "From: a@x.pl\r\nSubject: T\r\nMessage-ID: <m@x.pl>\r\n\r\ntreść", false);
        f.manager.start(account(&f));
        settle(&f).await;
        let events = f.events.lock().unwrap().clone();
        assert_eq!(events.first().map(|e| e.state), Some("syncing"));
        let last = events.last().unwrap();
        assert_eq!((last.state, last.done, last.total), ("idle", 1, 1));
        assert_eq!(f.done.load(Ordering::SeqCst), 1);
        let n: i64 = f.db.with(|c| Ok(c.query_row("SELECT count(*) FROM messages", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn second_sync_is_silent_until_it_finishes() {
        let f = fixture();
        f.manager.start(account(&f));
        settle(&f).await;
        f.events.lock().unwrap().clear();
        f.manager.start(account(&f));
        settle(&f).await;
        let states: Vec<_> = f.events.lock().unwrap().iter().map(|e| e.state).collect();
        assert_eq!(states, ["idle"], "kolejne synchronizacje nie migają paskiem postępu");
    }

    #[tokio::test]
    async fn request_during_a_run_triggers_exactly_one_rerun() {
        let f = fixture();
        let acc = account(&f);
        f.manager.start(acc.clone());
        // trzy prośby, zanim pierwsza ruszy (zadanie nie zdążyło się wykonać)
        f.manager.start(acc.clone());
        f.manager.start(acc.clone());
        settle(&f).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        settle(&f).await;
        assert_eq!(f.connects.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn connection_error_is_reported_and_next_start_works() {
        let f = fixture();
        let sink = f.events.clone();
        let emit: Emit = Arc::new(move |e| sink.lock().unwrap().push(e));
        let failing: Connector = Arc::new(|_| Box::pin(async { Err("Logowanie nie powiodło się".into()) }));
        let manager = SyncManager::new(f.db.clone(), failing, emit, Arc::new(|| {}));
        manager.start(account(&f));
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if manager.0.state.lock().unwrap().running.is_empty() {
                break;
            }
        }
        let last = f.events.lock().unwrap().last().cloned().unwrap();
        assert_eq!((last.state, last.error.as_deref()), ("error", Some("Logowanie nie powiodło się")));
        assert!(manager.0.state.lock().unwrap().running.is_empty());
    }

    #[tokio::test]
    async fn aborted_account_reports_nothing_and_sync_all_covers_every_account() {
        let f = fixture();
        f.db.with(|c| {
            c.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('b@x.pl', 'imap.x.pl', 'b')", [])?;
            Ok(())
        })
        .unwrap();
        f.manager.sync_all();
        settle(&f).await;
        assert_eq!(f.connects.load(Ordering::SeqCst), 2);

        let acc = account(&f);
        f.events.lock().unwrap().clear();
        f.manager.start(acc.clone());
        f.manager.abort(acc.id);
        settle(&f).await;
        assert!(f.events.lock().unwrap().iter().all(|e| e.state != "error"));
    }

    #[tokio::test]
    async fn pushes_are_debounced() {
        let f = fixture();
        for _ in 0..5 {
            f.manager.schedule_push();
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
        settle(&f).await;
        assert_eq!(f.connects.load(Ordering::SeqCst), 1);
    }
}
