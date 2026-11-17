//! Wysyłka z konta (od razu albo z kolejki „Wyślij o”) i przetwarzanie zaplanowanych wiadomości.

use super::manager::Connector;
use super::send::{save_to_sent, send_mail, OutgoingMail};
use crate::db::{accounts, outbox, Db};
use crate::error::{Error, Result};
use crate::secrets;
use futures::future::BoxFuture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Wysyła wiadomość z podanego konta: SMTP, a potem kopia w „Wysłanych” (jej błąd tylko logujemy).
pub type Sender = Arc<dyn Fn(i64, OutgoingMail) -> BoxFuture<'static, Result<()>> + Send + Sync>;

pub fn real_sender(db: Db, connect: Connector) -> Sender {
    Arc::new(move |account_id, mail| {
        let (db, connect) = (db.clone(), connect.clone());
        Box::pin(async move {
            let account = db
                .with(accounts::list)?
                .into_iter()
                .find(|a| a.id == account_id)
                .ok_or_else(|| Error::from("Wybierz konto, z którego chcesz wysłać wiadomość"))?;
            let password = secrets::get(&secrets::account_key(account.id))?
                .ok_or_else(|| Error(format!("Brak hasła dla {}. Usuń i dodaj konto ponownie.", account.email)))?;
            let raw = send_mail(&account, &password, &mail).await?;
            let copy = async {
                let mut session = connect(account.clone()).await?;
                save_to_sent(session.as_mut(), &raw).await
            };
            if let Err(e) = copy.await {
                eprintln!("zapis do Wysłanych {}: {e}", account.email);
            }
            Ok(())
        })
    })
}

#[derive(Clone)]
pub struct Outbox {
    db: Db,
    send: Sender,
    notify: Arc<dyn Fn() + Send + Sync>,
    running: Arc<AtomicBool>,
}

impl Outbox {
    pub fn new(db: Db, send: Sender, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Outbox { db, send, notify, running: Arc::new(AtomicBool::new(false)) }
    }

    pub fn notify(&self) {
        (self.notify)();
    }

    pub async fn send_now(&self, account_id: i64, mail: OutgoingMail) -> Result<()> {
        (self.send)(account_id, mail).await
    }

    /// Wysyła zaplanowane wiadomości, których czas już minął.
    pub async fn process(&self, now: i64) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let due = self.db.with(|c| outbox::claim_due(c, now)).unwrap_or_default();
        if !due.is_empty() {
            self.notify();
        }
        for item in due {
            match (self.send)(item.account_id, item.mail).await {
                Ok(()) => {
                    let _ = self.db.with(|c| outbox::mark_sent(c, item.id));
                }
                Err(e) => {
                    eprintln!("zaplanowana wysyłka: {e}");
                    let _ = self.db.with(|c| outbox::mark_failed(c, item.id, &e.0));
                }
            }
            self.notify();
        }
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn kick(&self) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move { this.process(crate::now_ms()).await });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;
    use crate::mail::send::MailAttachment;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    fn setup(fail: bool) -> (Outbox, Db, Arc<Mutex<Vec<(i64, OutgoingMail)>>>, Arc<AtomicUsize>) {
        let conn = open_memory();
        conn.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('me@x.pl', 'imap.x.pl', 'me')", []).unwrap();
        let db = Db::new(conn);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let (s, notified) = (sent.clone(), Arc::new(AtomicUsize::new(0)));
        let send: Sender = Arc::new(move |id, mail| {
            let s = s.clone();
            Box::pin(async move {
                if fail {
                    return Err("535 Authentication failed".into());
                }
                s.lock().unwrap().push((id, mail));
                Ok(())
            })
        });
        let n = notified.clone();
        (Outbox::new(db.clone(), send, Arc::new(move || {
            n.fetch_add(1, Ordering::SeqCst);
        })), db, sent, notified)
    }

    fn mail(subject: &str) -> OutgoingMail {
        OutgoingMail { to: vec!["a@x.pl".into()], subject: subject.into(), text: "t".into(), attachments: vec![MailAttachment { filename: "f.txt".into(), content: vec![9] }] }
    }

    #[tokio::test]
    async fn due_mail_is_sent_with_attachments_and_removed() {
        let (outbox, db, sent, notified) = setup(false);
        db.with(|c| {
            outbox::schedule(c, 1, &mail("Teraz"), 1_000, 1)?;
            outbox::schedule(c, 1, &mail("Jutro"), 9_000_000, 1)?;
            Ok(())
        })
        .unwrap();
        outbox.process(2_000).await;
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].0, sent[0].1.subject.as_str(), sent[0].1.attachments.len()), (1, "Teraz", 1));
        let left = db.with(|c| outbox::list(c, None)).unwrap();
        assert_eq!(left.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(), ["Jutro"]);
        assert!(notified.load(Ordering::SeqCst) >= 2, "interfejs dostaje informację o zmianie");
    }

    #[tokio::test]
    async fn failure_keeps_the_mail_with_the_server_reply_and_does_not_retry_by_itself() {
        let (outbox, db, _, _) = setup(true);
        db.with(|c| outbox::schedule(c, 1, &mail("A"), 1_000, 1)).unwrap();
        outbox.process(2_000).await;
        let m = db.with(|c| outbox::list(c, None)).unwrap().remove(0);
        assert_eq!((m.status.as_str(), m.error.as_deref()), ("error", Some("535 Authentication failed")));
        outbox.process(3_000).await;
        assert_eq!(db.with(|c| outbox::list(c, None)).unwrap()[0].status, "error");
    }

    #[tokio::test]
    async fn overlapping_runs_do_not_double_send() {
        let (outbox, db, sent, _) = setup(false);
        db.with(|c| outbox::schedule(c, 1, &mail("A"), 1_000, 1)).unwrap();
        tokio::join!(outbox.process(2_000), outbox.process(2_000), outbox.process(2_000));
        assert_eq!(sent.lock().unwrap().len(), 1);
    }
}
