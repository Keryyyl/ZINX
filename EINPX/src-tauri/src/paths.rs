//! Lokalizacje baz pozostałych aplikacji ZINX (ta sama ścieżka, z której korzystają ich własne rdzenie).
//! EINPX czyta ZIM i PFMX tylko do odczytu, a do CTHX zapisuje wyłącznie wpisy i wykonanie.

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn app_db(app_id: &str, file: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support").join(app_id).join(file)
}

/// Własna baza EINPX (notatki)
pub fn einpx() -> PathBuf {
    app_db("app.einpx.dashboard", "einpx.sqlite")
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

/// Otwiera bazę tylko do odczytu. Gdy aplikacja akurat pisze, czekamy krótko zamiast od razu błędu.
pub fn open_read(path: &Path, name: &str) -> Result<Connection, String> {
    if !path.exists() {
        return Err(format!("Nie znaleziono bazy {name}. Uruchom aplikację przynajmniej raz."));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("{name}: {e}"))?;
    conn.busy_timeout(Duration::from_secs(3)).map_err(|e| format!("{name}: {e}"))?;
    Ok(conn)
}

/// Otwiera CTHX do zapisu (wpisy i wykonanie). Schemat zakłada CTHX, więc tu nic nie migrujemy.
pub fn open_write(path: &Path) -> Result<Connection, String> {
    if !path.exists() {
        return Err("Nie znaleziono bazy CTHX. Uruchom CTHX przynajmniej raz.".into());
    }
    let conn = Connection::open(path).map_err(|e| format!("CTHX: {e}"))?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e| format!("CTHX: {e}"))?;
    conn.pragma_update(None, "foreign_keys", "ON").map_err(|e| format!("CTHX: {e}"))?;
    Ok(conn)
}
