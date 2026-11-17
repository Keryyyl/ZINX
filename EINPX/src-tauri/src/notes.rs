//! Szybkie notatki: własna baza EINPX (nie baza innej aplikacji), w folderze danych tej aplikacji.

use crate::dates::now_ms;
use crate::paths;
use rusqlite::{params, Connection};
use serde::Serialize;

const MAX_LEN: usize = 2000;
const LIST_LIMIT: i64 = 20;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: i64,
    pub text: String,
    /// milisekundy od epoki
    pub created_at: i64,
}

fn open() -> Result<Connection, String> {
    let path = paths::einpx();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Notatki: {e}"))?;
    }
    let conn = Connection::open(&path).map_err(|e| format!("Notatki: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
           id INTEGER PRIMARY KEY,
           text TEXT NOT NULL,
           created_at INTEGER NOT NULL
         );",
    )
    .map_err(|e| format!("Notatki: {e}"))?;
    Ok(conn)
}

pub fn list() -> Result<Vec<Note>, String> {
    let conn = open()?;
    let mut stmt = conn
        .prepare("SELECT id, text, created_at FROM notes ORDER BY created_at DESC, id DESC LIMIT ?1")
        .map_err(|e| format!("Notatki: {e}"))?;
    let rows = stmt
        .query_map([LIST_LIMIT], |r| Ok(Note { id: r.get(0)?, text: r.get(1)?, created_at: r.get(2)? }))
        .map_err(|e| format!("Notatki: {e}"))?;
    rows.collect::<Result<_, _>>().map_err(|e| format!("Notatki: {e}"))
}

pub fn add(text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Notatka jest pusta".into());
    }
    if text.chars().count() > MAX_LEN {
        return Err("Notatka jest za długa (najwyżej 2000 znaków)".into());
    }
    let conn = open()?;
    conn.execute("INSERT INTO notes (text, created_at) VALUES (?1, ?2)", params![text, now_ms()])
        .map_err(|e| format!("Notatki: {e}"))?;
    Ok(())
}

pub fn delete(id: i64) -> Result<(), String> {
    let conn = open()?;
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id]).map_err(|e| format!("Notatki: {e}"))?;
    Ok(())
}
