//! Gotówka w portfelu: ręczne wpisy ze znakiem i krótkim opisem, korekty do wpisanego stanu
//! oraz automatyczne wpisy z wypłat z bankomatu. Stan to suma wpisów.

use crate::error::{Error, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

const MAX_NOTE: usize = 80;
const MAX_AMOUNT: i64 = 100_000_000_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CashEntry {
    pub id: i64,
    /// + wpływ, − wydatek; w groszach
    pub amount: i64,
    pub note: String,
    /// „manual”, „atm” (wypłata z bankomatu) albo „correction” (ustawienie stanu)
    pub source: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CashState {
    pub balance: i64,
    pub entries: Vec<CashEntry>,
}

fn check_note(note: &str) -> Result<String> {
    let note = note.trim();
    if note.chars().count() > MAX_NOTE {
        return Err(Error::from("Opis: najwyżej 80 znaków"));
    }
    Ok(note.to_string())
}

fn check_amount(amount: i64) -> Result<()> {
    if amount == 0 || amount.abs() > MAX_AMOUNT {
        return Err(Error::from("Kwota musi być różna od zera i nie większa niż 1 000 000 000 zł"));
    }
    Ok(())
}

pub fn state(conn: &Connection) -> Result<CashState> {
    let balance: i64 = conn.query_row("SELECT COALESCE(SUM(amount), 0) FROM fin_cash", [], |r| r.get(0))?;
    let mut stmt = conn.prepare("SELECT id, amount, note, source, created_at FROM fin_cash ORDER BY created_at DESC, id DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok(CashEntry { id: r.get(0)?, amount: r.get(1)?, note: r.get(2)?, source: r.get(3)?, created_at: r.get(4)? })
    })?;
    Ok(CashState { balance, entries: rows.collect::<std::result::Result<_, _>>()? })
}

/// Ręczny wpis: + dodaje gotówkę, − odejmuje.
pub fn add(conn: &Connection, amount: i64, note: &str) -> Result<i64> {
    check_amount(amount)?;
    conn.execute(
        "INSERT INTO fin_cash (amount, note, source, created_at) VALUES (?1, ?2, 'manual', ?3)",
        params![amount, check_note(note)?, crate::now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Ustawia stan na wpisaną kwotę. Różnica trafia jako wpis korekty, więc historia się zgadza.
/// Zwraca kwotę korekty albo None, gdy stan już się zgadzał.
pub fn set_balance(conn: &Connection, target: i64, note: &str) -> Result<Option<i64>> {
    if !(0..=MAX_AMOUNT).contains(&target) {
        return Err(Error::from("Stan gotówki musi być między 0 a 1 000 000 000 zł"));
    }
    let current = state(conn)?.balance;
    let diff = target - current;
    if diff == 0 {
        return Ok(None);
    }
    let note = check_note(note)?;
    let note = if note.is_empty() { "Korekta stanu".to_string() } else { note };
    conn.execute(
        "INSERT INTO fin_cash (amount, note, source, created_at) VALUES (?1, ?2, 'correction', ?3)",
        params![diff, note, crate::now_ms()],
    )?;
    Ok(Some(diff))
}

/// Edycja ręcznego wpisu lub korekty. Wpisy z bankomatu są tylko do odczytu: wynikają z transakcji.
pub fn update(conn: &Connection, id: i64, amount: i64, note: &str) -> Result<()> {
    check_amount(amount)?;
    let changed = conn.execute(
        "UPDATE fin_cash SET amount = ?2, note = ?3 WHERE id = ?1 AND source <> 'atm'",
        params![id, amount, check_note(note)?],
    )?;
    if changed == 0 {
        return Err(Error::from("Nie ma takiego wpisu albo wpis pochodzi z bankomatu"));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let changed = conn.execute("DELETE FROM fin_cash WHERE id = ?1 AND source <> 'atm'", [id])?;
    if changed == 0 {
        return Err(Error::from("Nie ma takiego wpisu albo wpis pochodzi z bankomatu"));
    }
    Ok(())
}

/// Dodaje wpisy dla wypłat z bankomatu, które jeszcze ich nie mają. Zwraca liczbę nowych wpisów.
pub fn sync_atm(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "INSERT INTO fin_cash (amount, note, source, transaction_id, created_at)
         SELECT -t.amount, t.description, 'atm', t.id, t.imported_at
           FROM fin_transactions t
          WHERE t.amount < 0
            AND (t.kind LIKE '%BANKOMAT%' OR t.description LIKE '%BANKOMAT%')
            AND NOT EXISTS (SELECT 1 FROM fin_cash c WHERE c.transaction_id = t.id)",
        [],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::finance::{add_account, insert_transaction};
    use crate::db::open_memory;
    use crate::finance::millennium::Row;

    #[test]
    fn manual_entries_sum_to_the_balance_and_can_be_edited() {
        let conn = open_memory();
        add(&conn, 20000, "Od rodziny").unwrap();
        let id = add(&conn, -1500, "Napiwek w barze").unwrap();
        assert_eq!(state(&conn).unwrap().balance, 18500);
        update(&conn, id, -2000, "Napiwek w barze, poprawka").unwrap();
        assert_eq!(state(&conn).unwrap().balance, 18000);
        delete(&conn, id).unwrap();
        assert_eq!(state(&conn).unwrap().balance, 20000);
        assert!(add(&conn, 0, "zero").is_err());
        assert!(add(&conn, 100, &"x".repeat(81)).is_err());
    }

    #[test]
    fn setting_balance_writes_a_correction_for_the_difference() {
        let conn = open_memory();
        add(&conn, 10000, "start").unwrap();
        assert_eq!(set_balance(&conn, 10000, "").unwrap(), None, "zgodny stan nie tworzy wpisu");
        assert_eq!(set_balance(&conn, 8000, "Przeliczyłem portfel").unwrap(), Some(-2000));
        let st = state(&conn).unwrap();
        assert_eq!(st.balance, 8000);
        assert_eq!(st.entries[0].source, "correction");
        assert_eq!(st.entries[0].note, "Przeliczyłem portfel");
    }

    #[test]
    fn atm_withdrawals_are_added_once_and_read_only() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some("PL61109010140000071219812874")).unwrap().id;
        let row = Row {
            iban: "PL61109010140000071219812874".into(),
            booked_on: "2026-11-02".into(),
            kind: "PŁATNOŚĆ BLIK".into(),
            counterparty_account: None,
            counterparty: String::new(),
            description: "WYPŁATA BLIK Z BANKOMATU".into(),
            amount: -20000,
            currency: "PLN".into(),
            balance_after: 1000,
        };
        insert_transaction(&conn, acc, &row, "t").unwrap();
        assert_eq!(sync_atm(&conn).unwrap(), 1);
        assert_eq!(sync_atm(&conn).unwrap(), 0, "drugi przebieg nie dubluje wypłaty");
        let st = state(&conn).unwrap();
        assert_eq!((st.balance, st.entries[0].source.as_str()), (20000, "atm"));
        assert!(delete(&conn, st.entries[0].id).is_err());
        assert!(update(&conn, st.entries[0].id, 1, "x").is_err());
    }
}
