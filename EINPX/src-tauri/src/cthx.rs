//! CTHX: plan dnia i nawyki (odczyt) oraz dodawanie wpisów i zaznaczanie wykonania (zapis).
//! Zapis idzie prosto do cthx.sqlite, a CTHX pokazuje zmiany po przycisku „Odśwież” albo po powrocie do okna.

use crate::dates::{minutes, now_ms, occurs_on, valid_date, valid_time};
use crate::paths;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MAX_TITLE: usize = 120;
const MAX_DESCRIPTION: usize = 2000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub id: i64,
    /// „task” albo „event”
    pub kind: String,
    pub title: String,
    pub description: String,
    /// GG:MM albo brak (zadanie bez godziny, wydarzenie całodniowe)
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub done: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HabitLine {
    pub id: i64,
    pub name: String,
    /// ile razy w miesiącu chcę to zrobić
    pub goal: i64,
    pub done_this_month: i64,
    pub done_today: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewItem {
    pub kind: String,
    pub title: String,
    pub description: String,
    pub date: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

pub fn plan(today: &str) -> Result<Vec<PlanItem>, String> {
    let conn = paths::open_read(&paths::cthx(), "CTHX")?;
    let err = |e: rusqlite::Error| format!("CTHX: {e}");

    let done: HashSet<i64> = {
        let mut stmt = conn.prepare("SELECT item_id FROM item_done WHERE date = ?1").map_err(err)?;
        let rows = stmt.query_map([today], |r| r.get::<_, i64>(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };

    let mut stmt = conn
        .prepare("SELECT id, kind, title, description, date, start_time, end_time, recurrence FROM items")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                PlanItem {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    title: r.get(2)?,
                    description: r.get(3)?,
                    start_time: r.get(5)?,
                    end_time: r.get(6)?,
                    done: false,
                },
                r.get::<_, String>(4)?,
                r.get::<_, String>(7)?,
            ))
        })
        .map_err(err)?;

    let mut items = Vec::new();
    for row in rows {
        let (mut item, date, recurrence) = row.map_err(err)?;
        if occurs_on(&date, &recurrence, today) {
            item.done = done.contains(&item.id);
            items.push(item);
        }
    }
    // najpierw wpisy z godziną według czasu startu, potem bez godziny
    items.sort_by_key(|i| (i.start_time.is_none(), i.start_time.as_deref().map_or(0, minutes), i.id));
    Ok(items)
}

pub fn habits(today: &str, month: &str) -> Result<Vec<HabitLine>, String> {
    let conn = paths::open_read(&paths::cthx(), "CTHX")?;
    let err = |e: rusqlite::Error| format!("CTHX: {e}");

    let mut counts: HashMap<i64, i64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT habit_id, COUNT(*) FROM habit_done WHERE substr(date, 1, 7) = ?1 GROUP BY habit_id")
            .map_err(err)?;
        let rows = stmt.query_map([month], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(err)?;
        for row in rows {
            let (id, n) = row.map_err(err)?;
            counts.insert(id, n);
        }
    }
    let today_done: HashSet<i64> = {
        let mut stmt = conn.prepare("SELECT habit_id FROM habit_done WHERE date = ?1").map_err(err)?;
        let rows = stmt.query_map([today], |r| r.get::<_, i64>(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };

    let mut stmt = conn.prepare("SELECT id, name, goal FROM habits ORDER BY created_at, id").map_err(err)?;
    let list = stmt
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            Ok(HabitLine {
                id,
                name: r.get(1)?,
                goal: r.get(2)?,
                done_this_month: counts.get(&id).copied().unwrap_or(0),
                done_today: today_done.contains(&id),
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(list)
}

/// Sprawdza wpis tak jak CTHX (`check` w db.rs) zanim trafi do bazy
fn check(input: &NewItem) -> Result<(), String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("Podaj tytuł".into());
    }
    if title.chars().count() > MAX_TITLE {
        return Err("Tytuł: najwyżej 120 znaków".into());
    }
    if input.description.trim().chars().count() > MAX_DESCRIPTION {
        return Err("Opis jest za długi".into());
    }
    if input.kind != "task" && input.kind != "event" {
        return Err("Nieznany rodzaj wpisu".into());
    }
    if !valid_date(&input.date) {
        return Err("Nieprawidłowa data".into());
    }
    let start = input.start_time.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let end = input.end_time.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if let Some(t) = start {
        if !valid_time(t) {
            return Err("Nieprawidłowa godzina".into());
        }
    }
    if let Some(t) = end {
        if !valid_time(t) {
            return Err("Nieprawidłowa godzina zakończenia".into());
        }
    }
    if input.kind == "event" {
        match (start, end) {
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

pub fn add_item(input: &NewItem) -> Result<(), String> {
    check(input)?;
    let conn = paths::open_write(&paths::cthx())?;
    let trimmed = |s: &Option<String>| s.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    conn.execute(
        "INSERT INTO items (kind, title, description, date, start_time, end_time, recurrence, created_at, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'none', ?7, 'manual')",
        params![
            input.kind,
            input.title.trim(),
            input.description.trim(),
            input.date,
            trimmed(&input.start_time),
            trimmed(&input.end_time),
            now_ms()
        ],
    )
    .map_err(|e| format!("CTHX: {e}"))?;
    Ok(())
}

pub fn set_item_done(item_id: i64, date: &str, done: bool) -> Result<(), String> {
    if !valid_date(date) {
        return Err("Nieprawidłowa data".into());
    }
    let conn = paths::open_write(&paths::cthx())?;
    write_done(&conn, "item_done", "item_id", item_id, date, done)
}

pub fn set_habit_day(habit_id: i64, date: &str, done: bool) -> Result<(), String> {
    if !valid_date(date) {
        return Err("Nieprawidłowa data".into());
    }
    let conn = paths::open_write(&paths::cthx())?;
    write_done(&conn, "habit_done", "habit_id", habit_id, date, done)
}

fn write_done(conn: &Connection, table: &str, column: &str, id: i64, date: &str, done: bool) -> Result<(), String> {
    // nazwy tabel i kolumn pochodzą wyłącznie z kodu powyżej, nie z danych użytkownika
    let sql = if done {
        format!("INSERT OR IGNORE INTO {table} ({column}, date) VALUES (?1, ?2)")
    } else {
        format!("DELETE FROM {table} WHERE {column} = ?1 AND date = ?2")
    };
    conn.execute(&sql, params![id, date]).map_err(|e| format!("CTHX: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zapis na kopii bazy: ustaw EINPX_TEST_HOME na folder z kopią
    /// („Library/Application Support/app.cthx.planner/cthx.sqlite”). Bez zmiennej test się pomija.
    #[test]
    fn writes_go_to_cthx_copy() {
        let Ok(home) = std::env::var("EINPX_TEST_HOME") else { return };
        std::env::set_var("HOME", home);
        let today = "2026-11-08";

        add_item(&NewItem {
            kind: "event".into(),
            title: "Wykład na studiach".into(),
            description: "test".into(),
            date: today.into(),
            start_time: Some("10:00".into()),
            end_time: Some("12:00".into()),
        })
        .expect("wydarzenie");
        add_item(&NewItem { kind: "task".into(), title: "Zadanie testowe".into(), description: String::new(), date: today.into(), start_time: None, end_time: None })
            .expect("zadanie");

        assert!(add_item(&NewItem { kind: "event".into(), title: "Zły".into(), description: String::new(), date: today.into(), start_time: Some("12:00".into()), end_time: Some("10:00".into()) }).is_err());

        let plan_before = plan(today).expect("plan");
        let wykład = plan_before.iter().find(|i| i.title == "Wykład na studiach").expect("wykład w planie");
        assert_eq!(wykład.start_time.as_deref(), Some("10:00"));
        assert!(!wykład.done);

        set_item_done(wykład.id, today, true).expect("odhacz");
        assert!(plan(today).unwrap().iter().any(|i| i.id == wykład.id && i.done));
        set_item_done(wykład.id, today, false).expect("odznacz");
        assert!(plan(today).unwrap().iter().any(|i| i.id == wykład.id && !i.done));

        let habit = habits(today, "2026-11").expect("nawyki").into_iter().next().expect("jakikolwiek nawyk");
        set_habit_day(habit.id, today, true).expect("nawyk zrobiony");
        let after = habits(today, "2026-11").unwrap().into_iter().find(|h| h.id == habit.id).unwrap();
        assert!(after.done_today);
        assert_eq!(after.done_this_month, habit.done_this_month + if habit.done_today { 0 } else { 1 });
        set_habit_day(habit.id, today, habit.done_today).expect("przywróć stan");
    }
}
