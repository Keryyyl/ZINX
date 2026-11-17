//! ZIM (tylko odczyt): nieodczytane, ostatnia wiadomość i ważne (tag „Pilne”, wbudowany klucz `urgent`).

use crate::paths;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MailLine {
    pub od: String,
    pub temat: String,
    pub konto: String,
}

fn err(e: rusqlite::Error) -> String {
    format!("ZIM: {e}")
}

pub fn connect() -> Result<Connection, String> {
    paths::open_read(&paths::zim(), "ZIM")
}

pub fn unread(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT COUNT(*) FROM messages WHERE read = 0 AND folder = 'inbox'", [], |r| r.get(0)).map_err(err)
}

pub fn latest(conn: &Connection, limit: i64) -> Result<Vec<MailLine>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT m.from_addr, m.subject, COALESCE(a.alias, a.email)
               FROM messages m JOIN accounts a ON a.id = m.account_id
              WHERE m.folder = 'inbox'
              ORDER BY m.date DESC, m.id DESC LIMIT ?1",
        )
        .map_err(err)?;
    let rows = stmt.query_map([limit], |r| Ok(MailLine { od: r.get(0)?, temat: r.get(1)?, konto: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Nieprzeczytane wiadomości z tagiem ważności „urgent” (nazwa tagu może być zmieniona przez użytkownika)
pub fn important(conn: &Connection, limit: i64) -> Result<(i64, Vec<MailLine>), String> {
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT m.id) FROM messages m
               JOIN message_tags mt ON mt.message_id = m.id
               JOIN tags t ON t.id = mt.tag_id
              WHERE t.system_key = 'urgent' AND m.read = 0 AND m.folder = 'inbox'",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT m.from_addr, m.subject, COALESCE(a.alias, a.email)
               FROM messages m
               JOIN accounts a ON a.id = m.account_id
              WHERE m.read = 0 AND m.folder = 'inbox'
                AND EXISTS (SELECT 1 FROM message_tags mt JOIN tags t ON t.id = mt.tag_id
                             WHERE mt.message_id = m.id AND t.system_key = 'urgent')
              ORDER BY m.date DESC, m.id DESC LIMIT ?1",
        )
        .map_err(err)?;
    let rows = stmt.query_map([limit], |r| Ok(MailLine { od: r.get(0)?, temat: r.get(1)?, konto: r.get(2)? })).map_err(err)?;
    Ok((total, rows.collect::<Result<_, _>>().map_err(err)?))
}

#[derive(Debug, Serialize)]
pub struct FoundMail {
    pub od: String,
    pub temat: String,
    pub konto: String,
    pub data_ms: i64,
}

/// Szuka w nadawcy i temacie (bez rozróżniania wielkości liter) w skrzynce odbiorczej
pub fn search(conn: &Connection, query: &str, limit: i64) -> Result<Vec<FoundMail>, String> {
    let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    let mut stmt = conn
        .prepare(
            "SELECT m.from_addr, m.subject, COALESCE(a.alias, a.email), m.date
               FROM messages m JOIN accounts a ON a.id = m.account_id
              WHERE m.folder = 'inbox' AND (m.subject LIKE ?1 ESCAPE '\\' OR m.from_addr LIKE ?1 ESCAPE '\\')
              ORDER BY m.date DESC, m.id DESC LIMIT ?2",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map((pattern, limit), |r| Ok(FoundMail { od: r.get(0)?, temat: r.get(1)?, konto: r.get(2)?, data_ms: r.get(3)? }))
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

#[cfg(test)]
mod search_tests {
    use super::*;

    /// Odczyt z prawdziwej bazy ZIM (tylko odczyt); pomijany, gdy nie ma bazy
    #[test]
    fn search_reads_inbox_without_writing() {
        let Ok(conn) = connect() else { return };
        let found = search(&conn, "klarna", 5).unwrap();
        println!("znalezione: {}", found.len());
        assert!(search(&conn, "zzz-nie-ma-takiego-maila-123", 5).unwrap().is_empty());
    }
}
