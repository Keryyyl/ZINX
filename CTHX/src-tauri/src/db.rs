//! Baza SQLite: jedna tabela na zadania i wydarzenia, druga na wykonane wystąpienia.
//! Powtarzalność (codziennie, co tydzień, co miesiąc) liczy interfejs na podstawie daty startu.

use crate::error::{Error, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS items (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('task', 'event')),
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  date TEXT NOT NULL,
  start_time TEXT,
  end_time TEXT,
  recurrence TEXT NOT NULL DEFAULT 'none' CHECK (recurrence IN ('none', 'daily', 'weekly', 'monthly', 'yearly')),
  created_at INTEGER NOT NULL,
  source TEXT NOT NULL DEFAULT 'manual',
  external_id TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS items_external ON items(source, external_id);
CREATE TABLE IF NOT EXISTS habits (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  goal INTEGER NOT NULL CHECK (goal BETWEEN 1 AND 31),
  start_date TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS habit_done (
  habit_id INTEGER NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
  date TEXT NOT NULL,
  PRIMARY KEY (habit_id, date)
);
CREATE TABLE IF NOT EXISTS item_done (
  item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  date TEXT NOT NULL,
  PRIMARY KEY (item_id, date)
);
CREATE TABLE IF NOT EXISTS kanban_tasks (
  id INTEGER PRIMARY KEY,
  month TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('todo', 'doing', 'done')),
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  position INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS kanban_month ON kanban_tasks(month, status);
";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub description: String,
    /// pierwszy dzień wystąpienia, RRRR-MM-DD
    pub date: String,
    /// GG:MM albo brak
    pub start_time: Option<String>,
    /// GG:MM, tylko wydarzenia
    pub end_time: Option<String>,
    pub recurrence: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInput {
    /// brak = nowy wpis
    pub id: Option<i64>,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub date: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub recurrence: String,
}

/// Wykonane wystąpienie: id zadania i dzień, którego dotyczy
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub item_id: i64,
    pub date: String,
}

/// Wspólny uchwyt do bazy. Blokadę trzymamy krótko.
#[derive(Clone)]
pub struct Db(std::sync::Arc<std::sync::Mutex<Connection>>);

impl Db {
    pub fn new(conn: Connection) -> Self {
        Db(std::sync::Arc::new(std::sync::Mutex::new(conn)))
    }

    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.0.lock().map_err(|_| Error::from("Baza danych jest niedostępna"))
    }
}

const MAX_TITLE: usize = 120;
const MAX_DESCRIPTION: usize = 2000;

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    init(conn)
}

#[cfg(test)]
pub fn open_memory() -> Connection {
    init(Connection::open_in_memory().unwrap()).unwrap()
}

fn init(conn: Connection) -> Result<Connection> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // przebudowa tabeli wymaga wyłączonych kluczy obcych
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    migrate(&conn)?;
    conn.execute_batch(SCHEMA)?;
    crate::kanban::migrate(&conn)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Starsza tabela wpisów (bez pól źródła i z bez „co rok”) jest przebudowywana, dane zostają.
fn migrate(conn: &Connection) -> Result<()> {
    let has_items: bool = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'items'", [], |r| r.get::<_, i64>(0))? > 0;
    if !has_items {
        return Ok(());
    }
    let mut stmt = conn.prepare("PRAGMA table_info(items)")?;
    let columns: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<std::result::Result<_, _>>()?;
    if columns.iter().any(|c| c == "external_id") {
        return Ok(());
    }
    // legacy_alter_table: zmiana nazwy nie przepisuje kluczy obcych w item_done
    conn.pragma_update(None, "legacy_alter_table", "ON")?;
    conn.execute_batch(
        "BEGIN;
         ALTER TABLE items RENAME TO items_old;
         CREATE TABLE items (
           id INTEGER PRIMARY KEY,
           kind TEXT NOT NULL CHECK (kind IN ('task', 'event')),
           title TEXT NOT NULL,
           description TEXT NOT NULL DEFAULT '',
           date TEXT NOT NULL,
           start_time TEXT,
           end_time TEXT,
           recurrence TEXT NOT NULL DEFAULT 'none' CHECK (recurrence IN ('none', 'daily', 'weekly', 'monthly', 'yearly')),
           created_at INTEGER NOT NULL,
           source TEXT NOT NULL DEFAULT 'manual',
           external_id TEXT
         );
         INSERT INTO items (id, kind, title, description, date, start_time, end_time, recurrence, created_at)
           SELECT id, kind, title, description, date, start_time, end_time, recurrence, created_at FROM items_old;
         DROP TABLE items_old;
         COMMIT;",
    )?;
    conn.pragma_update(None, "legacy_alter_table", "OFF")?;
    Ok(())
}

pub fn valid_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let nums: Option<Vec<u32>> = parts.iter().map(|p| p.parse().ok()).collect();
    matches!(nums, Some(n) if (1..=12).contains(&n[1]) && (1..=31).contains(&n[2]))
}

/// GG:MM, godzina 00–23, minuty 00–59
fn valid_time(text: &str) -> bool {
    let Some((h, m)) = text.split_once(':') else { return false };
    if h.len() != 2 || m.len() != 2 {
        return false;
    }
    matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60)
}

fn minutes(text: &str) -> u32 {
    let (h, m) = text.split_once(':').unwrap_or(("0", "0"));
    h.parse::<u32>().unwrap_or(0) * 60 + m.parse::<u32>().unwrap_or(0)
}

/// Sprawdza wejście zanim trafi do bazy. Zwraca oczyszczone wartości.
fn check(input: &ItemInput) -> Result<ItemInput> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(Error::from("Podaj tytuł"));
    }
    if title.chars().count() > MAX_TITLE {
        return Err(Error::from("Tytuł: najwyżej 120 znaków"));
    }
    let description = input.description.trim().to_string();
    if description.chars().count() > MAX_DESCRIPTION {
        return Err(Error::from("Opis jest za długi"));
    }
    if input.kind != "task" && input.kind != "event" {
        return Err(Error::from("Nieznany rodzaj wpisu"));
    }
    if !matches!(input.recurrence.as_str(), "none" | "daily" | "weekly" | "monthly" | "yearly") {
        return Err(Error::from("Nieznana powtarzalność"));
    }
    if !valid_date(&input.date) {
        return Err(Error::from("Nieprawidłowa data"));
    }
    let start = match input.start_time.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if !valid_time(t) => return Err(Error::from("Nieprawidłowa godzina")),
        Some(t) => Some(t.to_string()),
        None => None,
    };
    let end = match input.end_time.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if !valid_time(t) => return Err(Error::from("Nieprawidłowa godzina zakończenia")),
        Some(t) => Some(t.to_string()),
        None => None,
    };
    // wydarzenie bez godziny jest całodniowe (np. urodziny); z godziną potrzebuje też końca
    if input.kind == "event" {
        match (&start, &end) {
            (None, None) => {}
            (Some(_), None) => return Err(Error::from("Wydarzenie z godziną rozpoczęcia potrzebuje też godziny zakończenia")),
            (None, Some(_)) => return Err(Error::from("Podaj godzinę rozpoczęcia albo zostaw wydarzenie całodniowe")),
            (Some(s), Some(e)) => {
                if minutes(e) <= minutes(s) {
                    return Err(Error::from("Godzina zakończenia musi być późniejsza niż rozpoczęcia"));
                }
            }
        }
    }
    let has_start = start.is_some();
    Ok(ItemInput {
        id: input.id,
        kind: input.kind.clone(),
        title,
        description,
        date: input.date.clone(),
        start_time: start,
        end_time: if input.kind == "event" && has_start { end } else { None },
        recurrence: input.recurrence.clone(),
    })
}

pub fn list(conn: &Connection) -> Result<Vec<Item>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, description, date, start_time, end_time, recurrence
         FROM items ORDER BY date, COALESCE(start_time, '99:99'), id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Item {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            description: r.get(3)?,
            date: r.get(4)?,
            start_time: r.get(5)?,
            end_time: r.get(6)?,
            recurrence: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn done_list(conn: &Connection) -> Result<Vec<Done>> {
    let mut stmt = conn.prepare("SELECT item_id, date FROM item_done")?;
    let rows = stmt.query_map([], |r| Ok(Done { item_id: r.get(0)?, date: r.get(1)? }))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Dodaje albo zmienia wpis; zwraca jego id.
pub fn save(conn: &Connection, input: &ItemInput) -> Result<i64> {
    let clean = check(input)?;
    match clean.id {
        Some(id) => {
            let changed = conn.execute(
                "UPDATE items SET kind = ?1, title = ?2, description = ?3, date = ?4, start_time = ?5, end_time = ?6, recurrence = ?7
                 WHERE id = ?8",
                params![clean.kind, clean.title, clean.description, clean.date, clean.start_time, clean.end_time, clean.recurrence, id],
            )?;
            if changed == 0 {
                return Err(Error::from("Nie znaleziono wpisu"));
            }
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO items (kind, title, description, date, start_time, end_time, recurrence, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![clean.kind, clean.title, clean.description, clean.date, clean.start_time, clean.end_time, clean.recurrence, crate::now_ms()],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
    Ok(())
}

/// Oznacza wystąpienie jako zrobione albo cofa oznaczenie.
pub fn set_done(conn: &Connection, item_id: i64, date: &str, done: bool) -> Result<()> {
    if !valid_date(date) {
        return Err(Error::from("Nieprawidłowa data"));
    }
    if done {
        conn.execute("INSERT OR IGNORE INTO item_done (item_id, date) VALUES (?1, ?2)", params![item_id, date])?;
    } else {
        conn.execute("DELETE FROM item_done WHERE item_id = ?1 AND date = ?2", params![item_id, date])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: &str) -> ItemInput {
        ItemInput {
            id: None,
            kind: kind.into(),
            title: "  Spotkanie  ".into(),
            description: "".into(),
            date: "2026-11-07".into(),
            start_time: Some("09:00".into()),
            end_time: Some("10:00".into()),
            recurrence: "none".into(),
        }
    }

    #[test]
    fn save_trims_and_lists() {
        let conn = open_memory();
        let id = save(&conn, &input("event")).unwrap();
        let items = list(&conn).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].title, "Spotkanie");
        assert_eq!(items[0].end_time.as_deref(), Some("10:00"));
    }

    #[test]
    fn event_needs_valid_range() {
        let conn = open_memory();
        let mut bad = input("event");
        bad.end_time = Some("08:00".into());
        assert!(save(&conn, &bad).is_err());
        let mut no_end = input("event");
        no_end.end_time = None;
        assert!(save(&conn, &no_end).is_err());
    }

    #[test]
    fn task_drops_end_time_and_checks_format() {
        let conn = open_memory();
        let mut task = input("task");
        task.end_time = Some("11:00".into());
        save(&conn, &task).unwrap();
        assert_eq!(list(&conn).unwrap()[0].end_time, None);

        let mut wrong_time = input("task");
        wrong_time.start_time = Some("25:00".into());
        assert!(save(&conn, &wrong_time).is_err());
        let mut wrong_date = input("task");
        wrong_date.date = "07.11.2026".into();
        assert!(save(&conn, &wrong_date).is_err());
    }

    #[test]
    fn update_and_delete_and_done() {
        let conn = open_memory();
        let id = save(&conn, &input("task")).unwrap();
        let mut edit = input("task");
        edit.id = Some(id);
        edit.title = "Nowy tytuł".into();
        save(&conn, &edit).unwrap();
        assert_eq!(list(&conn).unwrap()[0].title, "Nowy tytuł");

        set_done(&conn, id, "2026-11-07", true).unwrap();
        assert_eq!(done_list(&conn).unwrap(), vec![Done { item_id: id, date: "2026-11-07".into() }]);
        set_done(&conn, id, "2026-11-07", false).unwrap();
        assert!(done_list(&conn).unwrap().is_empty());

        delete(&conn, id).unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }

    /// Starsza baza (sprzed urodzin) zachowuje wpisy i ich wykonania po migracji.
    #[test]
    fn old_items_table_is_migrated_with_done_marks() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE items (
               id INTEGER PRIMARY KEY,
               kind TEXT NOT NULL CHECK (kind IN ('task', 'event')),
               title TEXT NOT NULL,
               description TEXT NOT NULL DEFAULT '',
               date TEXT NOT NULL,
               start_time TEXT,
               end_time TEXT,
               recurrence TEXT NOT NULL DEFAULT 'none' CHECK (recurrence IN ('none', 'daily', 'weekly', 'monthly')),
               created_at INTEGER NOT NULL
             );
             CREATE TABLE item_done (
               item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
               date TEXT NOT NULL,
               PRIMARY KEY (item_id, date)
             );
             INSERT INTO items (id, kind, title, date, recurrence, created_at) VALUES (5, 'task', 'Stare zadanie', '2026-11-07', 'daily', 1);
             INSERT INTO item_done (item_id, date) VALUES (5, '2026-11-07');",
        )
        .unwrap();
        let conn = init(conn).unwrap();
        let items = list(&conn).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Stare zadanie");
        assert_eq!(items[0].recurrence, "daily");
        assert_eq!(done_list(&conn).unwrap(), vec![Done { item_id: 5, date: "2026-11-07".into() }]);
        // nowa cykliczność działa już po migracji
        let mut yearly = input("event");
        yearly.start_time = None;
        yearly.end_time = None;
        yearly.recurrence = "yearly".into();
        save(&conn, &yearly).unwrap();
    }
}
