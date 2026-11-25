//! Kanban: strony (np. „Prezenty”, „Programowanie”), a na każdej stronie zadania w trzech kolumnach,
//! osobno dla każdego miesiąca. Zadania „Do zrobienia” z wcześniejszych miesięcy przechodzą do bieżącego miesiąca.

use crate::error::{Error, Result};
use crate::now_ms;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const MAX_TITLE: usize = 120;
const MAX_DESCRIPTION: usize = 2000;
const MAX_BOARD_NAME: usize = 40;
const STATUSES: [&str; 3] = ["todo", "doing", "done"];
/// Strona tworzona przy pierwszym uruchomieniu z nowym kanbanem, żeby przypiąć do niej istniejące zadania
const DEFAULT_BOARD: &str = "Ogólne";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KanbanTask {
    pub id: i64,
    /// RRRR-MM
    pub month: String,
    /// todo | doing | done
    pub status: String,
    pub title: String,
    pub description: String,
    pub board_id: i64,
    pub board_name: String,
    /// zadanie z kalendarza, z którym karta jest połączona
    pub item_id: Option<i64>,
    /// dzień tego zadania z kalendarza (RRRR-MM-DD), gdy karta jest połączona
    pub item_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KanbanBoard {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KanbanInput {
    /// brak = nowe zadanie
    pub id: Option<i64>,
    pub month: String,
    pub status: String,
    pub title: String,
    pub description: String,
    /// strona, na której leży zadanie (przy zmianie strony zadanie trafia na jej koniec)
    pub board_id: i64,
    /// zadanie z kalendarza do połączenia (tylko przy tworzeniu); tytuł i dzień bierze się z niego
    #[serde(default)]
    pub item_id: Option<i64>,
}

fn valid_month(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 7 && b[4] == b'-' && b[..4].iter().all(u8::is_ascii_digit) && b[5..].iter().all(u8::is_ascii_digit) && (1..=12).contains(&text[5..].parse::<u32>().unwrap_or(0))
}

/// Tworzy tabelę stron i dodaje kolumnę `board_id`. Przy pierwszym dodaniu kolumny tworzy stronę „Ogólne”
/// i przypina do niej wszystkie istniejące zadania. Później nie wraca, nawet gdy wszystkie strony zostaną usunięte.
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kanban_boards (
           id INTEGER PRIMARY KEY,
           name TEXT NOT NULL UNIQUE,
           position INTEGER NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL
         );",
    )?;
    let mut stmt = conn.prepare("PRAGMA table_info(kanban_tasks)")?;
    let columns: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<std::result::Result<_, _>>()?;
    if !columns.iter().any(|c| c == "item_id") {
        // połączenie z zadaniem z kalendarza; usunięcie zadania odłącza kartę, tytuł zostaje
        conn.execute("ALTER TABLE kanban_tasks ADD COLUMN item_id INTEGER REFERENCES items(id) ON DELETE SET NULL", [])?;
    }
    if columns.iter().any(|c| c == "board_id") {
        return Ok(());
    }
    conn.execute_batch("BEGIN;")?;
    let result = (|| -> Result<()> {
        conn.execute(
            "ALTER TABLE kanban_tasks ADD COLUMN board_id INTEGER REFERENCES kanban_boards(id) ON DELETE CASCADE",
            [],
        )?;
        conn.execute("INSERT INTO kanban_boards (name, position, created_at) VALUES (?1, 1, ?2)", params![DEFAULT_BOARD, now_ms()])?;
        let id = conn.last_insert_rowid();
        conn.execute("UPDATE kanban_tasks SET board_id = ?1 WHERE board_id IS NULL", [id])?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK;")?;
            Err(e)
        }
    }
}

/// Strony w kolejności, w jakiej są dodane
pub fn boards(conn: &Connection) -> Result<Vec<KanbanBoard>> {
    let mut stmt = conn.prepare("SELECT id, name FROM kanban_boards ORDER BY position, id")?;
    let rows = stmt.query_map([], |r| Ok(KanbanBoard { id: r.get(0)?, name: r.get(1)? }))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn check_board_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::from("Podaj nazwę strony"));
    }
    if name.chars().count() > MAX_BOARD_NAME {
        return Err(Error::from("Nazwa strony: najwyżej 40 znaków"));
    }
    Ok(name)
}

/// Dodaje stronę. Nazwy nie rozróżniają wielkości liter.
pub fn create_board(conn: &Connection, name: &str) -> Result<i64> {
    let name = check_board_name(name)?;
    let taken: bool = conn.query_row("SELECT COUNT(*) FROM kanban_boards WHERE name = ?1 COLLATE NOCASE", [name], |r| r.get::<_, i64>(0))? > 0;
    if taken {
        return Err(Error::from("Strona o tej nazwie już istnieje"));
    }
    conn.execute(
        "INSERT INTO kanban_boards (name, position, created_at)
         VALUES (?1, (SELECT COALESCE(MAX(position), 0) + 1 FROM kanban_boards), ?2)",
        params![name, now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Ile zadań ma strona (w całości, ze wszystkich miesięcy). Potrzebne do ostrzeżenia przed usunięciem.
pub fn board_task_count(conn: &Connection, id: i64) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM kanban_tasks WHERE board_id = ?1", [id], |r| r.get(0))?)
}

/// Usuwa stronę razem z jej zadaniami
pub fn delete_board(conn: &Connection, id: i64) -> Result<()> {
    conn.execute_batch("BEGIN;")?;
    let result = (|| -> Result<()> {
        conn.execute("DELETE FROM kanban_tasks WHERE board_id = ?1", [id])?;
        conn.execute("DELETE FROM kanban_boards WHERE id = ?1", [id])?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK;")?;
            Err(e)
        }
    }
}

/// Zadania miesiąca ze wszystkich stron. Gdy `month` jest bieżącym miesiącem, zaległe „Do zrobienia” przenoszą się tutaj.
pub fn list(conn: &Connection, month: &str, current_month: &str) -> Result<Vec<KanbanTask>> {
    if !valid_month(month) || !valid_month(current_month) {
        return Err(Error::from("Zły format miesiąca (oczekiwano RRRR-MM)"));
    }
    if month == current_month {
        // zadania połączone z kalendarzem nie przechodzą same: ich dzień wyznacza wpis
        conn.execute(
            "UPDATE kanban_tasks SET month = ?1 WHERE status = 'todo' AND month < ?1 AND item_id IS NULL",
            params![month],
        )?;
    }
    sync_linked(conn)?;
    let mut stmt = conn.prepare(
        "SELECT t.id, t.month, t.status, t.title, t.description, t.board_id, b.name, t.item_id, i.date
         FROM kanban_tasks t
         JOIN kanban_boards b ON b.id = t.board_id
         LEFT JOIN items i ON i.id = t.item_id
         WHERE t.month = ?1
         ORDER BY t.position, t.id",
    )?;
    let rows = stmt.query_map([month], |r| {
        Ok(KanbanTask {
            id: r.get(0)?,
            month: r.get(1)?,
            status: r.get(2)?,
            title: r.get(3)?,
            description: r.get(4)?,
            board_id: r.get(5)?,
            board_name: r.get(6)?,
            item_id: r.get(7)?,
            item_date: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Tytuł i dzień zadania z kalendarza (tylko zadania, wydarzenia nie)
fn calendar_task(conn: &Connection, item_id: i64) -> Result<(String, String)> {
    conn.query_row("SELECT title, date FROM items WHERE id = ?1 AND kind = 'task'", [item_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| Error::from("Nie ma takiego zadania w kalendarzu"))
}

/// Karty połączone z kalendarzem biorą tytuł i miesiąc z zadania. Wołane po zmianie wpisu i przy odczycie tablicy.
pub fn sync_linked(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE kanban_tasks SET
           title = (SELECT i.title FROM items i WHERE i.id = kanban_tasks.item_id),
           month = substr((SELECT i.date FROM items i WHERE i.id = kanban_tasks.item_id), 1, 7)
         WHERE item_id IS NOT NULL AND EXISTS (SELECT 1 FROM items i WHERE i.id = kanban_tasks.item_id)",
        [],
    )?;
    Ok(())
}

fn check(input: &KanbanInput) -> Result<()> {
    if input.title.trim().is_empty() {
        return Err(Error::from("Podaj tytuł zadania"));
    }
    if input.title.trim().chars().count() > MAX_TITLE {
        return Err(Error::from("Tytuł: najwyżej 120 znaków"));
    }
    if input.description.trim().chars().count() > MAX_DESCRIPTION {
        return Err(Error::from("Opis jest za długi"));
    }
    if !STATUSES.contains(&input.status.as_str()) {
        return Err(Error::from("Nieznana kolumna"));
    }
    if !valid_month(&input.month) {
        return Err(Error::from("Zły format miesiąca (oczekiwano RRRR-MM)"));
    }
    Ok(())
}

/// Dodaje zadanie albo zmienia je (gdy jest id). Zmiana kolumny albo strony ustawia zadanie na końcu tej kolumny.
/// Zadanie połączone z kalendarzem ma tytuł i dzień z wpisu; formularz zmienia tylko opis, stronę i kolumnę.
/// Zapis karty. Gdy karta jest połączona z zadaniem i zmienia się jej status z „Zrobione” albo na „Zrobione”,
/// oznaczenie zadania w kalendarzu idzie za nią (w dniu zadania).
pub fn save(conn: &Connection, input: &KanbanInput) -> Result<i64> {
    let before: Option<String> = match input.id {
        Some(id) => conn.query_row("SELECT status FROM kanban_tasks WHERE id = ?1", [id], |r| r.get(0)).optional()?,
        None => None,
    };
    let id = save_card(conn, input)?;
    let (status, item_id): (String, Option<i64>) =
        conn.query_row("SELECT status, item_id FROM kanban_tasks WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let crossed_done = before.as_deref() != Some(status.as_str()) && (before.as_deref() == Some("done") || status == "done");
    if let (Some(item_id), true) = (item_id, crossed_done) {
        let day: Option<String> = conn.query_row("SELECT date FROM items WHERE id = ?1", [item_id], |r| r.get(0)).optional()?;
        if let Some(day) = day {
            crate::db::set_done(conn, item_id, &day, status == "done")?;
        }
    }
    Ok(id)
}

/// Oznaczenie zadania w kalendarzu przenosi połączoną kartę do „Zrobione” (albo z powrotem do „Do zrobienia”).
/// Dotyczy tylko dnia zadania, czyli dnia karty.
pub fn on_calendar_done(conn: &Connection, item_id: i64, date: &str, done: bool) -> Result<()> {
    let item_day: Option<String> = conn.query_row("SELECT date FROM items WHERE id = ?1", [item_id], |r| r.get(0)).optional()?;
    if item_day.as_deref() != Some(date) {
        return Ok(());
    }
    let target = if done { "done" } else { "todo" };
    let cards: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare("SELECT id, status, month FROM kanban_tasks WHERE item_id = ?1")?;
        let rows = stmt.query_map([item_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for (id, status, month) in cards {
        let needs_move = if done { status != "done" } else { status == "done" };
        if needs_move {
            conn.execute(
                "UPDATE kanban_tasks SET status = ?2,
                   position = (SELECT COALESCE(MAX(position), 0) + 1 FROM kanban_tasks WHERE month = ?3 AND status = ?2)
                 WHERE id = ?1",
                params![id, target, month],
            )?;
        }
    }
    Ok(())
}

fn save_card(conn: &Connection, input: &KanbanInput) -> Result<i64> {
    let mut input = input.clone();
    let linked = match input.id {
        Some(id) => conn
            .query_row("SELECT item_id FROM kanban_tasks WHERE id = ?1", [id], |r| r.get::<_, Option<i64>>(0))
            .optional()?
            .ok_or_else(|| Error::from("Nie znaleziono zadania"))?,
        None => input.item_id,
    };
    if let Some(item_id) = linked {
        let (title, date) = calendar_task(conn, item_id)?;
        input.title = title;
        input.month = date.chars().take(7).collect();
    }
    check(&input)?;
    let title = input.title.trim();
    let description = input.description.trim();
    let board_exists: bool = conn.query_row("SELECT COUNT(*) FROM kanban_boards WHERE id = ?1", [input.board_id], |r| r.get::<_, i64>(0))? > 0;
    if !board_exists {
        return Err(Error::from("Nie ma takiej strony"));
    }
    match input.id {
        Some(id) => {
            let (old_status, old_board, month): (String, i64, String) = conn
                .query_row("SELECT status, board_id, month FROM kanban_tasks WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?
                .ok_or_else(|| Error::from("Nie znaleziono zadania"))?;
            if old_status == input.status && old_board == input.board_id {
                conn.execute("UPDATE kanban_tasks SET title = ?2, description = ?3 WHERE id = ?1", params![id, title, description])?;
            } else {
                conn.execute(
                    "UPDATE kanban_tasks SET status = ?2, title = ?3, description = ?4, board_id = ?5,
                       position = (SELECT COALESCE(MAX(position), 0) + 1 FROM kanban_tasks WHERE month = ?6 AND status = ?2)
                     WHERE id = ?1",
                    params![id, input.status, title, description, input.board_id, month],
                )?;
            }
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO kanban_tasks (month, status, title, description, position, created_at, board_id, item_id)
                 VALUES (?1, ?2, ?3, ?4,
                   (SELECT COALESCE(MAX(position), 0) + 1 FROM kanban_tasks WHERE month = ?1 AND status = ?2),
                   ?5, ?6, ?7)",
                params![input.month, input.status, title, description, now_ms(), input.board_id, linked],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// Przesuwa kartę do kolumny `status` i ustawia ją przed kartą `before_id` (`None` = na koniec kolumny).
/// Zmiana kolumny idzie przez zwykły zapis (z synchronizacją „Zrobione” z kalendarzem), potem kolumna dostaje
/// nowe, kolejne pozycje, więc kolejność jest taka, jaką widać po przeciągnięciu.
pub fn move_card(conn: &Connection, id: i64, status: &str, before_id: Option<i64>) -> Result<()> {
    if !STATUSES.contains(&status) {
        return Err(Error::from("Nieznana kolumna"));
    }
    if before_id == Some(id) {
        return Ok(());
    }
    let (month, current, board_id, title, description, item_id): (String, String, i64, String, String, Option<i64>) = conn
        .query_row(
            "SELECT month, status, board_id, title, description, item_id FROM kanban_tasks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?
        .ok_or_else(|| Error::from("Nie znaleziono zadania"))?;
    if current != status {
        save(
            conn,
            &KanbanInput { id: Some(id), month: month.clone(), status: status.into(), title, description, board_id, item_id },
        )?;
    }
    let mut order: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM kanban_tasks WHERE month = ?1 AND status = ?2 AND id != ?3 ORDER BY position, id")?;
        let rows = stmt.query_map(params![month, status, id], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let at = before_id.and_then(|b| order.iter().position(|&x| x == b)).unwrap_or(order.len());
    order.insert(at, id);
    for (i, card) in order.iter().enumerate() {
        conn.execute("UPDATE kanban_tasks SET position = ?2 WHERE id = ?1", params![card, i as i64 + 1])?;
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM kanban_tasks WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    /// Strona „Ogólne” powstaje przy pierwszym uruchomieniu, więc testy biorą jej id stąd
    fn general(conn: &Connection) -> i64 {
        boards(conn).unwrap().into_iter().find(|b| b.name == DEFAULT_BOARD).unwrap().id
    }

    fn input(id: Option<i64>, month: &str, status: &str, title: &str, board_id: i64) -> KanbanInput {
        KanbanInput { id, month: month.into(), status: status.into(), title: title.into(), description: String::new(), board_id, item_id: None }
    }

    #[test]
    fn add_move_edit_delete() {
        let conn = open_memory();
        let g = general(&conn);
        let a = save(&conn, &input(None, "2026-11", "todo", "A", g)).unwrap();
        let b = save(&conn, &input(None, "2026-11", "todo", "B", g)).unwrap();
        let tasks = list(&conn, "2026-11", "2026-11").unwrap();
        assert_eq!(tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);

        // przeniesienie do „W trakcie” zmienia kolumnę, a tytuł zostaje
        save(&conn, &input(Some(a), "2026-11", "doing", "A", g)).unwrap();
        let tasks = list(&conn, "2026-11", "2026-11").unwrap();
        assert_eq!(tasks.iter().find(|t| t.id == a).unwrap().status, "doing");

        // edycja tytułu bez zmiany kolumny i strony
        save(&conn, &input(Some(b), "2026-11", "todo", "B2", g)).unwrap();
        assert_eq!(list(&conn, "2026-11", "2026-11").unwrap().iter().find(|t| t.id == b).unwrap().title, "B2");

        delete(&conn, a).unwrap();
        assert_eq!(list(&conn, "2026-11", "2026-11").unwrap().len(), 1);
    }

    #[test]
    fn unfinished_todo_rolls_into_current_month_only() {
        let conn = open_memory();
        let g = general(&conn);
        let todo = save(&conn, &input(None, "2026-09", "todo", "zaległe", g)).unwrap();
        let doing = save(&conn, &input(None, "2026-09", "doing", "w trakcie", g)).unwrap();
        let done = save(&conn, &input(None, "2026-09", "done", "gotowe", g)).unwrap();

        // inny miesiąc niż bieżący: nic się nie przenosi
        assert!(list(&conn, "2026-10", "2026-11").unwrap().is_empty());
        // bieżący miesiąc: tylko „Do zrobienia” przechodzi
        let now = list(&conn, "2026-11", "2026-11").unwrap();
        assert_eq!(now.iter().map(|t| t.id).collect::<Vec<_>>(), [todo]);
        assert_eq!(now[0].month, "2026-11");
        let sept = list(&conn, "2026-09", "2026-11").unwrap();
        assert_eq!(sept.iter().map(|t| t.id).collect::<Vec<_>>(), [doing, done]);
    }

    #[test]
    fn rejects_bad_input() {
        let conn = open_memory();
        let g = general(&conn);
        assert!(save(&conn, &input(None, "2026-11", "todo", "   ", g)).is_err());
        assert!(save(&conn, &input(None, "2026-11", "archive", "x", g)).is_err());
        assert!(save(&conn, &input(None, "2026-13", "todo", "x", g)).is_err());
        assert!(save(&conn, &input(None, "2026-11", "todo", "x", 9999)).is_err());
    }

    #[test]
    fn default_board_exists_once() {
        let conn = open_memory();
        assert_eq!(boards(&conn).unwrap().iter().filter(|b| b.name == DEFAULT_BOARD).count(), 1);
        // ponowne uruchomienie (migracja już była) nie dodaje drugiej strony
        migrate(&conn).unwrap();
        assert_eq!(boards(&conn).unwrap().len(), 1);
    }

    #[test]
    fn boards_keep_tasks_apart_and_delete_with_them() {
        let conn = open_memory();
        let g = general(&conn);
        let prezenty = create_board(&conn, "Prezenty").unwrap();
        assert!(create_board(&conn, "prezenty").is_err(), "nazwy bez względu na wielkość liter nie mogą się powtarzać");
        assert!(create_board(&conn, "   ").is_err());

        save(&conn, &input(None, "2026-11", "todo", "kupić książkę", prezenty)).unwrap();
        save(&conn, &input(None, "2026-11", "todo", "zadanie ogólne", g)).unwrap();
        let all = list(&conn, "2026-11", "2026-11").unwrap();
        assert_eq!(all.iter().find(|t| t.title == "kupić książkę").unwrap().board_name, "Prezenty");
        assert_eq!(board_task_count(&conn, prezenty).unwrap(), 1);

        // zmiana strony przenosi zadanie
        let id = all.iter().find(|t| t.title == "zadanie ogólne").unwrap().id;
        save(&conn, &input(Some(id), "2026-11", "todo", "zadanie ogólne", prezenty)).unwrap();
        assert_eq!(board_task_count(&conn, prezenty).unwrap(), 2);

        delete_board(&conn, prezenty).unwrap();
        assert!(list(&conn, "2026-11", "2026-11").unwrap().is_empty());
        assert!(boards(&conn).unwrap().iter().all(|b| b.id != prezenty));
    }

    fn calendar_task_row(conn: &Connection, title: &str, date: &str) -> i64 {
        conn.execute(
            "INSERT INTO items (kind, title, description, date, recurrence, created_at, source)
             VALUES ('task', ?1, 'opis z kalendarza', ?2, 'none', 0, 'manual')",
            params![title, date],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn linked(id: Option<i64>, board_id: i64, item_id: Option<i64>, title: &str, description: &str) -> KanbanInput {
        KanbanInput { id, month: "2026-11".into(), status: "todo".into(), title: title.into(), description: description.into(), board_id, item_id }
    }

    #[test]
    fn linked_card_takes_title_and_date_from_calendar() {
        let conn = open_memory();
        let g = general(&conn);
        let item = calendar_task_row(&conn, "Zadanie z kalendarza", "2026-11-03");
        // formularz podaje inny tytuł: połączona karta go nie przyjmuje
        let card = save(&conn, &linked(None, g, Some(item), "ignorowane", "opis kanbana")).unwrap();
        let t = list(&conn, "2026-11", "2026-11").unwrap().into_iter().find(|t| t.id == card).unwrap();
        assert_eq!((t.title.as_str(), t.item_date.as_deref(), t.item_id), ("Zadanie z kalendarza", Some("2026-11-03"), Some(item)));
        assert_eq!(t.description, "opis kanbana");

        // zmiana tytułu i dnia w kalendarzu przechodzi na kartę, także na inny miesiąc
        conn.execute("UPDATE items SET title = 'Nowy tytuł', date = '2026-12-05' WHERE id = ?1", [item]).unwrap();
        assert!(list(&conn, "2026-11", "2026-11").unwrap().is_empty());
        let moved = list(&conn, "2026-12", "2026-11").unwrap();
        assert_eq!((moved[0].title.as_str(), moved[0].month.as_str()), ("Nowy tytuł", "2026-12"));

        // edycja karty: tytuł zostaje z kalendarza, opis zmienia się tylko w kanbanie
        save(&conn, &linked(Some(card), g, Some(item), "próba zmiany", "nowy opis kanbana")).unwrap();
        let t = list(&conn, "2026-12", "2026-11").unwrap().into_iter().find(|t| t.id == card).unwrap();
        assert_eq!((t.title.as_str(), t.description.as_str()), ("Nowy tytuł", "nowy opis kanbana"));
        let cal_desc: String = conn.query_row("SELECT description FROM items WHERE id = ?1", [item], |r| r.get(0)).unwrap();
        assert_eq!(cal_desc, "opis z kalendarza");

        // zmiana opisu w kalendarzu nie rusza opisu karty
        conn.execute("UPDATE items SET description = 'opis z kalendarza 2' WHERE id = ?1", [item]).unwrap();
        sync_linked(&conn).unwrap();
        let t = list(&conn, "2026-12", "2026-11").unwrap().into_iter().find(|t| t.id == card).unwrap();
        assert_eq!(t.description, "nowy opis kanbana");
    }

    #[test]
    fn deleting_calendar_task_unlinks_card_and_keeps_title() {
        let conn = open_memory();
        let g = general(&conn);
        let item = calendar_task_row(&conn, "Do usunięcia", "2026-11-03");
        let card = save(&conn, &linked(None, g, Some(item), "", "")).unwrap();
        conn.execute("DELETE FROM items WHERE id = ?1", [item]).unwrap();
        let t = list(&conn, "2026-11", "2026-11").unwrap().into_iter().find(|t| t.id == card).unwrap();
        assert_eq!((t.title.as_str(), t.item_id, t.item_date), ("Do usunięcia", None, None));
    }

    #[test]
    fn events_cannot_be_linked() {
        let conn = open_memory();
        let g = general(&conn);
        let event: i64 = {
            conn.execute(
                "INSERT INTO items (kind, title, description, date, recurrence, created_at, source)
                 VALUES ('event', 'Spotkanie', '', '2026-11-03', 'none', 0, 'manual')",
                [],
            )
            .unwrap();
            conn.last_insert_rowid()
        };
        assert!(save(&conn, &linked(None, g, Some(event), "", "")).is_err());
    }

    fn done_count(conn: &Connection, item: i64, day: &str) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM item_done WHERE item_id = ?1 AND date = ?2", params![item, day], |r| r.get(0)).unwrap()
    }

    #[test]
    fn cards_can_be_reordered_and_moved_between_columns() {
        let conn = open_memory();
        let g = general(&conn);
        let a = save(&conn, &input(None, "2026-11", "todo", "A", g)).unwrap();
        let b = save(&conn, &input(None, "2026-11", "todo", "B", g)).unwrap();
        let c = save(&conn, &input(None, "2026-11", "todo", "C", g)).unwrap();
        let titles = |conn: &Connection| -> Vec<String> {
            list(conn, "2026-11", "2026-11").unwrap().into_iter().filter(|t| t.status == "todo").map(|t| t.title).collect()
        };
        assert_eq!(titles(&conn), ["A", "B", "C"]);

        // C przed A
        move_card(&conn, c, "todo", Some(a)).unwrap();
        assert_eq!(titles(&conn), ["C", "A", "B"]);
        // A na koniec
        move_card(&conn, a, "todo", None).unwrap();
        assert_eq!(titles(&conn), ["C", "B", "A"]);
        // B do kolumny „W trakcie” przed inną kartą, a kolumna „Do zrobienia” zostaje w kolejności
        let doing = save(&conn, &input(None, "2026-11", "doing", "D", g)).unwrap();
        move_card(&conn, b, "doing", Some(doing)).unwrap();
        assert_eq!(titles(&conn), ["C", "A"]);
        let doing_titles: Vec<String> = list(&conn, "2026-11", "2026-11").unwrap().into_iter().filter(|t| t.status == "doing").map(|t| t.title).collect();
        assert_eq!(doing_titles, ["B", "D"]);
        // przeniesienie karty przed samą siebie nic nie zmienia
        move_card(&conn, c, "todo", Some(c)).unwrap();
        assert_eq!(titles(&conn), ["C", "A"]);
    }

    #[test]
    fn calendar_done_moves_linked_card_both_ways() {
        let conn = open_memory();
        let g = general(&conn);
        let item = calendar_task_row(&conn, "Zrobić raport", "2026-11-03");
        let card = save(&conn, &linked(None, g, Some(item), "", "")).unwrap();

        // zaznaczenie w kalendarzu: karta w „Zrobione”
        crate::db::set_done(&conn, item, "2026-11-03", true).unwrap();
        on_calendar_done(&conn, item, "2026-11-03", true).unwrap();
        let t = list(&conn, "2026-11", "2026-11").unwrap().into_iter().find(|t| t.id == card).unwrap();
        assert_eq!(t.status, "done");

        // odznaczenie w kalendarzu: karta z powrotem w „Do zrobienia”
        crate::db::set_done(&conn, item, "2026-11-03", false).unwrap();
        on_calendar_done(&conn, item, "2026-11-03", false).unwrap();
        let t = list(&conn, "2026-11", "2026-11").unwrap().into_iter().find(|t| t.id == card).unwrap();
        assert_eq!(t.status, "todo");
    }

    #[test]
    fn kanban_done_marks_calendar_task_both_ways() {
        let conn = open_memory();
        let g = general(&conn);
        let item = calendar_task_row(&conn, "Zrobić raport", "2026-11-03");
        let card = save(&conn, &linked(None, g, Some(item), "", "")).unwrap();

        // przeniesienie karty do „Zrobione” zaznacza zadanie w kalendarzu
        save(&conn, &KanbanInput { id: Some(card), month: "2026-11".into(), status: "done".into(), title: "".into(), description: "".into(), board_id: g, item_id: Some(item) }).unwrap();
        assert_eq!(done_count(&conn, item, "2026-11-03"), 1);

        // z powrotem do „W trakcie” odznacza
        save(&conn, &KanbanInput { id: Some(card), month: "2026-11".into(), status: "doing".into(), title: "".into(), description: "".into(), board_id: g, item_id: Some(item) }).unwrap();
        assert_eq!(done_count(&conn, item, "2026-11-03"), 0);
    }
}
