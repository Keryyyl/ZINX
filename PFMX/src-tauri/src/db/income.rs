//! Wpływy: dwa rodzaje. „salary” to wypłata, czyli wpływ powtarzający się co miesiąc od tego samego nadawcy;
//! „one_off” to jednorazowy wpływ (zwrot, prezent, sprzedaż). Ręczna zmiana jest ważniejsza niż wykrycie.
//! Przelewy między własnymi kontami nie są wpływem.

use crate::db::finance::OWN_TRANSFER_SQL;
use crate::db::recurring::{day_number, merchant_key};
use crate::error::{Error, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;

/// Wpływy z konta, bez przelewów własnych.
pub fn income_sql() -> String {
    format!("t.amount > 0 AND NOT {OWN_TRANSFER_SQL}")
}

pub const KINDS: [&str; 2] = ["salary", "one_off"];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IncomeSummary {
    /// suma wypłat w miesiącu, w groszach
    pub salary: i64,
    /// suma jednorazowych wpływów w miesiącu, w groszach
    pub one_off: i64,
}

/// Wykrywa wypłaty: ten sam nadawca w co najmniej dwóch miesiącach, odstęp 25–35 dni.
/// Wszystkie wpływy biorą udział w wykrywaniu, także ręcznie oznaczone; ręczny rodzaj zapisanego wpisu nie jest nadpisywany.
pub fn classify(conn: &Connection) -> Result<usize> {
    let inc = income_sql();
    let sql = format!(
        "SELECT t.id, t.counterparty, t.description, t.booked_on FROM fin_transactions t
          WHERE {inc}
          ORDER BY t.booked_on"
    );
    let rows: Vec<(i64, String, String, String)> = {
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        mapped.collect::<std::result::Result<_, _>>()?
    };
    let mut groups: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
    let mut keyed: Vec<(i64, Option<String>)> = Vec::new();
    for (id, counterparty, description, date) in rows {
        let key = merchant_key(&counterparty, &description);
        if let Some(k) = &key {
            groups.entry(k.clone()).or_default().push((id, date));
        }
        keyed.push((id, key));
    }
    // stare wpisy dla transakcji, które przestały być wpływami (np. przelew własny), znikają; ręczne zostają
    conn.execute(&format!("DELETE FROM fin_income WHERE manual = 0 AND transaction_id NOT IN (SELECT t.id FROM fin_transactions t WHERE {inc})"), [])?;
    let salary_keys: Vec<String> = groups.iter().filter(|(_, items)| is_monthly(items)).map(|(k, _)| k.clone()).collect();
    let mut changed = 0;
    for (id, key) in keyed {
        let kind = match key {
            Some(k) if salary_keys.contains(&k) => "salary",
            _ => "one_off",
        };
        changed += conn.execute(
            "INSERT INTO fin_income (transaction_id, kind, manual) VALUES (?1, ?2, 0)
             ON CONFLICT(transaction_id) DO UPDATE SET kind = excluded.kind WHERE fin_income.manual = 0",
            params![id, kind],
        )?;
    }
    Ok(changed)
}

/// Co najmniej dwa wpływy, każdy w innym miesiącu, co 25–35 dni.
fn is_monthly(items: &[(i64, String)]) -> bool {
    if items.len() < 2 {
        return false;
    }
    let days: Vec<Option<i64>> = items.iter().map(|(_, d)| day_number(d)).collect();
    let months: std::collections::BTreeSet<&str> = items.iter().map(|(_, d)| &d[..7]).collect();
    months.len() == items.len()
        && days.windows(2).all(|w| matches!((w[0], w[1]), (Some(a), Some(b)) if (25..=35).contains(&(b - a))))
}

/// Ręczna zmiana rodzaju wpływu. Działa tylko na wpływach.
pub fn set_kind(conn: &Connection, transaction_id: i64, kind: &str) -> Result<()> {
    let inc = income_sql();
    if !KINDS.contains(&kind) {
        return Err(Error::from("Nieznany rodzaj wpływu"));
    }
    let is_income: i64 = conn.query_row(
        &format!("SELECT count(*) FROM fin_transactions t WHERE t.id = ?1 AND {inc}"),
        [transaction_id],
        |r| r.get(0),
    )?;
    if is_income == 0 {
        return Err(Error::from("To nie jest wpływ"));
    }
    conn.execute(
        "INSERT INTO fin_income (transaction_id, kind, manual) VALUES (?1, ?2, 1)
         ON CONFLICT(transaction_id) DO UPDATE SET kind = excluded.kind, manual = 1",
        params![transaction_id, kind],
    )?;
    Ok(())
}

pub fn summary(conn: &Connection, month: &str) -> Result<IncomeSummary> {
    let sql = format!(
        "SELECT COALESCE(SUM(CASE WHEN i.kind = 'salary' THEN t.amount ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN i.kind = 'one_off' THEN t.amount ELSE 0 END), 0)
           FROM fin_transactions t JOIN fin_income i ON i.transaction_id = t.id
          WHERE substr(t.booked_on, 1, 7) = ?1 AND {}",
        income_sql()
    );
    Ok(conn.query_row(
        &sql,
        [month],
        |r| Ok(IncomeSummary { salary: r.get(0)?, one_off: r.get(1)? }),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::finance::{add_account, insert_transaction};
    use crate::db::open_memory;
    use crate::finance::millennium::Row;

    const IBAN: &str = "PL61109010140000071219812874";

    fn tx(conn: &Connection, acc: i64, date: &str, counterparty: &str, description: &str, amount: i64, balance: i64) -> i64 {
        let row = Row {
            iban: IBAN.into(),
            booked_on: date.into(),
            kind: "PRZELEW".into(),
            counterparty_account: None,
            counterparty: counterparty.into(),
            description: description.into(),
            amount,
            currency: "PLN".into(),
            balance_after: balance,
        };
        assert!(insert_transaction(conn, acc, &row, "t").unwrap());
        conn.query_row("SELECT id FROM fin_transactions WHERE dedup_key = ?1", [format!("{acc}|{date}|{amount}|{balance}")], |r| r.get(0)).unwrap()
    }

    fn kind_of(conn: &Connection, id: i64) -> Option<String> {
        conn.query_row("SELECT kind FROM fin_income WHERE transaction_id = ?1", [id], |r| r.get(0)).ok()
    }

    #[test]
    fn monthly_sender_is_salary_and_others_are_one_off() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let w1 = tx(&conn, acc, "2026-10-05", "Pracodawca Sp. z o.o.", "WYNAGRODZENIE", 800000, 800000);
        let w2 = tx(&conn, acc, "2026-11-05", "Pracodawca Sp. z o.o.", "WYNAGRODZENIE", 820000, 1620000);
        let zwrot = tx(&conn, acc, "2026-11-08", "Sklep internetowy", "ZWROT ZAMÓWIENIA", 4500, 1624500);
        classify(&conn).unwrap();
        assert_eq!(kind_of(&conn, w1).as_deref(), Some("salary"));
        assert_eq!(kind_of(&conn, w2).as_deref(), Some("salary"));
        assert_eq!(kind_of(&conn, zwrot).as_deref(), Some("one_off"));
        assert_eq!(summary(&conn, "2026-11").unwrap(), IncomeSummary { salary: 820000, one_off: 4500 });
    }

    #[test]
    fn inflow_from_outside_the_app_is_income_even_when_the_bank_calls_it_internal() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        // wpływ z konta spoza aplikacji: bank pisze „wewnętrzny”, ale to nie jest przelew między kontami z listy
        conn.execute(
            "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
             VALUES (?1, '2026-11-06', 'PRZELEW WEWNĘTRZNY PRZYCHODZĄCY', '', 'Przelew - Jan Kowalski', 20000, 'PLN', 21000, 't', 'ext-in', 0)",
            [acc],
        )
        .unwrap();
        let ext: i64 = conn.query_row("SELECT id FROM fin_transactions WHERE dedup_key = 'ext-in'", [], |r| r.get(0)).unwrap();
        classify(&conn).unwrap();
        assert_eq!(kind_of(&conn, ext).as_deref(), Some("one_off"));
        assert_eq!(summary(&conn, "2026-11").unwrap(), IncomeSummary { salary: 0, one_off: 20000 });
        let flagged: bool = conn
            .query_row(&format!("SELECT {} FROM fin_transactions t WHERE t.id = ?1", crate::db::finance::OWN_TRANSFER_SQL), [ext], |r| Ok(r.get::<_, i64>(0)? != 0))
            .unwrap();
        assert!(!flagged);
    }

    #[test]
    fn manual_choice_survives_reclassification_and_own_transfers_are_not_income() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let w1 = tx(&conn, acc, "2026-10-05", "Pracodawca", "WYNAGRODZENIE", 800000, 800000);
        let w2 = tx(&conn, acc, "2026-11-05", "Pracodawca", "WYNAGRODZENIE", 800000, 1600000);
        let own = tx(&conn, acc, "2026-11-06", "", "Przelew własny", 10000, 1610000);
        set_kind(&conn, w2, "one_off").unwrap();
        classify(&conn).unwrap();
        assert_eq!(kind_of(&conn, w1).as_deref(), Some("salary"));
        assert_eq!(kind_of(&conn, w2).as_deref(), Some("one_off"), "ręczny wybór nie jest nadpisywany");
        assert_eq!(kind_of(&conn, own), None, "przelew własny nie jest wpływem");
        assert!(set_kind(&conn, own, "salary").is_err());
        assert!(set_kind(&conn, w1, "bonus").is_err());
    }
}
