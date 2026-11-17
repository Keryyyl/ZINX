//! Poczta zaplanowana („Wyślij o”): ZIM wysyła ją sam, gdy jest uruchomiony.

use crate::error::Result;
use crate::mail::send::{MailAttachment, OutgoingMail};
use rusqlite::{params, Connection};
use serde::Serialize;

const SNIPPET_CHARS: usize = 120;

/// Wiadomość zaplanowana przez „Wyślij o”
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMail {
    pub id: i64,
    pub account_id: i64,
    pub to: Vec<String>,
    pub subject: String,
    pub snippet: String,
    pub send_at: i64,
    /// scheduled | sending | error
    pub status: String,
    pub error: Option<String>,
    pub attachments: i64,
}

pub fn schedule(conn: &Connection, account_id: i64, mail: &OutgoingMail, send_at: i64, now: i64) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO outbox (account_id, to_addrs, subject, body_text, send_at, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        params![account_id, serde_json::to_string(&mail.to)?, mail.subject, mail.text, send_at, now],
    )?;
    let id = tx.last_insert_rowid();
    for a in &mail.attachments {
        tx.execute("INSERT INTO outbox_attachments (outbox_id, filename, data) VALUES (?, ?, ?)", params![id, a.filename, a.content])?;
    }
    tx.commit()?;
    Ok(id)
}

pub fn list(conn: &Connection, account_id: Option<i64>) -> Result<Vec<ScheduledMail>> {
    let mut stmt = conn.prepare(
        "SELECT o.id, o.account_id, o.to_addrs, o.subject, o.body_text, o.send_at, o.status, o.error,
                (SELECT COUNT(*) FROM outbox_attachments a WHERE a.outbox_id = o.id)
         FROM outbox o WHERE ?1 IS NULL OR o.account_id = ?1 ORDER BY o.send_at",
    )?;
    let rows = stmt.query_map([account_id], |r| {
        let to_json: String = r.get(2)?;
        let body: String = r.get(4)?;
        Ok(ScheduledMail {
            id: r.get(0)?,
            account_id: r.get(1)?,
            to: serde_json::from_str(&to_json).unwrap_or_default(),
            subject: r.get(3)?,
            snippet: body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(SNIPPET_CHARS).collect(),
            send_at: r.get(5)?,
            status: r.get(6)?,
            error: r.get(7)?,
            attachments: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Wysyłanej właśnie wiadomości nie da się już anulować.
pub fn cancel(conn: &Connection, id: i64) -> Result<()> {
    if conn.execute("DELETE FROM outbox WHERE id = ? AND status != 'sending'", [id])? == 0 {
        return Err("Tej wiadomości nie można już anulować".into());
    }
    Ok(())
}

pub fn retry(conn: &Connection, id: i64, now: i64) -> Result<()> {
    conn.execute("UPDATE outbox SET status = 'scheduled', error = NULL, send_at = ? WHERE id = ? AND status = 'error'", params![now, id])?;
    Ok(())
}

pub struct Due {
    pub id: i64,
    pub account_id: i64,
    pub mail: OutgoingMail,
}

/// Zaplanowane na teraz lub wcześniej (np. aplikacja była wyłączona); od razu oznaczone jako wysyłane.
pub fn claim_due(conn: &Connection, now: i64) -> Result<Vec<Due>> {
    let tx = conn.unchecked_transaction()?;
    let rows: Vec<(i64, i64, String, String, String)> = {
        let mut stmt = tx.prepare("SELECT id, account_id, to_addrs, subject, body_text FROM outbox WHERE status = 'scheduled' AND send_at <= ? ORDER BY send_at")?;
        let rows = stmt.query_map([now], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut due = Vec::new();
    for (id, account_id, to_json, subject, text) in rows {
        tx.execute("UPDATE outbox SET status = 'sending' WHERE id = ?", [id])?;
        let mut stmt = tx.prepare("SELECT filename, data FROM outbox_attachments WHERE outbox_id = ? ORDER BY id")?;
        let attachments = stmt
            .query_map([id], |r| Ok(MailAttachment { filename: r.get(0)?, content: r.get(1)? }))?
            .collect::<std::result::Result<_, _>>()?;
        due.push(Due { id, account_id, mail: OutgoingMail { to: serde_json::from_str(&to_json).unwrap_or_default(), subject, text, attachments } });
    }
    tx.commit()?;
    Ok(due)
}

pub fn mark_sent(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM outbox WHERE id = ?", [id])?;
    Ok(())
}

pub fn mark_failed(conn: &Connection, id: i64, error: &str) -> Result<()> {
    conn.execute("UPDATE outbox SET status = 'error', error = ? WHERE id = ?", params![error, id])?;
    Ok(())
}

/// Po starcie: 'sending' oznacza, że aplikacja zamknęła się w trakcie wysyłki. Nie wiemy, czy serwer zdążył ją
/// przyjąć, więc nie ponawiamy sami (ryzyko podwójnej wysyłki).
pub fn recover_interrupted(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE outbox SET status = 'error',
           error = 'Wysyłkę przerwało zamknięcie aplikacji. Sprawdź Wysłane, zanim spróbujesz ponownie.'
         WHERE status = 'sending'",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    fn setup() -> Connection {
        let db = open_memory();
        db.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('me@x.pl', 'imap.x.pl', 'me')", []).unwrap();
        db
    }

    fn mail(subject: &str) -> OutgoingMail {
        OutgoingMail { to: vec!["a@x.pl".into()], subject: subject.into(), text: "linia  jeden\n\nlinia dwa".into(), attachments: vec![MailAttachment { filename: "a.txt".into(), content: vec![1, 2, 3] }] }
    }

    #[test]
    fn schedule_list_and_snippet() {
        let db = setup();
        schedule(&db, 1, &mail("Później"), 2_000, 1).unwrap();
        schedule(&db, 1, &mail("Wcześniej"), 1_000, 1).unwrap();
        let all = list(&db, None).unwrap();
        assert_eq!(all.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(), ["Wcześniej", "Później"]);
        assert_eq!((all[0].snippet.as_str(), all[0].attachments, all[0].status.as_str()), ("linia jeden linia dwa", 1, "scheduled"));
        assert!(list(&db, Some(2)).unwrap().is_empty());
    }

    #[test]
    fn claim_due_takes_only_due_and_marks_sending_once() {
        let db = setup();
        schedule(&db, 1, &mail("Teraz"), 1_000, 1).unwrap();
        schedule(&db, 1, &mail("Jutro"), 9_000, 1).unwrap();
        let due = claim_due(&db, 1_500).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!((due[0].mail.subject.as_str(), due[0].mail.attachments[0].content.as_slice()), ("Teraz", &[1u8, 2, 3][..]));
        assert!(claim_due(&db, 1_500).unwrap().is_empty(), "druga próba nie może zająć tej samej wiadomości");
        assert_eq!(list(&db, None).unwrap()[0].status, "sending");
    }

    #[test]
    fn cancel_fail_retry_and_recover() {
        let db = setup();
        let id = schedule(&db, 1, &mail("A"), 1_000, 1).unwrap();
        let due = claim_due(&db, 2_000).unwrap();
        assert!(cancel(&db, id).is_err(), "wysyłanej nie da się anulować");
        mark_failed(&db, due[0].id, "535 Authentication failed").unwrap();
        assert_eq!(list(&db, None).unwrap()[0].error.as_deref(), Some("535 Authentication failed"));
        retry(&db, id, 3_000).unwrap();
        let m = &list(&db, None).unwrap()[0];
        assert_eq!((m.status.as_str(), m.error.clone(), m.send_at), ("scheduled", None, 3_000));
        claim_due(&db, 3_000).unwrap();
        recover_interrupted(&db).unwrap();
        let m = &list(&db, None).unwrap()[0];
        assert_eq!(m.status, "error");
        assert!(m.error.as_deref().unwrap().contains("Sprawdź Wysłane"));
        cancel(&db, id).unwrap();
        assert!(list(&db, None).unwrap().is_empty());
        let files: i64 = db.query_row("SELECT count(*) FROM outbox_attachments", [], |r| r.get(0)).unwrap();
        assert_eq!(files, 0, "załączniki znikają razem z wiadomością");
    }

    #[test]
    fn sent_mail_leaves_the_outbox() {
        let db = setup();
        let id = schedule(&db, 1, &mail("A"), 1, 1).unwrap();
        mark_sent(&db, id).unwrap();
        assert!(list(&db, None).unwrap().is_empty());
    }
}
