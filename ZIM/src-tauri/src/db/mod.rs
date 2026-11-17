pub mod accounts;
pub mod attachments;
pub mod classification;
pub mod contacts;
pub mod messages;
pub mod outbox;
pub mod settings;
pub mod tags;

use crate::error::{Error, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// Wspólny uchwyt do bazy. Blokadę trzymamy krótko i nigdy przez `.await`, więc synchronizacja IMAP
/// nie blokuje interfejsu na czas sieci.
#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    pub fn new(conn: Connection) -> Self {
        Db(Arc::new(Mutex::new(conn)))
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0.lock().map_err(|_| Error::from("Baza danych jest niedostępna"))
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        f(&*self.lock()?)
    }
}

const SCHEMA: &str = include_str!("schema.sql");

pub fn open(path: &Path) -> Result<Connection> {
    init(Connection::open(path)?)
}

#[cfg(test)]
pub fn open_memory() -> Connection {
    init(Connection::open_in_memory().unwrap()).unwrap()
}

fn init(conn: Connection) -> Result<Connection> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(SCHEMA)?;
    tags::ensure_system_tags(&conn)?;
    repair_attachment_names(&conn)?;
    Ok(conn)
}

/// Ciąg `?, ?, ?` do klauzuli IN.
pub fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// Identyfikatory z interfejsu traktujemy jak niezaufane: tylko dodatnie, bez powtórzeń.
pub fn sanitize_ids(ids: &[i64]) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for &id in ids {
        if id > 0 && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Wcześniejsze wersje zapisywały nazwy plików tak, jak stały w nagłówku (`=?UTF-8?Q?...?=`).
fn repair_attachment_names(conn: &Connection) -> Result<()> {
    let encoded: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, filename FROM attachments WHERE filename LIKE '%=?%?=%'")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for (id, name) in encoded {
        let decoded = crate::mail::attachments::decode_filename(&name);
        if decoded != name {
            conn.execute("UPDATE attachments SET filename = ? WHERE id = ?", rusqlite::params![decoded, id])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_encoded_attachment_names_are_repaired_on_open() {
        let conn = open_memory();
        conn.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('a@x.pl', 'imap.x.pl', 'a')", []).unwrap();
        conn.execute("INSERT INTO messages (id, account_id, uid, from_addr, to_addr, subject, date) VALUES (1, 1, 1, 'a', 'b', 's', 1)", []).unwrap();
        for (part, name) in [("2", "=?UTF-8?Q?CV_Jan_Kowalski_(PL).pdf?="), ("3", "zwykły.pdf")] {
            conn.execute("INSERT INTO attachments (message_id, part, filename, mime, size) VALUES (1, ?, ?, 'application/pdf', 1)", [part, name]).unwrap();
        }
        repair_attachment_names(&conn).unwrap();
        let names: Vec<String> = conn.prepare("SELECT filename FROM attachments ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(names, ["CV Jan Kowalski (PL).pdf", "zwykły.pdf"]);
    }
}
