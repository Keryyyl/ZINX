//! Finanse z PFMX (tylko odczyt, bez inwestycji): saldo konta „Main”, konta oszczędnościowe i gotówka.

use crate::paths;
use rusqlite::OptionalExtension;
use serde::Serialize;

/// Saldo po ostatniej transakcji konta (grosze)
const LATEST_BALANCE: &str = "(SELECT t.balance_after FROM fin_transactions t WHERE t.account_id = a.id ORDER BY t.booked_on DESC, t.id DESC LIMIT 1)";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceSummary {
    /// saldo konta „Main”, grosze
    pub main: i64,
    /// suma kont oszczędnościowych, grosze
    pub savings_accounts: i64,
    /// gotówka (suma wpisów gotówki w PFMX), grosze
    pub cash: i64,
}

pub fn summary() -> Result<FinanceSummary, String> {
    let conn = paths::open_read(&paths::pfmx(), "PFMX")?;
    let err = |e: rusqlite::Error| format!("PFMX: {e}");

    let main: i64 = conn
        .query_row(
            &format!("SELECT COALESCE({LATEST_BALANCE}, 0) FROM fin_accounts a WHERE a.name = 'Main' COLLATE NOCASE ORDER BY a.id LIMIT 1"),
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?
        .ok_or_else(|| "PFMX: nie znaleziono konta „Main”".to_string())?;

    let savings_accounts: i64 = conn
        .query_row(
            &format!("SELECT COALESCE(SUM({LATEST_BALANCE}), 0) FROM fin_accounts a WHERE a.kind = 'savings' AND a.currency = 'PLN'"),
            [],
            |r| r.get(0),
        )
        .map_err(err)?;

    let cash: i64 = conn.query_row("SELECT COALESCE(SUM(amount), 0) FROM fin_cash", [], |r| r.get(0)).map_err(err)?;

    Ok(FinanceSummary { main, savings_accounts, cash })
}
