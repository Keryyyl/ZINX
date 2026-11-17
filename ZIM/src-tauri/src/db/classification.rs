//! Dane dla asystenta AI: które wiadomości czekają na tagi, zapis wyniku, skróty.

use crate::error::Result;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::LazyLock;

/// Tagujemy tylko świeże wiadomości; starsze użytkownik może dotagować ręcznie.
pub const RECENT_DAYS: i64 = 30;
pub const MAX_ATTEMPTS: i64 = 3;
const DAY_MS: i64 = 86_400_000;

pub fn recent_since(now: i64) -> i64 {
    now - RECENT_DAYS * DAY_MS
}

/// Wiadomość w postaci, jaką dostaje model.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifyInput {
    pub id: i64,
    pub from: String,
    pub subject: String,
    pub body: String,
    /// Nazwy załączników (bez obrazków osadzonych w treści)
    pub attachments: Vec<String>,
}

// spam i kosz pomijamy: nie ma sensu zużywać na nie modelu
const ELIGIBLE: &str = "folder IN ('inbox', 'archive')";

fn to_input(conn: &Connection, id: i64, from: String, subject: String, body: String) -> Result<ClassifyInput> {
    let mut stmt = conn.prepare("SELECT filename FROM attachments WHERE message_id = ? AND inline = 0 ORDER BY id")?;
    let attachments = stmt.query_map([id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    Ok(ClassifyInput { id, from, subject, body, attachments })
}

/// Najnowsza jeszcze niesklasyfikowana wiadomość.
pub fn next_to_classify(conn: &Connection, since: i64) -> Result<Option<ClassifyInput>> {
    let row: Option<(i64, String, String, String)> = conn
        .query_row(
            &format!(
                "SELECT id, from_addr, subject, body_text FROM messages
                 WHERE classified_at IS NULL AND classify_attempts < ?1 AND date >= ?2 AND {ELIGIBLE}
                 ORDER BY date DESC LIMIT 1"
            ),
            params![MAX_ATTEMPTS, since],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    row.map(|(id, from, subject, body)| to_input(conn, id, from, subject, body)).transpose()
}

static NO_REPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(no[-_.]?reply|do[-_.]?not[-_.]?reply)(\+.*)?$").unwrap());

/// Automatyczne nadawcy (noreply@, no-reply@, donotreply@): ich maile są zwykle tego samego rodzaju.
pub fn is_no_reply(email: Option<&str>) -> bool {
    let local = email.unwrap_or("").split('@').next().unwrap_or("").to_lowercase();
    NO_REPLY.is_match(&local)
}

/// Tagi wcześniejszej, już sprawdzonej wiadomości od tego samego nadawcy automatycznego: noreply albo wiadomość
/// z nagłówkiem List-Unsubscribe (None = nie ma wzorca, trzeba zapytać model). Bierze najnowszą z okna `since`,
/// więc ręczne poprawki użytkownika też się przenoszą, a reprocess (który czyści tylko to okno) nie kopiuje
/// przestarzałych wyników.
pub fn sender_tags(conn: &Connection, message_id: i64, since: i64) -> Result<Option<Vec<i64>>> {
    let me: Option<(Option<String>, i64)> =
        conn.query_row("SELECT from_email, bulk FROM messages WHERE id = ?", [message_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((Some(email), bulk)) = me else { return Ok(None) };
    if !(bulk == 1 || is_no_reply(Some(&email))) {
        return Ok(None);
    }
    let known: Option<i64> = conn
        .query_row(
            "SELECT id FROM messages WHERE from_email = ? AND id != ? AND classified_at IS NOT NULL AND date >= ?
             ORDER BY classified_at DESC LIMIT 1",
            params![email, message_id, since],
            |r| r.get(0),
        )
        .optional()?;
    let Some(known) = known else { return Ok(None) };
    let mut stmt = conn.prepare("SELECT tag_id FROM message_tags WHERE message_id = ?")?;
    let ids = stmt.query_map([known], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    Ok(Some(ids))
}

pub fn load_for_classification(conn: &Connection, id: i64) -> Result<Option<ClassifyInput>> {
    let row: Option<(String, String, String)> = conn
        .query_row("SELECT from_addr, subject, body_text FROM messages WHERE id = ? AND folder != 'deleted'", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    row.map(|(from, subject, body)| to_input(conn, id, from, subject, body)).transpose()
}

pub fn count_pending(conn: &Connection, since: i64) -> Result<usize> {
    Ok(conn.query_row(
        &format!("SELECT COUNT(*) FROM messages WHERE classified_at IS NULL AND classify_attempts < ?1 AND date >= ?2 AND {ELIGIBLE}"),
        params![MAX_ATTEMPTS, since],
        |r| r.get::<_, i64>(0),
    )? as usize)
}

/// Zapisuje wynik modelu: poprzednie tagi od AI tej wiadomości są zastępowane, a tagi dodane ręcznie zostają
/// (INSERT OR IGNORE nie zmienia ich źródła). Wiadomość z wbudowanym tagiem „Zachować” (po kluczu, nie po nazwie)
/// trafia z Odebranych do Archiwum; zwraca true, gdy została przeniesiona.
pub fn save_classification(conn: &Connection, message_id: i64, tag_ids: &[i64], now: i64) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM message_tags WHERE message_id = ? AND source = 'llm'", [message_id])?;
    for tag_id in tag_ids {
        tx.execute("INSERT OR IGNORE INTO message_tags (message_id, tag_id, source) VALUES (?, ?, 'llm')", params![message_id, tag_id])?;
    }
    tx.execute("UPDATE messages SET classified_at = ?, classify_attempts = 0 WHERE id = ?", params![now, message_id])?;
    let moved = tx.execute(
        "UPDATE messages SET folder = 'archive'
         WHERE id = ? AND folder = 'inbox'
           AND EXISTS (SELECT 1 FROM message_tags mt JOIN tags t ON t.id = mt.tag_id
                       WHERE mt.message_id = messages.id AND t.system_key = 'keep')",
        [message_id],
    )?;
    tx.commit()?;
    Ok(moved > 0)
}

pub fn record_failure(conn: &Connection, message_id: i64) -> Result<()> {
    conn.execute("UPDATE messages SET classify_attempts = classify_attempts + 1 WHERE id = ?", [message_id])?;
    Ok(())
}

/// Kolejka od nowa dla ostatnich wiadomości (np. po dodaniu tagów albo zmianie modelu).
pub fn reset_recent(conn: &Connection, since: i64) -> Result<()> {
    conn.execute(&format!("UPDATE messages SET classified_at = NULL, classify_attempts = 0 WHERE date >= ? AND {ELIGIBLE}"), [since])?;
    Ok(())
}

pub fn get_summary(conn: &Connection, message_id: i64) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT summary FROM messages WHERE id = ?", [message_id], |r| r.get::<_, Option<String>>(0)).optional()?.flatten())
}

pub fn save_summary(conn: &Connection, message_id: i64, summary: &str) -> Result<()> {
    conn.execute("UPDATE messages SET summary = ? WHERE id = ?", params![summary, message_id])?;
    Ok(())
}
