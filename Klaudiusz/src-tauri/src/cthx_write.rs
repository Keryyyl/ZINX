//! CTHX (zapis): dodawanie i edycja wpisów, odhaczanie wykonania i nawyków.
//! Walidacja jak w CTHX (src-tauri/src/db.rs). Zapis idzie prosto do cthx.sqlite,
//! a CTHX pokaże go po odświeżeniu, tak jak panel EINPX.

use crate::dates::{minutes, now_ms, valid_date, valid_time};
use crate::paths;
use rusqlite::{params, Connection, OptionalExtension};
use std::time::Duration;

const MAX_TITLE: usize = 120;
const MAX_DESCRIPTION: usize = 2000;
const RECURRENCES: [&str; 5] = ["none", "daily", "weekly", "monthly", "yearly"];

#[derive(Debug, Clone, PartialEq)]
pub struct ItemDraft {
    /// „task” albo „event”
    pub kind: String,
    pub title: String,
    pub description: String,
    pub date: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub recurrence: String,
}

/// Zmiany przy edycji. Pole `None` = bez zmian, `Some("")` przy godzinach = usuń godzinę.
#[derive(Debug, Default)]
pub struct Changes {
    pub title: Option<String>,
    pub description: Option<String>,
    pub date: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub recurrence: Option<String>,
}

fn err(e: rusqlite::Error) -> String {
    format!("CTHX: {e}")
}

fn clean(v: &Option<String>) -> Option<String> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

pub fn connect_write() -> Result<Connection, String> {
    let path = paths::cthx();
    if !path.exists() {
        return Err("Nie znaleziono bazy CTHX. Uruchom CTHX przynajmniej raz.".into());
    }
    let conn = Connection::open(&path).map_err(err)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
    conn.pragma_update(None, "foreign_keys", "ON").map_err(err)?;
    Ok(conn)
}

pub fn validate(d: &ItemDraft) -> Result<(), String> {
    if !valid_date(&d.date) {
        return Err("Nieprawidłowa data, podaj RRRR-MM-DD".into());
    }
    if d.title.trim().is_empty() {
        return Err("Podaj tytuł".into());
    }
    if d.title.trim().chars().count() > MAX_TITLE {
        return Err("Tytuł: najwyżej 120 znaków".into());
    }
    if d.description.trim().chars().count() > MAX_DESCRIPTION {
        return Err("Opis jest za długi".into());
    }
    if d.kind != "task" && d.kind != "event" {
        return Err("Nieznany rodzaj wpisu".into());
    }
    if !valid_date(&d.date) {
        return Err("Nieprawidłowa data, podaj RRRR-MM-DD".into());
    }
    if !RECURRENCES.contains(&d.recurrence.as_str()) {
        return Err("Powtarzanie: none, daily, weekly, monthly albo yearly".into());
    }
    if let Some(t) = &d.start {
        if !valid_time(t) {
            return Err("Nieprawidłowa godzina rozpoczęcia (GG:MM)".into());
        }
    }
    if let Some(t) = &d.end {
        if !valid_time(t) {
            return Err("Nieprawidłowa godzina zakończenia (GG:MM)".into());
        }
    }
    if d.kind == "event" {
        match (&d.start, &d.end) {
            (None, None) => {}
            (Some(_), None) => return Err("Wydarzenie z godziną rozpoczęcia potrzebuje też godziny zakończenia".into()),
            (None, Some(_)) => return Err("Podaj godzinę rozpoczęcia albo zostaw wydarzenie całodniowe".into()),
            (Some(s), Some(e)) => {
                if minutes(e) <= minutes(s) {
                    return Err("Godzina zakończenia musi być późniejsza niż rozpoczęcia".into());
                }
            }
        }
    }
    Ok(())
}

pub fn add(conn: &Connection, d: &ItemDraft) -> Result<i64, String> {
    validate(d)?;
    conn.execute(
        "INSERT INTO items (kind, title, description, date, start_time, end_time, recurrence, created_at, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'manual')",
        params![d.kind, d.title.trim(), d.description.trim(), d.date, clean(&d.start), clean(&d.end), d.recurrence, now_ms()],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

/// Wczytuje wpis i sprawdza, czy to właściwy rodzaj (zadanie / wydarzenie)
pub fn load(conn: &Connection, id: i64, kind: &str) -> Result<ItemDraft, String> {
    let row = conn
        .query_row(
            "SELECT kind, title, description, date, start_time, end_time, recurrence FROM items WHERE id = ?1",
            [id],
            |r| {
                Ok(ItemDraft {
                    kind: r.get(0)?,
                    title: r.get(1)?,
                    description: r.get(2)?,
                    date: r.get(3)?,
                    start: r.get(4)?,
                    end: r.get(5)?,
                    recurrence: r.get(6)?,
                })
            },
        )
        .optional()
        .map_err(err)?
        .ok_or_else(|| format!("Nie ma wpisu o numerze {id}"))?;
    if row.kind != kind {
        return Err(format!("Wpis {id} to nie jest {}", if kind == "task" { "zadanie" } else { "wydarzenie" }));
    }
    Ok(row)
}

pub fn apply_changes(mut d: ItemDraft, c: Changes) -> ItemDraft {
    if let Some(t) = c.title {
        d.title = t;
    }
    if let Some(t) = c.description {
        d.description = t;
    }
    if let Some(t) = c.date {
        d.date = t;
    }
    if let Some(t) = c.start {
        d.start = if t.is_empty() { None } else { Some(t) };
    }
    if let Some(t) = c.end {
        d.end = if t.is_empty() { None } else { Some(t) };
    }
    if let Some(t) = c.recurrence {
        d.recurrence = t;
    }
    d
}

pub fn replace(conn: &Connection, id: i64, d: &ItemDraft) -> Result<(), String> {
    validate(d)?;
    conn.execute(
        "UPDATE items SET title = ?1, description = ?2, date = ?3, start_time = ?4, end_time = ?5, recurrence = ?6 WHERE id = ?7",
        params![d.title.trim(), d.description.trim(), d.date, clean(&d.start), clean(&d.end), d.recurrence, id],
    )
    .map_err(err)?;
    Ok(())
}

pub fn item_title(conn: &Connection, id: i64) -> Result<String, String> {
    conn.query_row("SELECT title FROM items WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(err)?
        .ok_or_else(|| format!("Nie ma wpisu o numerze {id}"))
}

pub fn set_item_done(conn: &Connection, id: i64, date: &str, done: bool) -> Result<(), String> {
    item_title(conn, id)?;
    write_done(conn, "item_done", "item_id", id, date, done)
}

/// Znajduje nawyk po nazwie (bez rozróżniania wielkości liter). Dokładne trafienie ma pierwszeństwo.
pub fn find_habit(conn: &Connection, name: &str) -> Result<(i64, String), String> {
    let wanted = name.trim().to_lowercase();
    let mut stmt = conn.prepare("SELECT id, name FROM habits ORDER BY created_at, id").map_err(err)?;
    let all: Vec<(i64, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    if let Some(exact) = all.iter().find(|(_, n)| n.trim().to_lowercase() == wanted) {
        return Ok(exact.clone());
    }
    let partial: Vec<&(i64, String)> = all.iter().filter(|(_, n)| n.to_lowercase().contains(&wanted)).collect();
    match partial.len() {
        0 => Err(format!("Nie znaleziono nawyku „{name}”")),
        1 => Ok(partial[0].clone()),
        _ => Err(format!("Kilka nawyków pasuje do „{name}”: {}", partial.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

pub fn set_habit_done(conn: &Connection, habit_id: i64, date: &str, done: bool) -> Result<(), String> {
    write_done(conn, "habit_done", "habit_id", habit_id, date, done)
}

fn write_done(conn: &Connection, table: &str, column: &str, id: i64, date: &str, done: bool) -> Result<(), String> {
    // nazwy tabel i kolumn pochodzą wyłącznie z kodu powyżej, nie z danych użytkownika
    let sql = if done {
        format!("INSERT OR IGNORE INTO {table} ({column}, date) VALUES (?1, ?2)")
    } else {
        format!("DELETE FROM {table} WHERE {column} = ?1 AND date = ?2")
    };
    conn.execute(&sql, params![id, date]).map_err(err)?;
    Ok(())
}

/// Zapis wpisu do JSON (do cofania)
pub fn draft_to_json(d: &ItemDraft) -> serde_json::Value {
    serde_json::json!({"kind": d.kind, "title": d.title, "description": d.description, "date": d.date, "start": d.start, "end": d.end, "recurrence": d.recurrence})
}

pub fn draft_from_json(v: &serde_json::Value) -> Result<ItemDraft, String> {
    let s = |k: &str| v[k].as_str().map(str::to_string);
    Ok(ItemDraft {
        kind: s("kind").ok_or("brak rodzaju")?,
        title: s("title").ok_or("brak tytułu")?,
        description: s("description").unwrap_or_default(),
        date: s("date").ok_or("brak daty")?,
        start: s("start"),
        end: s("end"),
        recurrence: s("recurrence").unwrap_or_else(|| "none".into()),
    })
}

/// Usuwa wpis wraz z jego odhaczeniami (do cofnięcia dodania)
pub fn delete_item(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM item_done WHERE item_id = ?1", [id]).map_err(err)?;
    conn.execute("DELETE FROM items WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

/// Czy wpis / nawyk jest odhaczony w danym dniu
pub fn is_done(conn: &Connection, table: &str, column: &str, id: i64, date: &str) -> Result<bool, String> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1 AND date = ?2");
    let n: i64 = conn.query_row(&sql, params![id, date], |r| r.get(0)).map_err(err)?;
    Ok(n > 0)
}

pub fn write_done_pub(conn: &Connection, table: &str, column: &str, id: i64, date: &str, done: bool) -> Result<(), String> {
    write_done(conn, table, column, id, date, done)
}

/// Wczytuje wpis dowolnego rodzaju, razem z tym, czy pochodzi z kontaktów (takich nie usuwamy)
pub fn load_any(conn: &Connection, id: i64) -> Result<(ItemDraft, String), String> {
    conn.query_row(
        "SELECT kind, title, description, date, start_time, end_time, recurrence, source FROM items WHERE id = ?1",
        [id],
        |r| {
            Ok((
                ItemDraft {
                    kind: r.get(0)?,
                    title: r.get(1)?,
                    description: r.get(2)?,
                    date: r.get(3)?,
                    start: r.get(4)?,
                    end: r.get(5)?,
                    recurrence: r.get(6)?,
                },
                r.get::<_, String>(7)?,
            ))
        },
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| format!("Nie ma wpisu o numerze {id}"))
}

/// Przywraca wpis pod tym samym numerem (cofnięcie usunięcia)
pub fn insert_with_id(conn: &Connection, id: i64, d: &ItemDraft) -> Result<(), String> {
    validate(d)?;
    conn.execute(
        "INSERT INTO items (id, kind, title, description, date, start_time, end_time, recurrence, created_at, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'manual')",
        params![id, d.kind, d.title.trim(), d.description.trim(), d.date, clean(&d.start), clean(&d.end), d.recurrence, now_ms()],
    )
    .map_err(err)?;
    Ok(())
}

/// Szuka wpisów po fragmencie tytułu lub opisu (bez rozróżniania wielkości liter)
pub fn find_items(conn: &Connection, query: &str, limit: i64) -> Result<Vec<(i64, ItemDraft)>, String> {
    let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, title, description, date, start_time, end_time, recurrence FROM items
              WHERE title LIKE ?1 ESCAPE '\\' OR description LIKE ?1 ESCAPE '\\'
              ORDER BY date DESC, id DESC LIMIT ?2",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map((pattern, limit), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                ItemDraft {
                    kind: r.get(1)?,
                    title: r.get(2)?,
                    description: r.get(3)?,
                    date: r.get(4)?,
                    start: r.get(5)?,
                    end: r.get(6)?,
                    recurrence: r.get(7)?,
                },
            ))
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}
