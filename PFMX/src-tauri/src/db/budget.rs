//! Budżet miesięczny: koperty z planem, stan realny z otagowanych transakcji i rolowanie niewykorzystanych kwot.
//! Kwoty w groszach. Miesiąc to tekst „YYYY-MM”, więc porównania leksykograficzne są poprawne.

use super::finance::account_summaries;
use super::recurring;
use crate::error::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub const CUSHION_KEY: &str = "cushion_target";
/// Cel poduszki na start: 2000 zł. Użytkownik zmienia go w każdej chwili.
pub const CUSHION_DEFAULT: i64 = 200_000;
/// Największa dozwolona kwota w groszach (miliard złotych); chroni przed pomyłką w polu.
const MAX_AMOUNT: i64 = 100_000_000_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub id: i64,
    pub key: String,
    pub name: String,
    /// Plan na wybrany miesiąc (edytowalny)
    pub planned: i64,
    /// Niewykorzystane z poprzednich miesięcy, przeniesione na ten
    pub carry_in: i64,
    /// Stan realny: suma otagowanych wydatków w tym miesiącu
    pub actual: i64,
    /// Ile zostało: carry_in + planned - actual (może być ujemne)
    pub available: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Cushion {
    pub target: i64,
    /// Suma kont typu „buffer”
    pub balance: i64,
    /// Ile ponad cel; to ma iść na oszczędności albo inwestycje (program tylko informuje)
    pub surplus: i64,
    pub reached: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    pub month: String,
    pub envelopes: Vec<Envelope>,
    pub cushion: Cushion,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeInfo {
    pub id: i64,
    pub key: String,
    pub name: String,
}

/// Kategorie (koperty) w kolejności wyświetlania.
pub fn envelopes(conn: &Connection) -> Result<Vec<EnvelopeInfo>> {
    let mut stmt = conn.prepare("SELECT id, key, name FROM fin_envelopes ORDER BY position")?;
    let rows = stmt.query_map([], |r| Ok(EnvelopeInfo { id: r.get(0)?, key: r.get(1)?, name: r.get(2)? }))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn valid_month(month: &str) -> bool {
    let b = month.as_bytes();
    b.len() == 7 && b[4] == b'-' && b[..4].iter().all(u8::is_ascii_digit) && b[5..].iter().all(u8::is_ascii_digit) && (1..=12).contains(&month[5..].parse::<u32>().unwrap_or(0))
}

pub fn next_month(month: &str) -> String {
    let (y, m): (i32, i32) = (month[..4].parse().unwrap_or(1970), month[5..].parse().unwrap_or(1));
    if m >= 12 {
        format!("{:04}-01", y + 1)
    } else {
        format!("{:04}-{:02}", y, m + 1)
    }
}

fn check_month(month: &str) -> Result<()> {
    if valid_month(month) {
        Ok(())
    } else {
        Err(Error::from("Zły format miesiąca (oczekiwano RRRR-MM)"))
    }
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM fin_settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT OR REPLACE INTO fin_settings (key, value) VALUES (?1, ?2)", params![key, value])?;
    Ok(())
}

pub fn cushion_target(conn: &Connection) -> Result<i64> {
    Ok(get_setting(conn, CUSHION_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(CUSHION_DEFAULT))
}

pub fn set_cushion_target(conn: &Connection, amount: i64) -> Result<()> {
    if !(0..=MAX_AMOUNT).contains(&amount) {
        return Err(Error::from("Cel poduszki musi być między 0 a 1 000 000 000 zł"));
    }
    set_setting(conn, CUSHION_KEY, &amount.to_string())
}

/// Jeśli miesiąca jeszcze nie ma w budżecie, kopiujemy plan z ostatniego wcześniejszego miesiąca.
/// Pierwszy miesiąc w ogóle: stałe opłaty = suma aktywnych płatności, reszta 0.
fn ensure_month(conn: &Connection, month: &str) -> Result<()> {
    let existing: i64 = conn.query_row("SELECT count(*) FROM fin_budget WHERE month = ?1", [month], |r| r.get(0))?;
    if existing > 0 {
        return Ok(());
    }
    let previous: Option<String> = conn.query_row("SELECT max(month) FROM fin_budget WHERE month < ?1", [month], |r| r.get(0))?;
    let envelopes: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, key FROM fin_envelopes ORDER BY position")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let fixed_total = recurring::monthly_total(conn)?;
    for (id, key) in envelopes {
        let planned = match &previous {
            Some(prev) => conn
                .query_row("SELECT planned FROM fin_budget WHERE envelope_id = ?1 AND month = ?2", params![id, prev], |r| r.get(0))
                .optional()?
                .unwrap_or(0),
            None if key == "fixed" => fixed_total,
            None => 0,
        };
        conn.execute("INSERT INTO fin_budget (envelope_id, month, planned) VALUES (?1, ?2, ?3)", params![id, month, planned])?;
    }
    Ok(())
}

/// Uzupełnia plan każdego miesiąca od pierwszych danych do wybranego, żeby żaden nie wypadł z rolowania.
fn ensure_range(conn: &Connection, month: &str) -> Result<()> {
    let first: Option<String> = conn.query_row(
        "SELECT min(m) FROM (SELECT month AS m FROM fin_budget UNION ALL SELECT substr(booked_on, 1, 7) FROM fin_transactions)",
        [],
        |r| r.get(0),
    )?;
    let mut cursor = match first {
        Some(f) if f.as_str() < month => f,
        _ => return ensure_month(conn, month),
    };
    while cursor.as_str() < month {
        ensure_month(conn, &cursor)?;
        cursor = next_month(&cursor);
    }
    ensure_month(conn, month)
}

fn planned_for(conn: &Connection, envelope_id: i64, month: &str) -> Result<i64> {
    Ok(conn
        .query_row("SELECT planned FROM fin_budget WHERE envelope_id = ?1 AND month = ?2", params![envelope_id, month], |r| r.get(0))
        .optional()?
        .unwrap_or(0))
}

/// Wydatki (ujemnie = wpływy zwracane do koperty) z transakcji otagowanych w danym miesiącu.
fn actual_for(conn: &Connection, envelope_id: i64, month: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(-SUM(t.amount), 0)
           FROM fin_tx_envelope e JOIN fin_transactions t ON t.id = e.transaction_id
          WHERE e.envelope_id = ?1 AND substr(t.booked_on, 1, 7) = ?2",
        params![envelope_id, month],
        |r| r.get(0),
    )?)
}

/// Wydatki miesiąca bez kategorii: bez przelewów własnych, bankomatu i wpływów.
fn unassigned_spending(conn: &Connection, month: &str) -> Result<i64> {
    let sql = format!(
        "SELECT COALESCE(-SUM(t.amount), 0) FROM fin_transactions t
          WHERE substr(t.booked_on, 1, 7) = ?1
            AND {}
            AND NOT EXISTS (SELECT 1 FROM fin_tx_envelope e WHERE e.transaction_id = t.id)",
        crate::db::categorize::spending_sql()
    );
    Ok(conn.query_row(&sql, [month], |r| r.get(0))?)
}

/// Najwcześniejszy miesiąc, w którym koperta ma plan albo wydatek.
fn earliest_month(conn: &Connection, envelope_id: i64) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT min(m) FROM (
             SELECT month AS m FROM fin_budget WHERE envelope_id = ?1
             UNION ALL
             SELECT substr(t.booked_on, 1, 7) FROM fin_tx_envelope e
               JOIN fin_transactions t ON t.id = e.transaction_id WHERE e.envelope_id = ?1)",
        [envelope_id],
        |r| r.get(0),
    )?)
}

/// Niewykorzystane kwoty z poprzednich miesięcy: suma (plan - wydatki) od najwcześniejszego miesiąca.
fn carry_into(conn: &Connection, envelope_id: i64, month: &str) -> Result<i64> {
    let start = match earliest_month(conn, envelope_id)? {
        Some(first) if first.as_str() < month => first,
        _ => return Ok(0),
    };
    let mut carry = 0;
    let mut cursor = start;
    while cursor.as_str() < month {
        carry += planned_for(conn, envelope_id, &cursor)? - actual_for(conn, envelope_id, &cursor)?;
        cursor = next_month(&cursor);
    }
    Ok(carry)
}

pub fn view(conn: &Connection, month: &str) -> Result<BudgetView> {
    check_month(month)?;
    ensure_range(conn, month)?;
    let envelopes: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare("SELECT id, key, name FROM fin_envelopes ORDER BY position")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    // „Bez Koperty” to nie koperta: pokazuje wydatki tego miesiąca, które nie mają kategorii (bez planu i przeniesienia)
    let unassigned = unassigned_spending(conn, month)?;
    let mut out = vec![Envelope {
        id: 0,
        key: "unassigned".into(),
        name: "Bez Koperty".into(),
        planned: 0,
        carry_in: 0,
        actual: unassigned,
        available: -unassigned,
    }];
    for (id, key, name) in envelopes {
        let planned = planned_for(conn, id, month)?;
        let carry_in = carry_into(conn, id, month)?;
        let actual = actual_for(conn, id, month)?;
        out.push(Envelope { id, key, name, planned, carry_in, actual, available: carry_in + planned - actual });
    }
    Ok(BudgetView { month: month.to_string(), envelopes: out, cushion: cushion(conn)? })
}

/// Stan poduszki: konta typu „buffer” (w złotówkach) względem celu.
pub fn cushion(conn: &Connection) -> Result<Cushion> {
    let target = cushion_target(conn)?;
    let balance: i64 = account_summaries(conn)?
        .iter()
        .filter(|a| a.kind == "buffer" && a.currency == "PLN")
        .map(|a| a.balance)
        .sum();
    Ok(Cushion { target, balance, surplus: (balance - target).max(0), reached: balance >= target })
}

pub fn set_planned(conn: &Connection, envelope_id: i64, month: &str, amount: i64) -> Result<()> {
    check_month(month)?;
    if !(0..=MAX_AMOUNT).contains(&amount) {
        return Err(Error::from("Kwota koperty musi być między 0 a 1 000 000 000 zł"));
    }
    ensure_range(conn, month)?;
    let changed = conn.execute(
        "UPDATE fin_budget SET planned = ?3 WHERE envelope_id = ?1 AND month = ?2",
        params![envelope_id, month, amount],
    )?;
    if changed == 0 {
        conn.execute("INSERT INTO fin_budget (envelope_id, month, planned) VALUES (?1, ?2, ?3)", params![envelope_id, month, amount])?;
    }
    Ok(())
}

/// Przypisuje transakcję do koperty; None zdejmuje przypisanie.
pub fn tag_transaction(conn: &Connection, transaction_id: i64, envelope_id: Option<i64>) -> Result<()> {
    match envelope_id {
        None => {
            conn.execute("DELETE FROM fin_tx_envelope WHERE transaction_id = ?1", [transaction_id])?;
        }
        Some(envelope) => {
            let exists: i64 = conn.query_row("SELECT count(*) FROM fin_transactions WHERE id = ?1", [transaction_id], |r| r.get(0))?;
            if exists == 0 {
                return Err(Error::from("Nie ma takiej transakcji"));
            }
            conn.execute(
                "INSERT OR REPLACE INTO fin_tx_envelope (transaction_id, envelope_id) VALUES (?1, ?2)",
                params![transaction_id, envelope],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::finance::{add_account, insert_transaction};
    use crate::db::open_memory;
    use crate::finance::millennium::Row;

    const IBAN: &str = "PL61109010140000071219812874";

    fn tx(conn: &Connection, account: i64, booked_on: &str, amount: i64, balance_after: i64, counterparty: &str) -> i64 {
        let row = Row {
            iban: IBAN.into(),
            booked_on: booked_on.into(),
            kind: "ZAKUP".into(),
            counterparty_account: None,
            counterparty: counterparty.into(),
            description: String::new(),
            amount,
            currency: "PLN".into(),
            balance_after,
        };
        assert!(insert_transaction(conn, account, &row, "test").unwrap());
        conn.query_row("SELECT id FROM fin_transactions WHERE dedup_key = ?1", [format!("{account}|{booked_on}|{amount}|{balance_after}")], |r| r.get(0)).unwrap()
    }

    fn env_id(conn: &Connection, key: &str) -> i64 {
        conn.query_row("SELECT id FROM fin_envelopes WHERE key = ?1", [key], |r| r.get(0)).unwrap()
    }

    #[test]
    fn categories_match_the_agreed_list() {
        let conn = open_memory();
        let names: Vec<String> = envelopes(&conn).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(
            names,
            ["Jedzenie i Wydatki Domowe", "Wyjścia", "Transport", "Wydatki Jednorazowe", "Inwestycje", "Rozwój", "Opłaty i Subskrypcje"]
        );
    }

    #[test]
    fn months_roll_forward_and_validate() {
        assert_eq!(next_month("2026-11"), "2026-12");
        assert_eq!(next_month("2026-12"), "2027-01");
        assert!(valid_month("2026-11"));
        assert!(!valid_month("2026-13"));
        assert!(!valid_month("26-11"));
    }

    #[test]
    fn unspent_amount_rolls_into_next_month_and_new_month_copies_plan() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let living = env_id(&conn, "food");
        let t = tx(&conn, acc, "2026-10-05", -30000, 70000, "Sklep");
        tag_transaction(&conn, t, Some(living)).unwrap();
        set_planned(&conn, living, "2026-10", 100000).unwrap();

        let november = view(&conn, "2026-11").unwrap();
        let living_nov = november.envelopes.iter().find(|e| e.key == "food").unwrap();
        assert_eq!(living_nov.planned, 100000, "plan z października ma być skopiowany");
        assert_eq!(living_nov.carry_in, 70000, "300 zł zostało w październiku");
        assert_eq!(living_nov.available, 170000);

        // w październiku nic się nie zmienia: rolowanie działa tylko w przód
        let october = view(&conn, "2026-10").unwrap();
        let living_oct = october.envelopes.iter().find(|e| e.key == "food").unwrap();
        assert_eq!((living_oct.carry_in, living_oct.actual, living_oct.available), (0, 30000, 70000));
    }

    #[test]
    fn overspending_carries_a_negative_balance() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let spending = env_id(&conn, "oneoff");
        set_planned(&conn, spending, "2026-10", 10000).unwrap();
        let t = tx(&conn, acc, "2026-10-09", -25000, 75000, "Restauracja");
        tag_transaction(&conn, t, Some(spending)).unwrap();
        let nov = view(&conn, "2026-11").unwrap();
        let e = nov.envelopes.iter().find(|e| e.key == "oneoff").unwrap();
        assert_eq!(e.carry_in, -15000);
    }

    #[test]
    fn first_month_fixed_bills_default_to_recurring_total() {
        let conn = open_memory();
        conn.execute(
            "INSERT INTO fin_recurring (name, kind, amount, installments_left, source, created_at)
             VALUES ('Rata', 'other', 45000, 10, 'manual', 0), ('Netflix', 'subscription', 4900, NULL, 'manual', 0)",
            [],
        )
        .unwrap();
        let v = view(&conn, "2026-11").unwrap();
        assert_eq!(v.envelopes.iter().find(|e| e.key == "fixed").unwrap().planned, 49900);
    }

    #[test]
    fn cushion_reports_surplus_above_target() {
        let conn = open_memory();
        add_account(&conn, "Poduszka Finansowa", "buffer", Some("PL27114020040000300201355387")).unwrap();
        let acc = conn.query_row("SELECT id FROM fin_accounts WHERE kind = 'buffer'", [], |r| r.get::<_, i64>(0)).unwrap();
        tx(&conn, acc, "2026-11-01", 250000, 250000, "Przelew");

        let c = cushion(&conn).unwrap();
        assert_eq!((c.target, c.balance, c.surplus, c.reached), (200000, 250000, 50000, true));

        set_cushion_target(&conn, 300000).unwrap();
        let c = cushion(&conn).unwrap();
        assert_eq!((c.surplus, c.reached), (0, false));
        assert!(set_cushion_target(&conn, -1).is_err());
    }
}
