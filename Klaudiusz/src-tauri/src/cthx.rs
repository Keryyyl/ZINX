//! CTHX (tylko odczyt): plan dnia, nawyki i kanban.

use crate::dates::{minutes, occurs_on};
use crate::paths;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize)]
pub struct PlanItem {
    pub id: i64,
    /// „zadanie” albo „wydarzenie”
    pub rodzaj: String,
    pub tytul: String,
    /// np. „10:00–12:00”, albo brak (bez godziny / całodniowe)
    pub godzina: Option<String>,
    pub zrobione: bool,
}

#[derive(Debug, Serialize)]
pub struct HabitLine {
    pub nazwa: String,
    pub cel_miesieczny: i64,
    pub w_tym_miesiacu: i64,
    pub zrobione_dzis: bool,
}

#[derive(Debug, Serialize)]
pub struct KanbanLine {
    pub status: String,
    pub tytul: String,
    pub opis: String,
}

fn err(e: rusqlite::Error) -> String {
    format!("CTHX: {e}")
}

pub fn plan(conn: &Connection, day: &str) -> Result<Vec<PlanItem>, String> {
    let done: HashSet<i64> = {
        let mut stmt = conn.prepare("SELECT item_id FROM item_done WHERE date = ?1").map_err(err)?;
        let rows = stmt.query_map([day], |r| r.get::<_, i64>(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    let mut stmt = conn
        .prepare("SELECT id, kind, title, date, start_time, end_time, recurrence FROM items")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(err)?;

    let mut found: Vec<(Option<String>, u32, i64, PlanItem)> = Vec::new();
    for row in rows {
        let (id, kind, title, date, start, end, recurrence) = row.map_err(err)?;
        if !occurs_on(&date, &recurrence, day) {
            continue;
        }
        let godzina = match (&start, &end) {
            (Some(s), Some(e)) => Some(format!("{s}–{e}")),
            (Some(s), None) => Some(s.clone()),
            _ => None,
        };
        let sort_key = start.as_deref().map_or(u32::MAX, minutes);
        found.push((start.clone(), sort_key, id, PlanItem {
            id,
            rodzaj: if kind == "event" { "wydarzenie".into() } else { "zadanie".into() },
            tytul: title,
            godzina,
            zrobione: done.contains(&id),
        }));
    }
    found.sort_by_key(|(_, key, id, _)| (*key, *id));
    Ok(found.into_iter().map(|(_, _, _, item)| item).collect())
}

pub fn habits(conn: &Connection, today: &str, month: &str) -> Result<Vec<HabitLine>, String> {
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
                nazwa: r.get(1)?,
                cel_miesieczny: r.get(2)?,
                w_tym_miesiacu: counts.get(&id).copied().unwrap_or(0),
                zrobione_dzis: today_done.contains(&id),
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(list)
}

pub fn kanban(conn: &Connection, month: &str) -> Result<Vec<KanbanLine>, String> {
    let mut stmt = conn
        .prepare("SELECT status, title, description FROM kanban_tasks WHERE month = ?1 ORDER BY CASE status WHEN 'todo' THEN 0 WHEN 'doing' THEN 1 ELSE 2 END, position, id")
        .map_err(err)?;
    let rows = stmt
        .query_map([month], |r| Ok(KanbanLine { status: r.get(0)?, tytul: r.get(1)?, opis: r.get(2)? }))
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Połączenie z CTHX tylko do odczytu
pub fn connect() -> Result<Connection, String> {
    paths::open_read(&paths::cthx(), "CTHX")
}

#[derive(Debug, Serialize)]
pub struct Birthday {
    pub imie: String,
    /// dzień i miesiąc, RRRR-MM-DD z kontaktów (rok urodzenia)
    pub data: String,
}

/// Urodziny z kontaktów (wpisy `source = contacts`) przypadające w danym miesiącu
pub fn birthdays(conn: &Connection, month: &str) -> Result<Vec<Birthday>, String> {
    let mut stmt = conn
        .prepare("SELECT title, date FROM items WHERE source = 'contacts' AND substr(date, 6, 2) = ?1 ORDER BY substr(date, 9, 2), title")
        .map_err(err)?;
    let mm = &month[5..7];
    let rows = stmt
        .query_map([mm], |r| {
            let title: String = r.get(0)?;
            Ok(Birthday { imie: title.trim_start_matches("Urodziny:").trim().to_string(), data: r.get(1)? })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Wolne przedziały co najmniej `minutes` długości w oknie 08:00–22:00 danego dnia.
/// Zajęte są wpisy z godziną; wpis bez końca traktujemy jak 30-minutowy (tak jak oś czasu w EINPX).
pub fn free_slots(plan: &[PlanItem], minutes_needed: u32) -> Vec<(String, String)> {
    const DAY_START: u32 = 8 * 60;
    const DAY_END: u32 = 22 * 60;
    let mut busy: Vec<(u32, u32)> = plan
        .iter()
        .filter_map(|i| {
            let g = i.godzina.as_ref()?;
            let (start, end) = match g.split_once('–') {
                Some((s, e)) => (minutes(s), minutes(e)),
                None => (minutes(g), minutes(g) + 30),
            };
            Some((start, end.max(start + 30)))
        })
        .collect();
    busy.sort();

    let mut slots = Vec::new();
    let mut cursor = DAY_START;
    for (start, end) in busy.into_iter().chain(std::iter::once((DAY_END, DAY_END))) {
        if start > cursor && start.min(DAY_END) - cursor >= minutes_needed {
            slots.push((fmt_hm(cursor), fmt_hm(start.min(DAY_END))));
        }
        cursor = cursor.max(end);
        if cursor >= DAY_END {
            break;
        }
    }
    slots
}

fn fmt_hm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(godzina: Option<&str>) -> PlanItem {
        PlanItem { id: 0, rodzaj: "wydarzenie".into(), tytul: "x".into(), godzina: godzina.map(str::to_string), zrobione: false }
    }

    #[test]
    fn finds_gaps_between_timed_items() {
        let plan = vec![item(Some("09:00–10:00")), item(Some("12:00–13:30")), item(Some("15:00"))];
        assert_eq!(
            free_slots(&plan, 60),
            vec![("08:00".into(), "09:00".into()), ("10:00".into(), "12:00".into()), ("13:30".into(), "15:00".into()), ("15:30".into(), "22:00".into())]
        );
        // przerwa 08:00–08:50 (50 min) nie mieści się w 60 minutach, a w 30 minutach tak
        assert!(free_slots(&[item(Some("08:50–09:00"))], 60).iter().all(|(a, _)| a != "08:00"));
        assert!(free_slots(&[item(Some("08:50–09:00"))], 30).iter().any(|(a, _)| a == "08:00"));
    }

    #[test]
    fn empty_day_is_one_window() {
        assert_eq!(free_slots(&[], 60), vec![("08:00".into(), "22:00".into())]);
    }
}
