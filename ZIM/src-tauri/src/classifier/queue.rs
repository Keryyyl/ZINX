//! Tło tagowania: bierze po jednej niesklasyfikowanej wiadomości (od najnowszej) i pyta model.
//! Uruchamiana po synchronizacji; gdy model jest niedostępny, kończy po cichu i próbuje przy następnej.

use super::errors::{AiError, Reason};
use super::llm::Classify;
use crate::db::classification::{
    count_pending, load_for_classification, next_to_classify, recent_since, record_failure, reset_recent, save_classification, sender_tags,
};
use crate::db::settings::{get_ai_config, AiConfig};
use crate::db::{tags, Db};
use crate::error::{Error, Result};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    #[serde(flatten)]
    pub config: AiConfig,
    pub state: &'static str,
    /// Wiadomości z ostatnich 30 dni czekające na tagi
    pub pending: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct ClassifierDeps {
    pub db: Db,
    pub classify: Classify,
    /// Po każdej otagowanej wiadomości; wołający sam decyduje, jak często odświeżać interfejs
    pub on_tagged: Arc<dyn Fn() + Send + Sync>,
    /// Po przeniesieniu wiadomości do Archiwum (tag „Zachować”): trzeba wypchnąć zmianę na serwer
    pub on_archived: Arc<dyn Fn() + Send + Sync>,
    pub now: Arc<dyn Fn() -> i64 + Send + Sync>,
}

#[derive(Default)]
struct Flags {
    running: bool,
    again: bool,
}

struct Status {
    state: &'static str,
    error: Option<String>,
}

struct Inner {
    deps: ClassifierDeps,
    flags: Mutex<Flags>,
    status: Mutex<Status>,
}

#[derive(Clone)]
pub struct Classifier(Arc<Inner>);

impl Classifier {
    pub fn new(deps: ClassifierDeps) -> Self {
        Classifier(Arc::new(Inner { deps, flags: Mutex::default(), status: Mutex::new(Status { state: "idle", error: None }) }))
    }

    fn set(&self, state: &'static str, error: Option<String>) {
        *self.0.status.lock().unwrap() = Status { state, error };
    }

    fn now(&self) -> i64 {
        (self.0.deps.now)()
    }

    fn config(&self) -> Result<AiConfig> {
        self.0.deps.db.with(|c| get_ai_config(c))
    }

    async fn pass(&self) {
        let d = &self.0.deps;
        loop {
            let Ok(cfg) = self.config() else { return };
            if !cfg.enabled {
                return self.set("disabled", None);
            }
            let Ok(tag_list) = d.db.with(|c| tags::list(c)) else { return };
            if tag_list.is_empty() {
                return self.set("no-tags", None);
            }
            let since = recent_since(self.now());
            let Ok(next) = d.db.with(|c| next_to_classify(c, since)) else { return };
            let Some(mail) = next else { return self.set("idle", None) };

            self.set("working", None);
            let outcome: std::result::Result<Vec<i64>, AiError> = async {
                // nadawca noreply sprawdzony wcześniej: te same tagi bez pytania modelu
                if let Some(ids) = d.db.with(|c| sender_tags(c, mail.id, since))? {
                    return Ok(ids);
                }
                (d.classify)(tag_list.clone(), mail.clone(), cfg.clone()).await
            }
            .await;
            match outcome {
                Ok(ids) => {
                    let archived = d.db.with(|c| save_classification(c, mail.id, &ids, self.now()));
                    if matches!(archived, Ok(true)) {
                        (d.on_archived)();
                    }
                    (d.on_tagged)();
                }
                Err(AiError::Unavailable(reason, message)) => {
                    let state = match reason {
                        Reason::Down => "unavailable",
                        Reason::NoModel => "no-model",
                        Reason::NoKey => "no-key",
                    };
                    return self.set(state, Some(message));
                }
                Err(AiError::Other(message)) => {
                    eprintln!("klasyfikacja wiadomości: {message}");
                    let _ = d.db.with(|c| record_failure(c, mail.id));
                }
            }
        }
    }

    pub async fn run(&self) {
        {
            let mut f = self.0.flags.lock().unwrap();
            if f.running {
                f.again = true; // nowe dane przyszły w trakcie przebiegu
                return;
            }
            f.running = true;
        }
        loop {
            self.0.flags.lock().unwrap().again = false;
            self.pass().await;
            let mut f = self.0.flags.lock().unwrap();
            if !f.again {
                f.running = false;
                return;
            }
        }
    }

    pub fn kick(&self) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move { this.run().await });
    }

    pub fn status(&self) -> QueueStatus {
        let config = self.config().unwrap_or(AiConfig { enabled: false, provider: "ollama".into(), model: String::new() });
        let (state, error) = {
            let s = self.0.status.lock().unwrap();
            (s.state, s.error.clone())
        };
        let pending = self.0.deps.db.with(|c| count_pending(c, recent_since(self.now()))).unwrap_or(0);
        QueueStatus { state: if config.enabled { state } else { "disabled" }, config, pending, error }
    }

    /// Tagi ostatnich wiadomości liczone od nowa (np. po dodaniu tagów albo zmianie modelu).
    pub fn reprocess(&self) {
        let since = recent_since(self.now());
        let _ = self.0.deps.db.with(|c| reset_recent(c, since));
        self.kick();
    }

    /// Jedna wiadomość od razu; błędy trafiają do wołającego, bo użytkownik czeka na wynik.
    pub async fn classify_now(&self, id: i64) -> Result<()> {
        let d = &self.0.deps;
        let cfg = self.config()?;
        if !cfg.enabled {
            return Err("Asystent AI jest wyłączony w ustawieniach".into());
        }
        let tag_list = d.db.with(|c| tags::list(c))?;
        if tag_list.is_empty() {
            return Err("Najpierw utwórz tagi w Ustawieniach → Tagi".into());
        }
        let mail = d.db.with(|c| load_for_classification(c, id))?.ok_or_else(|| Error::from("Tej wiadomości już nie ma"))?;
        let ids = (d.classify)(tag_list, mail, cfg).await?;
        if d.db.with(|c| save_classification(c, id, &ids, self.now()))? {
            (d.on_archived)();
        }
        (d.on_tagged)();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::classification::MAX_ATTEMPTS;
    use crate::db::{open_memory, settings};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    const NOW: i64 = 1_793_000_000_000;
    const DAY: i64 = 86_400_000;

    fn setup() -> Db {
        let conn = open_memory();
        conn.execute("INSERT INTO accounts (id, email, imap_host, username) VALUES (1, 'a@x.pl', 'imap.x.pl', 'a')", []).unwrap();
        // zostawiamy tylko własne tagi testu; wbudowane tagi ważności są zawsze, więc usuwamy je wprost
        conn.execute("DELETE FROM tags", []).unwrap();
        Db::new(conn)
    }

    fn add(db: &Db, id: i64, days_ago: i64, folder: &str, from: &str) {
        db.with(|c| {
            c.execute(
                "INSERT INTO messages (id, account_id, uid, from_addr, from_email, to_addr, subject, date, body_text, folder)
                 VALUES (?1, 1, ?1, ?2, ?2, 'a@x.pl', ?3, ?4, 'treść', ?5)",
                rusqlite::params![id, from, format!("Temat {id}"), NOW - days_ago * DAY, folder],
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn tag(db: &Db, name: &str) -> i64 {
        db.with(|c| tags::create(c, name, None, "")).unwrap().id
    }

    type Answer = Arc<Mutex<std::result::Result<Vec<i64>, AiError>>>;

    struct Harness {
        classifier: Classifier,
        calls: Arc<Mutex<Vec<i64>>>,
        tagged: Arc<AtomicUsize>,
        archived: Arc<AtomicBool>,
        answer: Answer,
    }

    fn harness(db: &Db, answer: std::result::Result<Vec<i64>, AiError>) -> Harness {
        let (calls, tagged, archived) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        let answer: Answer = Arc::new(Mutex::new(answer));
        let (c, a) = (calls.clone(), answer.clone());
        let classify: Classify = Arc::new(move |_, mail, _| {
            c.lock().unwrap().push(mail.id);
            let reply = a.lock().unwrap().clone();
            Box::pin(async move { reply })
        });
        let (t, ar) = (tagged.clone(), archived.clone());
        let classifier = Classifier::new(ClassifierDeps {
            db: db.clone(),
            classify,
            on_tagged: Arc::new(move || {
                t.fetch_add(1, Ordering::SeqCst);
            }),
            on_archived: Arc::new(move || ar.store(true, Ordering::SeqCst)),
            now: Arc::new(|| NOW),
        });
        Harness { classifier, calls, tagged, archived, answer }
    }

    fn tags_of(db: &Db, id: i64) -> Vec<(String, String)> {
        db.with(|c| {
            let mut stmt = c.prepare("SELECT t.name, mt.source FROM message_tags mt JOIN tags t ON t.id = mt.tag_id WHERE mt.message_id = ? ORDER BY t.name")?;
            let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
    }

    #[tokio::test]
    async fn tags_fresh_inbox_and_archive_mail_newest_first() {
        let db = setup();
        let work = tag(&db, "Praca");
        add(&db, 1, 5, "inbox", "b@x.pl");
        add(&db, 2, 1, "inbox", "c@x.pl");
        add(&db, 3, 2, "spam", "d@x.pl");
        add(&db, 4, 90, "inbox", "e@x.pl");
        let h = harness(&db, Ok(vec![work]));
        h.classifier.run().await;
        assert_eq!(*h.calls.lock().unwrap(), [2, 1], "od najnowszej; spam i stare pomijamy");
        assert_eq!(tags_of(&db, 2), [("Praca".to_string(), "llm".to_string())]);
        assert!(tags_of(&db, 3).is_empty() && tags_of(&db, 4).is_empty());
        let status = h.classifier.status();
        assert_eq!((status.state, status.pending), ("idle", 0));
        assert_eq!(h.tagged.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn manual_tags_are_never_overwritten() {
        let db = setup();
        let work = tag(&db, "Praca");
        add(&db, 1, 1, "inbox", "b@x.pl");
        db.with(|c| tags::set_message_tag(c, &[1], work, true)).unwrap();
        harness(&db, Ok(vec![work])).classifier.run().await;
        assert_eq!(tags_of(&db, 1), [("Praca".to_string(), "manual".to_string())]);
    }

    #[tokio::test]
    async fn without_tags_or_when_disabled_nothing_is_asked() {
        let db = setup();
        add(&db, 1, 1, "inbox", "b@x.pl");
        let h = harness(&db, Ok(vec![]));
        h.classifier.run().await;
        assert_eq!(h.classifier.status().state, "no-tags");
        tag(&db, "Praca");
        db.with(|c| settings::set(c, "ai.enabled", "0")).unwrap();
        h.classifier.run().await;
        assert_eq!(h.classifier.status().state, "disabled");
        assert!(h.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unavailable_model_stops_quietly_without_using_attempts_and_resumes() {
        let db = setup();
        let work = tag(&db, "Praca");
        add(&db, 1, 1, "inbox", "b@x.pl");
        let h = harness(&db, Err(AiError::Unavailable(Reason::Down, "brak połączenia".into())));
        h.classifier.run().await;
        let s = h.classifier.status();
        assert_eq!((s.state, s.error.as_deref(), s.pending), ("unavailable", Some("brak połączenia"), 1));
        *h.answer.lock().unwrap() = Ok(vec![work]);
        h.classifier.run().await;
        assert_eq!(h.classifier.status().state, "idle");
        assert_eq!(tags_of(&db, 1).len(), 1);
    }

    #[tokio::test]
    async fn rejected_key_and_missing_model_have_their_own_states() {
        let db = setup();
        tag(&db, "Praca");
        add(&db, 1, 1, "inbox", "b@x.pl");
        let h = harness(&db, Err(AiError::Unavailable(Reason::NoKey, "klucz odrzucony".into())));
        h.classifier.run().await;
        assert_eq!((h.classifier.status().state, h.classifier.status().pending), ("no-key", 1));
        assert_eq!(h.calls.lock().unwrap().len(), 1);
        *h.answer.lock().unwrap() = Err(AiError::Unavailable(Reason::NoModel, "brak modelu".into()));
        h.classifier.run().await;
        assert_eq!(h.classifier.status().state, "no-model");
    }

    #[tokio::test]
    async fn gives_up_on_a_message_after_a_few_failures() {
        let db = setup();
        tag(&db, "Praca");
        add(&db, 1, 1, "inbox", "b@x.pl");
        let h = harness(&db, Err(AiError::Other("Model zwrócił niepoprawny JSON".into())));
        h.classifier.run().await;
        assert_eq!(h.calls.lock().unwrap().len() as i64, MAX_ATTEMPTS);
        let s = h.classifier.status();
        assert_eq!((s.state, s.pending), ("idle", 0));
    }

    #[tokio::test]
    async fn classify_now_and_reprocess_replace_ai_tags() {
        let db = setup();
        let (a, b) = (tag(&db, "A"), tag(&db, "B"));
        add(&db, 1, 1, "inbox", "x@x.pl");
        let h = harness(&db, Ok(vec![a]));
        h.classifier.run().await;
        assert_eq!(tags_of(&db, 1)[0].0, "A");
        *h.answer.lock().unwrap() = Ok(vec![b]);
        h.classifier.classify_now(1).await.unwrap();
        assert_eq!(tags_of(&db, 1), [("B".to_string(), "llm".to_string())]);
        *h.answer.lock().unwrap() = Ok(vec![a]);
        db.with(|c| reset_recent(c, recent_since(NOW))).unwrap();
        h.classifier.run().await;
        assert_eq!(tags_of(&db, 1), [("A".to_string(), "llm".to_string())]);
        assert!(h.classifier.classify_now(99).await.is_err());
    }

    #[tokio::test]
    async fn automatic_senders_reuse_tags_without_asking_the_model() {
        let db = setup();
        let bills = tag(&db, "Rachunki");
        add(&db, 1, 3, "inbox", "noreply@firma.pl");
        add(&db, 2, 1, "inbox", "noreply@firma.pl");
        add(&db, 3, 0, "inbox", "jan@firma.pl");
        // najstarsza już sprawdzona wcześniej
        db.with(|c| {
            c.execute("UPDATE messages SET classified_at = ? WHERE id = 1", [NOW - DAY])?;
            tags::set_message_tag(c, &[1], bills, true)
        })
        .unwrap();
        let h = harness(&db, Ok(vec![]));
        h.classifier.run().await;
        assert_eq!(*h.calls.lock().unwrap(), [3], "model tylko dla zwykłego nadawcy");
        assert_eq!(tags_of(&db, 2), [("Rachunki".to_string(), "llm".to_string())]);
    }

    #[tokio::test]
    async fn keep_tag_archives_the_message_and_asks_for_a_push() {
        let db = setup();
        db.with(|c| tags::ensure_system_tags(c)).unwrap();
        let keep = db.with(|c| tags::list(c)).unwrap().into_iter().find(|t| t.system.as_deref() == Some("keep")).unwrap().id;
        add(&db, 1, 1, "inbox", "b@x.pl");
        let h = harness(&db, Ok(vec![keep]));
        h.classifier.run().await;
        assert!(h.archived.load(Ordering::SeqCst));
        let folder: String = db.with(|c| Ok(c.query_row("SELECT folder FROM messages WHERE id = 1", [], |r| r.get(0))?)).unwrap();
        assert_eq!(folder, "archive");
    }

    #[tokio::test]
    async fn overlapping_runs_do_not_process_a_message_twice() {
        let db = setup();
        let work = tag(&db, "Praca");
        add(&db, 1, 1, "inbox", "b@x.pl");
        let h = harness(&db, Ok(vec![work]));
        tokio::join!(h.classifier.run(), h.classifier.run(), h.classifier.run());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(h.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn no_reply_detection() {
        use crate::db::classification::is_no_reply;
        for yes in ["noreply@x.pl", "No-Reply@x.pl", "do_not_reply@x.pl", "donotreply+abc@x.pl"] {
            assert!(is_no_reply(Some(yes)), "{yes}");
        }
        for no in ["jan@x.pl", "reply@x.pl", "noreplyjan@x.pl"] {
            assert!(!is_no_reply(Some(no)), "{no}");
        }
        assert!(!is_no_reply(None));
    }
}
