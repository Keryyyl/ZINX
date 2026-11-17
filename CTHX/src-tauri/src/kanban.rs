//! Kanban: zadania w trzech kolumnach, osobno dla każdego miesiąca.
//! Zadania „Do zrobienia” z wcześniejszych miesięcy przechodzą do bieżącego miesiąca przy jego otwarciu.

use crate::error::{Error, Result};
use crate::now_ms;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const MAX_TITLE: usize = 120;
const MAX_DESCRIPTION: usize = 2000;
const STATUSES: [&str; 3] = ["todo", "doing", "done"];

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
}

fn valid_month(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 7 && b[4] == b'-' && b[..4].iter().all(u8::is_ascii_digit) && b[5..].iter().all(u8::is_ascii_digit) && (1..=12).contains(&text[5..].parse::<u32>().unwrap_or(0))
}

/// Zadania miesiąca `month`. Gdy `month` jest bieżącym miesiącem, zaległe „Do zrobienia” przenoszą się tutaj.
pub fn list(conn: &Connection, month: &str, current_month: &str) -> Result<Vec<KanbanTask>> {
    if !valid_month(month) || !valid_month(current_month) {
        return Err(Error::from("Zły format miesiąca (oczekiwano RRRR-MM)"));
    }
    if month == current_month {
        conn.execute("UPDATE kanban_tasks SET month = ?1 WHERE status = 'todo' AND month < ?1", params![month])?;
    }
    let mut stmt = conn.prepare("SELECT id, month, status, title, description FROM kanban_tasks WHERE month = ?1 ORDER BY position, id")?;
    let rows = stmt.query_map([month], |r| {
        Ok(KanbanTask { id: r.get(0)?, month: r.get(1)?, status: r.get(2)?, title: r.get(3)?, description: r.get(4)? })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
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

/// Dodaje zadanie albo zmienia tytuł, opis i kolumnę (gdy jest id). Zmiana kolumny ustawia zadanie na końcu tej kolumny.
pub fn save(conn: &Connection, input: &KanbanInput) -> Result<i64> {
    check(input)?;
    let title = input.title.trim();
    let description = input.description.trim();
    match input.id {
        Some(id) => {
            let (old_status, month): (String, String) = conn
                .query_row("SELECT status, month FROM kanban_tasks WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?
                .ok_or_else(|| Error::from("Nie znaleziono zadania"))?;
            if old_status == input.status {
                conn.execute("UPDATE kanban_tasks SET title = ?2, description = ?3 WHERE id = ?1", params![id, title, description])?;
            } else {
                conn.execute(
                    "UPDATE kanban_tasks SET status = ?2, title = ?3, description = ?4,
                       position = (SELECT COALESCE(MAX(position), 0) + 1 FROM kanban_tasks WHERE month = ?5 AND status = ?2)
                     WHERE id = ?1",
                    params![id, input.status, title, description, month],
                )?;
            }
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO kanban_tasks (month, status, title, description, position, created_at)
                 VALUES (?1, ?2, ?3, ?4,
                   (SELECT COALESCE(MAX(position), 0) + 1 FROM kanban_tasks WHERE month = ?1 AND status = ?2), ?5)",
                params![input.month, input.status, title, description, now_ms()],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM kanban_tasks WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    fn input(id: Option<i64>, month: &str, status: &str, title: &str) -> KanbanInput {
        KanbanInput { id, month: month.into(), status: status.into(), title: title.into(), description: String::new() }
    }

    #[test]
    fn add_move_edit_delete() {
        let conn = open_memory();
        let a = save(&conn, &input(None, "2026-11", "todo", "A")).unwrap();
        let b = save(&conn, &input(None, "2026-11", "todo", "B")).unwrap();
        let tasks = list(&conn, "2026-11", "2026-11").unwrap();
        assert_eq!(tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);

        // przeniesienie do „W trakcie” zmienia kolumnę, a tytuł zostaje
        save(&conn, &input(Some(a), "2026-11", "doing", "A")).unwrap();
        let tasks = list(&conn, "2026-11", "2026-11").unwrap();
        assert_eq!(tasks.iter().find(|t| t.id == a).unwrap().status, "doing");

        // edycja tytułu bez zmiany kolumny
        save(&conn, &input(Some(b), "2026-11", "todo", "B2")).unwrap();
        assert_eq!(list(&conn, "2026-11", "2026-11").unwrap().iter().find(|t| t.id == b).unwrap().title, "B2");

        delete(&conn, a).unwrap();
        assert_eq!(list(&conn, "2026-11", "2026-11").unwrap().len(), 1);
    }

    #[test]
    fn unfinished_todo_rolls_into_current_month_only() {
        let conn = open_memory();
        let todo = save(&conn, &input(None, "2026-09", "todo", "zaległe")).unwrap();
        let doing = save(&conn, &input(None, "2026-09", "doing", "w trakcie")).unwrap();
        let done = save(&conn, &input(None, "2026-09", "done", "gotowe")).unwrap();

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
        assert!(save(&conn, &input(None, "2026-11", "todo", "   ")).is_err());
        assert!(save(&conn, &input(None, "2026-11", "archive", "x")).is_err());
        assert!(save(&conn, &input(None, "2026-13", "todo", "x")).is_err());
    }
}
