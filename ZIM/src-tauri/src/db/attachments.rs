use crate::error::Result;
use crate::mail::attachments::AttachmentInfo;
use rusqlite::{params, Connection};
use serde::Serialize;

/// Załącznik odebranej wiadomości; sama treść jest na serwerze, dopóki jej nie pobierzemy
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Attachment {
    pub id: i64,
    pub filename: String,
    pub mime: String,
    pub size: i64,
    /// Obrazek osadzony w treści maila (np. logo w stopce)
    pub inline: bool,
}

pub fn list(conn: &Connection, message_id: i64) -> Result<Vec<Attachment>> {
    let mut stmt = conn.prepare("SELECT id, filename, mime, size, inline FROM attachments WHERE message_id = ? ORDER BY id")?;
    let rows = stmt.query_map([message_id], |r| {
        Ok(Attachment { id: r.get(0)?, filename: r.get(1)?, mime: r.get(2)?, size: r.get(3)?, inline: r.get::<_, i64>(4)? == 1 })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Zapisuje metadane załączników wiadomości i oznacza ją jako sprawdzoną. Transakcję zakłada wywołujący.
pub fn save(conn: &Connection, message_id: i64, infos: &[AttachmentInfo]) -> Result<()> {
    for a in infos {
        conn.execute(
            "INSERT OR IGNORE INTO attachments (message_id, part, filename, mime, size, content_id, inline) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![message_id, a.part, a.filename, a.mime, a.size, a.content_id, a.inline as i64],
        )?;
    }
    conn.execute("UPDATE messages SET attachments_checked = 1 WHERE id = ?", [message_id])?;
    Ok(())
}

/// Wiadomości sprzed obsługi załączników: metadane dociągamy partiami przy synchronizacji.
pub fn unchecked(conn: &Connection, account_id: i64, mailbox: &str, limit: i64) -> Result<Vec<(i64, u32)>> {
    let mut stmt = conn.prepare(
        "SELECT id, server_uid FROM messages
         WHERE account_id = ? AND mailbox = ? AND server_uid IS NOT NULL AND attachments_checked = 0 LIMIT ?",
    )?;
    let rows = stmt.query_map(params![account_id, mailbox, limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Załącznik razem z miejscem wiadomości na serwerze (potrzebnym do pobrania).
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentLocation {
    pub id: i64,
    pub message_id: i64,
    pub account_id: i64,
    pub part: String,
    pub filename: String,
    pub mailbox: Option<String>,
    pub server_uid: Option<u32>,
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<AttachmentLocation>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT a.id, a.message_id, m.account_id, a.part, a.filename, m.mailbox, m.server_uid
             FROM attachments a JOIN messages m ON m.id = a.message_id WHERE a.id = ?",
            [id],
            |r| {
                Ok(AttachmentLocation {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    account_id: r.get(2)?,
                    part: r.get(3)?,
                    filename: r.get(4)?,
                    mailbox: r.get(5)?,
                    server_uid: r.get(6)?,
                })
            },
        )
        .optional()?)
}
