pub mod budget;
pub mod categorize;
pub mod income;
pub mod invest;
pub mod cash;
pub mod finance;
pub mod receipts;
pub mod recurring;

use crate::error::{Error, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// Wspólny uchwyt do bazy. Blokadę trzymamy krótko i nigdy przez `.await`.
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
    conn.execute_batch(finance::SCHEMA)?;
    conn.execute_batch(receipts::SCHEMA)?;
    Ok(conn)
}
