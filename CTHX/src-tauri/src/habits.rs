//! Nawyki: nazwa, cel na miesiąc (ile razy) i dni, w które go zrobiono.
//! Dzień przed datą startu nie jest liczony jako porażka.

use crate::db::valid_date;
use crate::error::{Error, Result};
use crate::now_ms;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

const MAX_NAME: usize = 80;
const MAX_DESCRIPTION: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    pub id: i64,
    pub name: String,
    pub description: String,
    /// ile razy w miesiącu chcę to zrobić
    pub goal: i64,
    /// pierwszy dzień śledzenia, RRRR-MM-DD
    pub start_date: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HabitInput {
    /// brak = nowy nawyk
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub goal: i64,
    pub start_date: String,
}

/// Jeden odhaczony dzień nawyku
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HabitDay {
    pub habit_id: i64,
    pub date: String,
}

pub fn list(conn: &Connection) -> Result<Vec<Habit>> {
    let mut stmt = conn.prepare("SELECT id, name, description, goal, start_date FROM habits ORDER BY created_at, id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Habit { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, goal: r.get(3)?, start_date: r.get(4)? })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn days(conn: &Connection) -> Result<Vec<HabitDay>> {
    let mut stmt = conn.prepare("SELECT habit_id, date FROM habit_done ORDER BY date")?;
    let rows = stmt.query_map([], |r| Ok(HabitDay { habit_id: r.get(0)?, date: r.get(1)? }))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn check(input: &HabitInput) -> Result<HabitInput> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(Error::from("Podaj nazwę nawyku"));
    }
    if name.chars().count() > MAX_NAME {
        return Err(Error::from("Nazwa: najwyżej 80 znaków"));
    }
    let description = input.description.trim().to_string();
    if description.chars().count() > MAX_DESCRIPTION {
        return Err(Error::from("Opis jest za długi"));
    }
    if !(1..=31).contains(&input.goal) {
        return Err(Error::from("Cel: od 1 do 31 razy w miesiącu"));
    }
    if !valid_date(&input.start_date) {
        return Err(Error::from("Nieprawidłowa data startu"));
    }
    Ok(HabitInput { id: input.id, name, description, goal: input.goal, start_date: input.start_date.clone() })
}

pub fn save(conn: &Connection, input: &HabitInput) -> Result<i64> {
    let clean = check(input)?;
    match clean.id {
        Some(id) => {
            let changed = conn.execute(
                "UPDATE habits SET name = ?1, description = ?2, goal = ?3, start_date = ?4 WHERE id = ?5",
                params![clean.name, clean.description, clean.goal, clean.start_date, id],
            )?;
            if changed == 0 {
                return Err(Error::from("Nie znaleziono nawyku"));
            }
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO habits (name, description, goal, start_date, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![clean.name, clean.description, clean.goal, clean.start_date, now_ms()],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM habits WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn set_day(conn: &Connection, habit_id: i64, date: &str, done: bool) -> Result<()> {
    if !valid_date(date) {
        return Err(Error::from("Nieprawidłowa data"));
    }
    if done {
        conn.execute("INSERT OR IGNORE INTO habit_done (habit_id, date) VALUES (?1, ?2)", params![habit_id, date])?;
    } else {
        conn.execute("DELETE FROM habit_done WHERE habit_id = ?1 AND date = ?2", params![habit_id, date])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn input(name: &str, goal: i64) -> HabitInput {
        HabitInput { id: None, name: name.into(), description: "".into(), goal, start_date: "2026-11-01".into() }
    }

    #[test]
    fn habit_crud_and_days() {
        let conn = db::open_memory();
        let id = save(&conn, &input("  Bieganie ", 5)).unwrap();
        let mut edit = input("Bieg", 8);
        edit.id = Some(id);
        save(&conn, &edit).unwrap();
        let habits = list(&conn).unwrap();
        assert_eq!(habits.len(), 1);
        assert_eq!((habits[0].name.as_str(), habits[0].goal), ("Bieg", 8));

        set_day(&conn, id, "2026-11-02", true).unwrap();
        set_day(&conn, id, "2026-11-02", true).unwrap(); // dwukrotne odhaczenie nie dubluje
        assert_eq!(days(&conn).unwrap(), vec![HabitDay { habit_id: id, date: "2026-11-02".into() }]);

        delete(&conn, id).unwrap();
        assert!(list(&conn).unwrap().is_empty());
        assert!(days(&conn).unwrap().is_empty());
    }

    #[test]
    fn rejects_bad_goal_and_date() {
        let conn = db::open_memory();
        assert!(save(&conn, &input("x", 0)).is_err());
        assert!(save(&conn, &input("x", 32)).is_err());
        assert!(save(&conn, &input("", 3)).is_err());
        let mut bad = input("x", 3);
        bad.start_date = "01.11.2026".into();
        assert!(save(&conn, &bad).is_err());
    }
}
