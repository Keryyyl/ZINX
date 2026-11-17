//! Notatki szybkie z EINPX (tylko odczyt): wyszukiwanie po fragmencie tekstu.

use crate::paths;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct NoteLine {
    pub tekst: String,
    /// czas zapisu, ms od epoki
    pub dodano_ms: i64,
}

/// Liczba wszystkich notatek w EINPX (do komunikatu, gdy nic nie znaleziono)
pub fn count() -> Result<i64, String> {
    let conn: Connection = paths::open_read(&paths::einpx(), "EINPX")?;
    conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0)).map_err(|e| format!("EINPX: {e}"))
}

/// Szuka notatek zawierających zapytanie. Bez względu na wielkość liter (także polskie znaki)
/// i z odmianą: „wizycie” znajdzie „wizyta”, bo porównujemy rdzeń słowa bez ostatnich znaków.
pub fn search(query: &str, limit: usize) -> Result<Vec<NoteLine>, String> {
    let conn: Connection = paths::open_read(&paths::einpx(), "EINPX")?;
    let err = |e: rusqlite::Error| format!("EINPX: {e}");
    let mut stmt = conn
        .prepare("SELECT text, created_at FROM notes ORDER BY created_at DESC, id DESC")
        .map_err(err)?;
    let all = stmt.query_map([], |r| Ok(NoteLine { tekst: r.get(0)?, dodano_ms: r.get(1)? })).map_err(err)?;
    let all: Vec<NoteLine> = all.collect::<Result<_, _>>().map_err(err)?;
    let q = query.trim().to_lowercase();
    let chars: Vec<char> = q.chars().collect();
    // rdzeń: dla dłuższych słów odcinamy 2 ostatnie litery (odmiana)
    let stem: String = if chars.len() > 5 { chars[..chars.len() - 2].iter().collect() } else { q.clone() };
    if stem.is_empty() {
        return Ok(Vec::new());
    }
    Ok(all
        .into_iter()
        .filter(|n| n.tekst.to_lowercase().contains(&stem))
        .take(limit)
        .collect())
}

/// Dopisuje szybką notatkę do EINPX (tabela `notes`, ta sama co w panelu EINPX)
pub fn add(text: &str, now_ms: i64) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Notatka jest pusta".into());
    }
    if text.chars().count() > 500 {
        return Err("Notatka jest za długa (najwyżej 500 znaków)".into());
    }
    let path = paths::einpx();
    if !path.exists() {
        return Err("Nie znaleziono bazy EINPX. Uruchom EINPX przynajmniej raz.".into());
    }
    let conn = rusqlite::Connection::open(&path).map_err(|e| format!("EINPX: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| format!("EINPX: {e}"))?;
    conn.execute("INSERT INTO notes (text, created_at) VALUES (?1, ?2)", rusqlite::params![text, now_ms])
        .map_err(|e| format!("EINPX: {e}"))?;
    Ok(conn.last_insert_rowid())
}

/// Usuwa notatkę z EINPX (do cofnięcia)
pub fn delete(id: i64) -> Result<(), String> {
    let path = paths::einpx();
    let conn = rusqlite::Connection::open(&path).map_err(|e| format!("EINPX: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| format!("EINPX: {e}"))?;
    conn.execute("DELETE FROM notes WHERE id = ?1", [id]).map_err(|e| format!("EINPX: {e}"))?;
    Ok(())
}
