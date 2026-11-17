//! Lokalizacje baz ZINX-a. Klaudiusz czyta je tylko do odczytu (etap 2); zapisy dojdą w etapie 4.

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn app_db(app_id: &str, file: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support").join(app_id).join(file)
}

pub fn zim() -> PathBuf {
    app_db("app.zim.mail", "zim.sqlite")
}

pub fn pfmx() -> PathBuf {
    app_db("app.pfmx.finance", "pfmx.sqlite")
}

pub fn cthx() -> PathBuf {
    app_db("app.cthx.planner", "cthx.sqlite")
}

/// Otwiera bazę tylko do odczytu. Gdy inna aplikacja akurat pisze, czekamy krótko.
pub fn open_read(path: &Path, name: &str) -> Result<Connection, String> {
    if !path.exists() {
        return Err(format!("Nie znaleziono bazy {name}. Uruchom aplikację przynajmniej raz."));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("{name}: {e}"))?;
    conn.busy_timeout(Duration::from_secs(3)).map_err(|e| format!("{name}: {e}"))?;
    Ok(conn)
}

/// Notatki z EINPX (tylko odczyt)
pub fn einpx() -> PathBuf {
    app_db("app.einpx.dashboard", "einpx.sqlite")
}
