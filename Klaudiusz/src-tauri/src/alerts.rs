//! Przypomnienia (Set_Alert). Zapisywane we własnej bazie Klaudiusza, a sprawdzane co kilkanaście sekund
//! w tle. Gdy przypomnienie wypadnie, pokazuje powiadomienie macOS. Gdy aplikacja była wyłączona,
//! zaległe przypomnienia wypadają przy następnym starcie.

use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::time::Duration;

const CHECK_EVERY: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Alert {
    pub id: i64,
    pub text: String,
    pub fire_at_ms: i64,
}

fn err(e: rusqlite::Error) -> String {
    format!("Przypomnienia: {e}")
}

pub fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/app.klaudiusz.assistant/klaudiusz.sqlite")
}

pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS alerts (
           id INTEGER PRIMARY KEY,
           text TEXT NOT NULL,
           fire_at INTEGER NOT NULL,
           item_id INTEGER,
           fired INTEGER NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS alerts_due ON alerts(fired, fire_at);",
    )
    .map_err(err)
}

pub fn open() -> Result<Connection, String> {
    let path = db_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Przypomnienia: {e}"))?;
    }
    let conn = Connection::open(&path).map_err(err)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
    init(&conn)?;
    Ok(conn)
}

pub fn add(conn: &Connection, text: &str, fire_at_ms: i64, item_id: Option<i64>) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO alerts (text, fire_at, item_id, fired, created_at) VALUES (?1, ?2, ?3, 0, ?4)",
        params![text, fire_at_ms, item_id, crate::dates::now_ms()],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn pending(conn: &Connection) -> Result<Vec<Alert>, String> {
    let mut stmt = conn.prepare("SELECT id, text, fire_at FROM alerts WHERE fired = 0 ORDER BY fire_at, id").map_err(err)?;
    let rows = stmt.query_map([], |r| Ok(Alert { id: r.get(0)?, text: r.get(1)?, fire_at_ms: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn due(conn: &Connection, now_ms: i64) -> Result<Vec<Alert>, String> {
    let mut stmt = conn
        .prepare("SELECT id, text, fire_at FROM alerts WHERE fired = 0 AND fire_at <= ?1 ORDER BY fire_at, id")
        .map_err(err)?;
    let rows = stmt.query_map([now_ms], |r| Ok(Alert { id: r.get(0)?, text: r.get(1)?, fire_at_ms: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn mark_fired(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("UPDATE alerts SET fired = 1 WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

/// Powiadomienie macOS (pod nazwą Klaudiusza, patrz `notice.rs`)
pub fn notify(text: &str) -> Result<(), String> {
    crate::notice::send(text)
}

/// Jeden przebieg: wysyła powiadomienia dla przypomnień, które już wypadły
fn tick() -> Result<(), String> {
    let conn = open()?;
    for alert in due(&conn, crate::dates::now_ms())? {
        match notify(&alert.text) {
            Ok(()) => {
                mark_fired(&conn, alert.id)?;
                // przypomnienie wypowiadamy też na głos (przy wyciszeniu tylko powiadomienie)
                let _ = crate::tts::speak(&format!("Przypomnienie: {}", alert.text));
            }
            // nie oznaczamy jako wysłane, spróbujemy ponownie za chwilę
            Err(e) => eprintln!("{e}"),
        }
    }
    Ok(())
}

/// Uruchamia pętlę sprawdzającą w tle (wołane raz przy starcie aplikacji)
pub fn start_loop() {
    std::thread::spawn(|| loop {
        if let Err(e) = tick() {
            eprintln!("{e}");
        }
        std::thread::sleep(CHECK_EVERY);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_only_returns_unfired_alerts_whose_time_came() {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
        let a = add(&conn, "wcześniej", 1_000, None).unwrap();
        add(&conn, "później", 5_000, None).unwrap();
        let due_now = due(&conn, 2_000).unwrap();
        assert_eq!(due_now.len(), 1);
        assert_eq!(due_now[0].text, "wcześniej");
        mark_fired(&conn, a).unwrap();
        assert!(due(&conn, 2_000).unwrap().is_empty());
        assert_eq!(pending(&conn).unwrap().len(), 1);
    }

    #[test]
    fn escapes_quotes_for_applescript() {
        let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        assert_eq!(escape(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }
}

/// Usuwa przypomnienie (do cofnięcia)
pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM alerts WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}
