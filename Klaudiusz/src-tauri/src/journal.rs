//! Dziennik zmian i cofanie. Każda zmiana (zapis) dostaje wpis z operacją odwrotną.
//! Dane w bazie Klaudiusza, obok przypomnień. Tu też trzymamy ustawienia (np. wyciszenie).

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS journal (
           id INTEGER PRIMARY KEY,
           created_at INTEGER NOT NULL,
           description TEXT NOT NULL,
           undo TEXT NOT NULL,
           undone INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS settings (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );",
    )
    .map_err(|e| format!("Dziennik: {e}"))
}

fn err(e: rusqlite::Error) -> String {
    format!("Dziennik: {e}")
}

/// Otwiera bazę Klaudiusza z tabelami dziennika i ustawień
pub fn open() -> Result<Connection, String> {
    let conn = crate::alerts::open()?;
    init(&conn)?;
    Ok(conn)
}

/// Zapisuje zmianę wraz z operacją odwrotną (JSON)
pub fn record(description: &str, undo: &Value) -> Result<(), String> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO journal (created_at, description, undo) VALUES (?1, ?2, ?3)",
        params![crate::dates::now_ms(), description, undo.to_string()],
    )
    .map_err(err)?;
    Ok(())
}

/// Ostatnia zmiana, którą jeszcze można cofnąć: (id, opis, operacja odwrotna)
pub fn last_undoable(conn: &Connection) -> Result<Option<(i64, String, Value)>, String> {
    conn.query_row(
        "SELECT id, description, undo FROM journal WHERE undone = 0 ORDER BY id DESC LIMIT 1",
        [],
        |r| {
            let undo: String = r.get(2)?;
            Ok((r.get(0)?, r.get(1)?, serde_json::from_str(&undo).unwrap_or(Value::Null)))
        },
    )
    .optional()
    .map_err(err)
}

pub fn mark_undone(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("UPDATE journal SET undone = 1 WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

/// Ostatnie wpisy: (kiedy w ms, opis, czy cofnięte)
pub fn recent(conn: &Connection, limit: i64) -> Result<Vec<(i64, String, bool)>, String> {
    let mut stmt = conn
        .prepare("SELECT created_at, description, undone FROM journal ORDER BY id DESC LIMIT ?1")
        .map_err(err)?;
    let rows = stmt
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? == 1)))
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![key, value]).map_err(err)?;
    Ok(())
}
