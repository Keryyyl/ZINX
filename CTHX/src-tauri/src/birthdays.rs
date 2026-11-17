//! Urodziny z kontaktów jako coroczne wydarzenia całodniowe. Każdy kontakt ma jeden wpis
//! (źródło „contacts”, identyfikator kontaktu), więc ponowna synchronizacja niczego nie dubluje.

use crate::now_ms;
use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashSet;

pub const SOURCE: &str = "contacts";

/// Urodziny jednego kontaktu. Rok jest tylko wtedy, gdy kontakt go zna.
#[derive(Debug, Clone, PartialEq)]
pub struct Birthday {
    pub contact_id: String,
    pub name: String,
    pub month: u32,
    pub day: u32,
    pub year: Option<i32>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub added: i64,
    pub updated: i64,
    /// wpisy kontaktów, których już nie ma w książce adresowej
    pub removed: i64,
    /// ile urodzin przyszło z kontaktów
    pub total: i64,
}

/// Data pierwszego wystąpienia. Bez roku przyjmujemy 1900, żeby wpis obejmował wszystkie lata.
fn first_date(b: &Birthday) -> String {
    format!("{:04}-{:02}-{:02}", b.year.unwrap_or(1900), b.month, b.day)
}

pub fn sync(conn: &Connection, people: &[Birthday]) -> Result<SyncReport> {
    let tx = conn.unchecked_transaction()?;
    let mut report = SyncReport { total: people.len() as i64, ..Default::default() };
    let mut seen: HashSet<&str> = HashSet::new();

    for person in people {
        seen.insert(person.contact_id.as_str());
        let title = format!("Urodziny: {}", person.name);
        let date = first_date(person);
        let existing: Option<i64> = conn
            .query_row("SELECT id FROM items WHERE source = ?1 AND external_id = ?2", params![SOURCE, person.contact_id], |r| r.get(0))
            .optional()?;
        match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE items SET kind = 'event', title = ?1, date = ?2, recurrence = 'yearly', start_time = NULL, end_time = NULL WHERE id = ?3",
                    params![title, date, id],
                )?;
                report.updated += 1;
            }
            None => {
                conn.execute(
                    "INSERT INTO items (kind, title, description, date, start_time, end_time, recurrence, created_at, source, external_id)
                     VALUES ('event', ?1, '', ?2, NULL, NULL, 'yearly', ?3, ?4, ?5)",
                    params![title, date, now_ms(), SOURCE, person.contact_id],
                )?;
                report.added += 1;
            }
        }
    }

    // kontakt usunięty z książki: jego urodziny znikają z kalendarza
    let stale: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, external_id FROM items WHERE source = ?1 AND external_id IS NOT NULL")?;
        let rows = stmt.query_map(params![SOURCE], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for (id, contact_id) in stale {
        if !seen.contains(contact_id.as_str()) {
            conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
            report.removed += 1;
        }
    }

    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn person(id: &str, name: &str, month: u32, day: u32, year: Option<i32>) -> Birthday {
        Birthday { contact_id: id.into(), name: name.into(), month, day, year }
    }

    #[test]
    fn birthdays_become_yearly_all_day_events_and_are_not_duplicated() {
        let conn = db::open_memory();
        let list = vec![person("c1", "Anna Nowak", 3, 14, Some(1990)), person("c2", "Jan Kowal", 11, 2, None)];
        let first = sync(&conn, &list).unwrap();
        assert_eq!((first.added, first.updated, first.removed, first.total), (2, 0, 0, 2));

        let again = sync(&conn, &list).unwrap();
        assert_eq!((again.added, again.updated), (0, 2));

        let items = db::list(&conn).unwrap();
        assert_eq!(items.len(), 2);
        let anna = items.iter().find(|i| i.title == "Urodziny: Anna Nowak").unwrap();
        assert_eq!(anna.kind, "event");
        assert_eq!(anna.recurrence, "yearly");
        assert_eq!(anna.date, "1990-03-14");
        assert_eq!(anna.start_time, None);
        assert_eq!(anna.end_time, None);
    }

    #[test]
    fn removed_contacts_remove_their_birthdays_only() {
        let conn = db::open_memory();
        sync(&conn, &[person("c1", "Anna", 3, 14, None), person("c2", "Jan", 5, 1, None)]).unwrap();
        // ręcznie dodane wydarzenie zostaje, nawet gdy kontaktów nie ma
        db::save(&conn, &db::ItemInput { id: None, kind: "event".into(), title: "Ręczne".into(), description: "".into(), date: "2026-01-01".into(), start_time: None, end_time: None, recurrence: "none".into() }).unwrap();

        let report = sync(&conn, &[person("c1", "Anna", 3, 14, None)]).unwrap();
        assert_eq!(report.removed, 1);
        let titles: Vec<String> = db::list(&conn).unwrap().into_iter().map(|i| i.title).collect();
        assert!(titles.contains(&"Urodziny: Anna".to_string()));
        assert!(titles.contains(&"Ręczne".to_string()));
        assert!(!titles.contains(&"Urodziny: Jan".to_string()));
    }
}
