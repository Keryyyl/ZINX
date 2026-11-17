//! Kategorie transakcji. Kolejność: wykluczenia (przelewy własne, bankomat, wpływy), reguły zapamiętane z poprawek,
//! potem model lokalny (kolejka w `finance::ai`). Ręczne przypisanie zawsze wygrywa.

use crate::db::finance::OWN_TRANSFER_SQL;
use crate::db::recurring::merchant_key;
use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// Kategorie, do których przypisujemy wydatki (inwestycje nie są wydatkiem).
pub const SPENDING_KEYS: [&str; 6] = ["fixed", "food", "transport", "outings", "oneoff", "development"];

/// Tylko wydatki z konta, które nie są przelewem własnym ani wypłatą z bankomatu.
pub fn spending_sql() -> String {
    format!("t.amount < 0 AND NOT (t.kind LIKE '%BANKOMAT%' OR t.description LIKE '%BANKOMAT%') AND NOT {OWN_TRANSFER_SQL}")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub id: i64,
    pub counterparty: String,
    pub description: String,
    /// kwota dodatnia, w groszach
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiCategory {
    pub id: i64,
    pub key: String,
    pub name: String,
}

pub fn ai_categories(conn: &Connection) -> Result<Vec<AiCategory>> {
    let mut stmt = conn.prepare("SELECT id, key, name FROM fin_envelopes ORDER BY position")?;
    let rows = stmt.query_map([], |r| Ok(AiCategory { id: r.get(0)?, key: r.get(1)?, name: r.get(2)? }))?;
    let all: Vec<AiCategory> = rows.collect::<std::result::Result<_, _>>()?;
    Ok(all.into_iter().filter(|c| SPENDING_KEYS.contains(&c.key.as_str())).collect())
}

/// Stosuje zapamiętane reguły do wydatków bez kategorii. Zwraca liczbę przypisanych transakcji.
pub fn apply_rules(conn: &Connection) -> Result<usize> {
    let sp = spending_sql();
    let rules: HashMap<String, i64> = {
        let mut stmt = conn.prepare("SELECT merchant_key, envelope_id FROM fin_tx_rule")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    if rules.is_empty() {
        return Ok(0);
    }
    let untagged: Vec<(i64, String, String)> = {
        let sql = format!(
            "SELECT t.id, t.counterparty, t.description FROM fin_transactions t
              WHERE {sp} AND NOT EXISTS (SELECT 1 FROM fin_tx_envelope e WHERE e.transaction_id = t.id)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut tagged = 0;
    for (id, counterparty, description) in untagged {
        let Some(key) = merchant_key(&counterparty, &description) else { continue };
        if let Some(envelope) = rules.get(&key) {
            tagged += conn.execute("INSERT OR IGNORE INTO fin_tx_envelope (transaction_id, envelope_id) VALUES (?1, ?2)", params![id, envelope])?;
        }
    }
    Ok(tagged)
}

/// Poprawka użytkownika: zapamiętuje regułę dla odbiorcy i stosuje ją do pozostałych nieotagowanych transakcji.
/// Zwraca liczbę dodatkowo przypisanych transakcji. Transakcje wykluczone nie tworzą reguł.
pub fn learn(conn: &Connection, transaction_id: i64, envelope: i64) -> Result<usize> {
    let sp = spending_sql();
    let row: Option<(String, String, bool)> = conn
        .query_row(
            &format!(
                "SELECT t.counterparty, t.description, ({sp}) FROM fin_transactions t WHERE t.id = ?1"
            ),
            [transaction_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)),
        )
        .optional()?;
    let Some((counterparty, description, is_spending)) = row else { return Ok(0) };
    let Some(key) = merchant_key(&counterparty, &description) else { return Ok(0) };
    if !is_spending {
        return Ok(0);
    }
    conn.execute(
        "INSERT OR REPLACE INTO fin_tx_rule (merchant_key, envelope_id, updated_at) VALUES (?1, ?2, ?3)",
        params![key, envelope, crate::now_ms()],
    )?;
    let sql = format!(
        "SELECT t.id, t.counterparty, t.description FROM fin_transactions t
          WHERE {sp} AND t.id <> ?1 AND NOT EXISTS (SELECT 1 FROM fin_tx_envelope e WHERE e.transaction_id = t.id)"
    );
    let candidates: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([transaction_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut applied = 0;
    for (id, cp, desc) in candidates {
        if merchant_key(&cp, &desc).as_deref() == Some(key.as_str()) {
            applied += conn.execute("INSERT OR IGNORE INTO fin_tx_envelope (transaction_id, envelope_id) VALUES (?1, ?2)", params![id, envelope])?;
        }
    }
    Ok(applied)
}

/// Najświeższe reguły jako przykłady dla modelu: (odbiorca, kategoria).
pub fn examples(conn: &Connection, limit: usize) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT r.merchant_key, e.name FROM fin_tx_rule r JOIN fin_envelopes e ON e.id = r.envelope_id
          ORDER BY r.updated_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Wydatki, których nie ma jeszcze w kategorii i które AI nie oceniało.
pub fn pending(conn: &Connection, limit: usize) -> Result<Vec<Pending>> {
    let sp = spending_sql();
    let sql = format!(
        "SELECT t.id, t.counterparty, t.description, -t.amount FROM fin_transactions t
          WHERE {sp}
            AND NOT EXISTS (SELECT 1 FROM fin_tx_envelope e WHERE e.transaction_id = t.id)
            AND NOT EXISTS (SELECT 1 FROM fin_tx_ai a WHERE a.transaction_id = t.id)
          ORDER BY t.booked_on DESC, t.id DESC LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok(Pending { id: r.get(0)?, counterparty: r.get(1)?, description: r.get(2)?, amount: r.get(3)? })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Zapisuje odpowiedź modelu. Kategoria trafia do transakcji tylko wtedy, gdy model coś wskazał.
pub fn store_ai(conn: &Connection, transaction_id: i64, envelope: Option<i64>) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO fin_tx_ai (transaction_id, envelope_id, checked_at) VALUES (?1, ?2, ?3)",
        params![transaction_id, envelope, crate::now_ms()],
    )?;
    if let Some(envelope) = envelope {
        conn.execute("INSERT OR IGNORE INTO fin_tx_envelope (transaction_id, envelope_id) VALUES (?1, ?2)", params![transaction_id, envelope])?;
    }
    Ok(())
}

pub fn count_untagged_pending(conn: &Connection) -> Result<i64> {
    let sp = spending_sql();
    let sql = format!(
        "SELECT count(*) FROM fin_transactions t WHERE {sp}
           AND NOT EXISTS (SELECT 1 FROM fin_tx_envelope e WHERE e.transaction_id = t.id)"
    );
    Ok(conn.query_row(&sql, [], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::finance::{add_account, insert_transaction};
    use crate::db::open_memory;
    use crate::finance::millennium::Row;

    const IBAN: &str = "PL61109010140000071219812874";

    fn tx(conn: &Connection, account: i64, date: &str, counterparty: &str, description: &str, amount: i64, balance: i64) -> i64 {
        let row = Row {
            iban: IBAN.into(),
            booked_on: date.into(),
            kind: "ZAKUP".into(),
            counterparty_account: None,
            counterparty: counterparty.into(),
            description: description.into(),
            amount,
            currency: "PLN".into(),
            balance_after: balance,
        };
        assert!(insert_transaction(conn, account, &row, "t").unwrap());
        conn.query_row("SELECT id FROM fin_transactions WHERE dedup_key = ?1", [format!("{account}|{date}|{amount}|{balance}")], |r| r.get(0)).unwrap()
    }

    fn env(conn: &Connection, key: &str) -> i64 {
        conn.query_row("SELECT id FROM fin_envelopes WHERE key = ?1", [key], |r| r.get(0)).unwrap()
    }

    #[test]
    fn correction_is_remembered_and_applied_to_the_same_merchant() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let first = tx(&conn, acc, "2026-11-01", "Biedronka", "ZAKUP", -1200, 1000);
        let later = tx(&conn, acc, "2026-11-02", "Biedronka", "ZAKUP", -3400, 900);
        let other = tx(&conn, acc, "2026-11-03", "Żabka", "ZAKUP", -500, 800);
        let food = env(&conn, "food");
        crate::db::budget::tag_transaction(&conn, first, Some(food)).unwrap();
        assert_eq!(learn(&conn, first, food).unwrap(), 1, "druga transakcja Biedronki dostaje tę samą kategorię");
        let got: i64 = conn.query_row("SELECT envelope_id FROM fin_tx_envelope WHERE transaction_id = ?1", [later], |r| r.get(0)).unwrap();
        assert_eq!(got, food);
        assert!(conn.query_row("SELECT envelope_id FROM fin_tx_envelope WHERE transaction_id = ?1", [other], |r| r.get::<_, i64>(0)).is_err());
        // nowa transakcja tego odbiorcy trafia do kategorii przy następnym przebiegu
        let newer = tx(&conn, acc, "2026-11-05", "Biedronka", "ZAKUP", -700, 700);
        assert_eq!(apply_rules(&conn).unwrap(), 1);
        assert!(conn.query_row("SELECT envelope_id FROM fin_tx_envelope WHERE transaction_id = ?1", [newer], |r| r.get::<_, i64>(0)).is_ok());
    }

    #[test]
    fn transfers_atm_and_income_are_never_categorised() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let atm = tx(&conn, acc, "2026-11-02", "", "WYPŁATA BLIK Z BANKOMATU", -20000, 1000);
        let own = tx(&conn, acc, "2026-11-03", "", "PRZELEW WŁASNY", -5000, 900);
        let income = tx(&conn, acc, "2026-11-04", "Pracodawca", "WYNAGRODZENIE", 800000, 900000);
        let spend = tx(&conn, acc, "2026-11-05", "Żabka", "ZAKUP", -500, 899500);
        let pend = pending(&conn, 50).unwrap();
        assert_eq!(pend.iter().map(|p| p.id).collect::<Vec<_>>(), vec![spend]);
        for id in [atm, own, income] {
            assert!(pend.iter().all(|p| p.id != id));
        }
        // ręczne przypisanie wykluczonej transakcji nie tworzy reguły
        assert_eq!(learn(&conn, atm, env(&conn, "oneoff")).unwrap(), 0);
        assert!(examples(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn model_answers_are_stored_and_not_asked_twice() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let a = tx(&conn, acc, "2026-11-01", "Uber", "", -2300, 1000);
        let b = tx(&conn, acc, "2026-11-02", "Nieznany sklep", "", -900, 900);
        store_ai(&conn, a, Some(env(&conn, "transport"))).unwrap();
        store_ai(&conn, b, None).unwrap();
        assert!(pending(&conn, 50).unwrap().is_empty(), "ocenione transakcje nie wracają do kolejki");
        let got: i64 = conn.query_row("SELECT envelope_id FROM fin_tx_envelope WHERE transaction_id = ?1", [a], |r| r.get(0)).unwrap();
        assert_eq!(got, env(&conn, "transport"));
        assert!(conn.query_row("SELECT envelope_id FROM fin_tx_envelope WHERE transaction_id = ?1", [b], |r| r.get::<_, i64>(0)).is_err());
    }
}
