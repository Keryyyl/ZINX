//! PFMX (tylko odczyt): konto Main, oszczędności i poduszka, gotówka oraz data ostatniego importu.

use crate::paths;
use rusqlite::{Connection, OptionalExtension};

/// Saldo po ostatniej transakcji konta (grosze)
const LATEST_BALANCE: &str = "(SELECT t.balance_after FROM fin_transactions t WHERE t.account_id = a.id ORDER BY t.booked_on DESC, t.id DESC LIMIT 1)";

fn err(e: rusqlite::Error) -> String {
    format!("PFMX: {e}")
}

pub fn connect() -> Result<Connection, String> {
    paths::open_read(&paths::pfmx(), "PFMX")
}

pub fn main_balance(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        &format!("SELECT COALESCE({LATEST_BALANCE}, 0) FROM fin_accounts a WHERE a.name = 'Main' COLLATE NOCASE ORDER BY a.id LIMIT 1"),
        [],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "PFMX: nie znaleziono konta „Main”".to_string())
}

/// Konta oszczędnościowe i poduszka finansowa (PLN)
pub fn savings_and_buffer(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        &format!("SELECT COALESCE(SUM({LATEST_BALANCE}), 0) FROM fin_accounts a WHERE a.kind IN ('savings', 'buffer') AND a.currency = 'PLN'"),
        [],
        |r| r.get(0),
    )
    .map_err(err)
}

pub fn cash(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT COALESCE(SUM(amount), 0) FROM fin_cash", [], |r| r.get(0)).map_err(err)
}

/// Nazwa pliku i czas (ms od epoki) ostatniego importu z Exportera
pub fn last_import(conn: &Connection) -> Result<Option<(String, i64)>, String> {
    conn.query_row("SELECT file_name, imported_at FROM fin_imports ORDER BY imported_at DESC, id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(err)
}

#[derive(Debug, serde::Serialize)]
pub struct Payment {
    pub nazwa: String,
    /// „subskrypcja” albo „inne” (np. raty)
    pub rodzaj: String,
    /// kwota w groszach
    pub kwota_grosze: i64,
    /// ile rat zostało (gdy dotyczy)
    pub raty_zostalo: Option<i64>,
}

/// Płatności cykliczne (PFMX „Stałe płatności”), które nie są jeszcze oznaczone jako zapłacone w danym miesiącu
pub fn upcoming_payments(conn: &Connection, month: &str) -> Result<Vec<Payment>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.name, r.kind, r.amount, r.installments_left
               FROM fin_recurring r
              WHERE r.hidden = 0
                AND NOT EXISTS (SELECT 1 FROM fin_recurring_paid p WHERE p.recurring_id = r.id AND p.month = ?1)
                AND (r.installments_left IS NULL OR r.installments_left > 0)
              ORDER BY r.name COLLATE NOCASE",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([month], |r| {
            let kind: String = r.get(1)?;
            Ok(Payment {
                nazwa: r.get(0)?,
                rodzaj: if kind == "subscription" { "subskrypcja".into() } else { "inne".into() },
                kwota_grosze: r.get(2)?,
                raty_zostalo: r.get(3)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Połączenie z PFMX do zapisu. Zapisujemy wyłącznie ręczne wpisy gotówki (`fin_cash`, source = manual),
/// tak jak robi to PFMX, a wpisów z bankomatu nie ruszamy.
pub fn connect_write() -> Result<Connection, String> {
    let path = paths::pfmx();
    if !path.exists() {
        return Err("Nie znaleziono bazy PFMX. Uruchom PFMX przynajmniej raz.".into());
    }
    let conn = Connection::open(&path).map_err(err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(err)?;
    Ok(conn)
}

pub const MAX_CASH_CHANGE: i64 = 100_000 * 100;

/// Dodaje ręczny wpis gotówki (ze znakiem: + wpłata, − wydatek). Zwraca id wpisu.
pub fn add_cash(conn: &Connection, amount_grosze: i64, note: &str, now_ms: i64) -> Result<i64, String> {
    if amount_grosze == 0 {
        return Err("Kwota nie może być zerem".into());
    }
    if amount_grosze.abs() > MAX_CASH_CHANGE {
        return Err("Kwota jest za duża (najwyżej 100 000 zł)".into());
    }
    conn.execute(
        "INSERT INTO fin_cash (amount, note, source, transaction_id, created_at) VALUES (?1, ?2, 'manual', NULL, ?3)",
        rusqlite::params![amount_grosze, note.trim(), now_ms],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

/// Usuwa ręczny wpis gotówki (do cofnięcia zmiany). Wpisy z bankomatu nie są ruszane.
pub fn delete_manual_cash(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM fin_cash WHERE id = ?1 AND source = 'manual'", [id]).map_err(err)?;
    Ok(())
}
