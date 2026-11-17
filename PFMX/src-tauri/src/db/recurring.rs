//! Stałe płatności i subskrypcje.
//!
//! Wykrywanie działa po każdym imporcie: klucz odbiorcy to nazwa z pola odbiorcy, a przy zakupach kartą
//! (pole puste) pierwsze znaczące słowo z opisu. Płatność jest cykliczna, gdy powtarza się raz w miesiącu
//! co 25–35 dni przy kwocie w granicach 10%, albo gdy bank sam oznaczył ją jako „PŁATNOŚĆ CYKLICZNA”.
//! Transakcje o tym samym kluczu i podobnej kwocie odhaczają płatność w ich miesiącu automatycznie.
//! Ręczne odhaczenie odlicza ratę; automatyczne nie zmienia liczby rat.

use crate::error::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recurring {
    pub id: i64,
    pub name: String,
    /// „subscription” albo „other”
    pub kind: String,
    /// Rata albo kwota miesięczna, w groszach
    pub amount: i64,
    /// None = bez końca (np. subskrypcja)
    pub installments_left: Option<i64>,
    pub source: String,
    /// Rat nie zostało; takiej płatności nie liczy się do planu
    pub ended: bool,
    pub paid_this_month: bool,
    /// Opłacone automatycznie z transakcji (nie ręcznie)
    pub paid_auto: bool,
    /// Data ostatniej dopasowanej płatności (RRRR-MM-DD)
    pub last_paid_on: Option<String>,
    /// Spodziewana następna płatność: ostatnia + miesiąc
    pub next_due_on: Option<String>,
    /// Nowa kwota z banku, gdy abonament podrożał lub stał się tańszy (tylko subskrypcje)
    pub observed_amount: Option<i64>,
    /// Moment dodania (ms od epoki), żeby pokazać nowo wykryte płatności
    pub created_at: i64,
    /// Kategoria (koperta budżetu); transakcje tej płatności trafiają do niej automatycznie
    pub envelope_id: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecurringInput {
    pub id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub amount: i64,
    pub installments_left: Option<i64>,
    /// None = „Opłaty i subskrypcje”
    #[serde(default)]
    pub envelope_id: Option<i64>,
}

const KINDS: [&str; 2] = ["subscription", "other"];

/// Słowa z opisów banku, które nie są nazwą odbiorcy (wielkimi literami, bez znaków diakrytycznych w kluczu).
const NOISE: &[&str] = &[
    "ZAKUP", "FIZ", "UŻYCIE", "KARTY", "KARTA", "KARTĄ", "PŁATNOŚĆ", "PLATNOSC", "PRZELEW", "WŁASNY", "WEWNĘTRZNY",
    "PRZYCHODZĄCY", "WYCHODZĄCY", "CYKLICZNA", "BLIK", "INTERNECIE", "ONLINE", "TRANSAKCJA", "BEZGOTÓWKOWA",
    "WYPŁATA", "BANKOMATU", "TELEFON", "INNEGO", "BANKU", "SPOLKA", "SP", "ZOO", "JMP", "POL", "IRL", "WARSZAWA",
    "NA", "DO", "PRZEZ", "RATA", "OD", "DLA",
];

/// Odbiorcy znani jako usługi abonamentowe; ich płatności cykliczne trafiają do podstrony Subskrypcje.
const SUBSCRIPTION_VENDORS: &[&str] = &[
    "NETFLIX", "SPOTIFY", "YOUTUBE", "HBO", "DISNEY", "CANAL", "APPLE", "GOOGLE", "MICROSOFT", "ADOBE", "UBER",
    "CHATGPT", "OPENAI", "ICLOUD", "AMAZON", "PLAY", "ORANGE", "PLUS",
];

/// Klucz odbiorcy: nazwa z pola odbiorcy, a gdy pusta, pierwsze znaczące słowo z opisu. Wielkimi literami.
pub fn merchant_key(counterparty: &str, description: &str) -> Option<String> {
    let source = if counterparty.trim().is_empty() { description } else { counterparty };
    let upper = source.to_uppercase();
    upper
        .split(|c: char| !c.is_alphabetic())
        .find(|t| t.chars().count() >= 3 && !NOISE.contains(t))
        .map(str::to_string)
}

/// „RATA 3/12” w opisie znaczy, że zostało 12 − 3 = 9 rat.
pub fn installments_left(description: &str) -> Option<i64> {
    description.split_whitespace().find_map(|tok| {
        let tok = tok.trim_matches(|c: char| !c.is_ascii_digit() && c != '/');
        let (a, b) = tok.split_once('/')?;
        let (a, b): (i64, i64) = (a.parse().ok()?, b.parse().ok()?);
        (a >= 1 && b >= a && b <= 1000).then_some(b - a)
    })
}

/// Dzień od 1970-01-01 z daty RRRR-MM-DD (do liczenia odstępów między płatnościami).
pub(crate) fn day_number(date: &str) -> Option<i64> {
    let y: i64 = date.get(0..4)?.parse().ok()?;
    let m: i64 = date.get(5..7)?.parse().ok()?;
    let d: i64 = date.get(8..10)?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Kwota z transakcji mieści się w 10% kwoty zapisanej przy płatności.
fn within_tolerance(actual: i64, expected: i64) -> bool {
    expected > 0 && (actual - expected).abs() * 10 <= expected
}

/// Id kategorii „Opłaty i subskrypcje”, czyli domyślnej dla płatności cyklicznych.
pub fn fixed_envelope(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT id FROM fin_envelopes WHERE key = 'fixed'", [], |r| r.get(0))?)
}

pub fn list(conn: &Connection, month: &str) -> Result<Vec<Recurring>> {
    let last_paid = last_paid_dates(conn)?;
    let charges = latest_bank_charges(conn)?;
    let fixed = fixed_envelope(conn)?;
    let mut stmt = conn.prepare(
        "SELECT r.id, r.name, r.kind, r.amount, r.installments_left, r.source, r.match_key, r.created_at,
                EXISTS (SELECT 1 FROM fin_recurring_paid p WHERE p.recurring_id = r.id AND p.month = ?1),
                EXISTS (SELECT 1 FROM fin_recurring_match m WHERE m.recurring_id = r.id AND m.month = ?1 AND m.dismissed = 0),
                COALESCE(c.envelope_id, ?2)
           FROM fin_recurring r
           LEFT JOIN fin_recurring_category c ON c.recurring_id = r.id
          WHERE r.hidden = 0
          ORDER BY r.kind, r.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![month, fixed], |r| {
        let id: i64 = r.get(0)?;
        let kind: String = r.get(2)?;
        let amount: i64 = r.get(3)?;
        let installments_left: Option<i64> = r.get(4)?;
        let match_key: Option<String> = r.get(6)?;
        let manual: bool = r.get(8)?;
        let auto: bool = r.get(9)?;
        let last_paid_on = last_paid.get(&id).cloned();
        // zmiana ceny: najnowsza kwota z banku po tym samym odbiorcy, nowsza niż ostatnia płatność
        let observed_amount = match (kind.as_str(), match_key.as_ref().and_then(|k| charges.get(k)), last_paid_on.as_deref()) {
            ("subscription", Some((date, value)), Some(last)) if *value != amount && date.as_str() >= last => Some(*value),
            _ => None,
        };
        Ok(Recurring {
            id,
            name: r.get(1)?,
            kind,
            amount,
            ended: installments_left == Some(0),
            installments_left,
            source: r.get(5)?,
            paid_this_month: manual || auto,
            paid_auto: auto && !manual,
            next_due_on: last_paid_on.as_deref().and_then(add_one_month),
            last_paid_on,
            observed_amount,
            created_at: r.get(7)?,
            envelope_id: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Data ostatniej niecofniętej dopasowanej płatności, po id płatności cyklicznej.
fn last_paid_dates(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut stmt = conn.prepare(
        "SELECT m.recurring_id, MAX(t.booked_on)
           FROM fin_recurring_match m JOIN fin_transactions t ON t.id = m.transaction_id
          WHERE m.dismissed = 0
          GROUP BY m.recurring_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Najnowsze obciążenia oznaczone przez bank jako cykliczne, po kluczu odbiorcy: (data, kwota).
/// Tylko je bierzemy do zmiany ceny, żeby zakupy BLIK u tego samego sprzedawcy nie udawały podwyżki.
fn latest_bank_charges(conn: &Connection) -> Result<HashMap<String, (String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT counterparty, description, booked_on, -amount FROM fin_transactions
          WHERE amount < 0 AND kind LIKE '%CYKLICZN%'
          ORDER BY booked_on DESC, id DESC",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (counterparty, description, date, amount) = row?;
        if let Some(key) = merchant_key(&counterparty, &description) {
            out.entry(key).or_insert((date, amount));
        }
    }
    Ok(out)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Ten sam dzień w następnym miesiącu (ostatni dzień, gdy miesiąc jest krótszy): 2026-01-31 -> 2026-02-28.
pub fn add_one_month(date: &str) -> Option<String> {
    let y: i32 = date.get(0..4)?.parse().ok()?;
    let m: u32 = date.get(5..7)?.parse().ok()?;
    let d: u32 = date.get(8..10)?.parse().ok()?;
    let (ny, nm) = if m >= 12 { (y + 1, 1) } else { (y, m + 1) };
    Some(format!("{:04}-{:02}-{:02}", ny, nm, d.min(days_in_month(ny, nm))))
}

/// Suma miesięcznych płatności z kategorii „Opłaty i subskrypcje”, które jeszcze trwają.
/// Płatność przypisana do innej kategorii (np. karta miesięczna do Transportu) nie wchodzi do tego planu.
pub fn monthly_total(conn: &Connection) -> Result<i64> {
    let fixed = fixed_envelope(conn)?;
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(r.amount), 0) FROM fin_recurring r
           LEFT JOIN fin_recurring_category c ON c.recurring_id = r.id
          WHERE r.hidden = 0
            AND (r.installments_left IS NULL OR r.installments_left > 0)
            AND COALESCE(c.envelope_id, ?1) = ?1",
        [fixed],
        |r| r.get(0),
    )?)
}

/// Ustawia kategorię płatności. Transakcje już dopasowane do tej płatności przechodzą do nowej kategorii.
fn set_category(conn: &Connection, id: i64, envelope: i64) -> Result<()> {
    let exists: i64 = conn.query_row("SELECT count(*) FROM fin_envelopes WHERE id = ?1", [envelope], |r| r.get(0))?;
    if exists == 0 {
        return Err(Error::from("Nie ma takiej kategorii"));
    }
    conn.execute(
        "INSERT OR REPLACE INTO fin_recurring_category (recurring_id, envelope_id) VALUES (?1, ?2)",
        params![id, envelope],
    )?;
    conn.execute(
        "UPDATE fin_tx_envelope SET envelope_id = ?2
          WHERE transaction_id IN (SELECT transaction_id FROM fin_recurring_match WHERE recurring_id = ?1)",
        params![id, envelope],
    )?;
    Ok(())
}

fn validate(input: &RecurringInput) -> Result<()> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(Error::from("Nazwa płatności: od 1 do 60 znaków"));
    }
    if !KINDS.contains(&input.kind.as_str()) {
        return Err(Error::from("Nieznany rodzaj płatności"));
    }
    if input.amount <= 0 {
        return Err(Error::from("Kwota płatności musi być większa od zera"));
    }
    if input.installments_left.is_some_and(|n| !(0..=1000).contains(&n)) {
        return Err(Error::from("Liczba rat: od 0 do 1000"));
    }
    Ok(())
}

pub fn save(conn: &Connection, input: &RecurringInput) -> Result<i64> {
    validate(input)?;
    let name = input.name.trim();
    match input.id {
        Some(id) => {
            // edycja nie zmienia źródła ani klucza, więc wykrywanie nie doda płatności drugi raz
            let changed = conn.execute(
                "UPDATE fin_recurring SET name = ?2, kind = ?3, amount = ?4, installments_left = ?5 WHERE id = ?1",
                params![id, name, input.kind, input.amount, input.installments_left],
            )?;
            if changed == 0 {
                return Err(Error::from("Nie ma takiej płatności"));
            }
            if let Some(envelope) = input.envelope_id {
                set_category(conn, id, envelope)?;
            }
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO fin_recurring (name, kind, amount, installments_left, source, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'manual', ?5)",
                params![name, input.kind, input.amount, input.installments_left, crate::now_ms()],
            )?;
            let id = conn.last_insert_rowid();
            set_category(conn, id, input.envelope_id.unwrap_or(fixed_envelope(conn)?))?;
            Ok(id)
        }
    }
}

/// Ręczne usuwa się na dobre; wykryta z historii tylko się ukrywa (klucz zostaje, więc wraca dopiero po ręcznym dodaniu).
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let source: Option<String> = conn.query_row("SELECT source FROM fin_recurring WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    match source.as_deref() {
        None => Err(Error::from("Nie ma takiej płatności")),
        Some("auto") => {
            conn.execute("UPDATE fin_recurring SET hidden = 1 WHERE id = ?1", [id])?;
            Ok(())
        }
        Some(_) => {
            conn.execute("DELETE FROM fin_recurring WHERE id = ?1", [id])?;
            Ok(())
        }
    }
}

/// Odhacza płatność w miesiącu albo cofa odhaczenie.
/// Ręczne odhaczenie odlicza ratę; automatyczne (z transakcji) nie zmienia rat, tylko zapamiętuje cofnięcie.
pub fn set_paid(conn: &Connection, id: i64, month: &str, paid: bool) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let (installments, active): (Option<i64>, bool) = tx
        .query_row("SELECT installments_left, hidden = 0 FROM fin_recurring WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| Error::from("Nie ma takiej płatności"))?;
    if !active {
        return Err(Error::from("Nie ma takiej płatności"));
    }
    let manual: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM fin_recurring_paid WHERE recurring_id = ?1 AND month = ?2)",
        params![id, month],
        |r| r.get(0),
    )?;
    let auto_active: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM fin_recurring_match WHERE recurring_id = ?1 AND month = ?2 AND dismissed = 0)",
        params![id, month],
        |r| r.get(0),
    )?;
    let auto_any: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM fin_recurring_match WHERE recurring_id = ?1 AND month = ?2)",
        params![id, month],
        |r| r.get(0),
    )?;
    if paid {
        if manual || auto_active {
            // już opłacone
        } else if auto_any {
            // użytkownik wcześniej cofnął automatyczne odhaczenie; przywracamy je bez zmiany rat
            tx.execute("UPDATE fin_recurring_match SET dismissed = 0 WHERE recurring_id = ?1 AND month = ?2", params![id, month])?;
        } else {
            tx.execute("INSERT INTO fin_recurring_paid (recurring_id, month) VALUES (?1, ?2)", params![id, month])?;
            if let Some(n) = installments.filter(|n| *n > 0) {
                tx.execute("UPDATE fin_recurring SET installments_left = ?2 WHERE id = ?1", params![id, n - 1])?;
            }
        }
    } else {
        if manual {
            tx.execute("DELETE FROM fin_recurring_paid WHERE recurring_id = ?1 AND month = ?2", params![id, month])?;
            if let Some(n) = installments {
                tx.execute("UPDATE fin_recurring SET installments_left = ?2 WHERE id = ?1", params![id, n + 1])?;
            }
        }
        if auto_active {
            tx.execute("UPDATE fin_recurring_match SET dismissed = 1 WHERE recurring_id = ?1 AND month = ?2", params![id, month])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Czy seria wpłat/wypłat z tego odbiorcy jest cykliczna.
/// Pojedyncza płatność liczy się tylko wtedy, gdy bank sam oznaczył ją jako cykliczną.
/// Dla serii oznaczonej przez bank kwota może się zmienić (podwyżka), więc `check_amount` jest wtedy fałszem.
fn is_recurring(items: &[&Occ], check_amount: bool) -> bool {
    if items.len() == 1 {
        return items[0].bank_recurring;
    }
    // jedna płatność w miesiącu: wielokrotne zakupy u tego samego sprzedawcy odpadają
    let months: BTreeSet<&str> = items.iter().map(|i| &i.date[..7]).collect();
    if months.len() != items.len() {
        return false;
    }
    let (min, max) = items.iter().fold((i64::MAX, 0), |(lo, hi), i| (lo.min(i.amount), hi.max(i.amount)));
    if check_amount && (max - min) * 10 > max {
        return false;
    }
    items.windows(2).all(|w| match (day_number(&w[0].date), day_number(&w[1].date)) {
        (Some(a), Some(b)) => (25..=35).contains(&(b - a)),
        _ => false,
    })
}

struct Occ {
    date: String,
    /// kwota dodatnia
    amount: i64,
    /// nazwa z pola odbiorcy; None dla zakupów kartą
    counterparty: Option<String>,
    description: String,
    bank_recurring: bool,
}

/// Nazwa do wyświetlenia: nazwa odbiorcy, a dla zakupów kartą klucz zapisany ładnie („Uber”).
fn display_name(key: &str, counterparty: Option<&str>) -> String {
    if let Some(name) = counterparty {
        return name.to_string();
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => key.to_string(),
    }
}

/// Wykrywa nowe płatności cykliczne i odhacza transakcje, które im odpowiadają. Zwraca ile nowych dopisał.
pub fn detect(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT t.counterparty, t.description, t.kind, t.booked_on, -t.amount
           FROM fin_transactions t
          WHERE t.amount < 0
            AND (t.counterparty_account IS NULL
                 OR t.counterparty_account NOT IN (SELECT iban FROM fin_accounts WHERE iban IS NOT NULL))
          ORDER BY t.booked_on",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })?;
    let mut groups: BTreeMap<String, Vec<Occ>> = BTreeMap::new();
    for row in rows {
        let (counterparty, description, kind, date, amount) = row?;
        let Some(key) = merchant_key(&counterparty, &description) else { continue };
        let name = counterparty.trim();
        groups.entry(key).or_default().push(Occ {
            date,
            amount,
            counterparty: (!name.is_empty()).then(|| name.to_string()),
            description,
            bank_recurring: kind.to_uppercase().contains("CYKLICZN"),
        });
    }
    drop(stmt);

    let mut added = 0;
    for (key, items) in groups {
        // najpierw płatności oznaczone przez bank (np. abonament obok zakupów BLIK u tego samego odbiorcy),
        // potem cała grupa; wtedy wystarczy zwykły rytm miesięczny
        let all: Vec<&Occ> = items.iter().collect();
        let marked: Vec<&Occ> = items.iter().filter(|i| i.bank_recurring).collect();
        let series = if !marked.is_empty() && is_recurring(&marked, false) {
            marked
        } else if is_recurring(&all, true) {
            all
        } else {
            continue;
        };
        let latest = *series.last().expect("seria ma co najmniej jeden wpis");
        let kind = if SUBSCRIPTION_VENDORS.contains(&key.as_str()) { "subscription" } else { "other" };
        let name = display_name(&key, latest.counterparty.as_deref());
        added += conn.execute(
            "INSERT OR IGNORE INTO fin_recurring (name, kind, amount, installments_left, source, match_key, created_at)
             VALUES (?1, ?2, ?3, ?4, 'auto', ?5, ?6)",
            params![name, kind, latest.amount, installments_left(&latest.description), key, crate::now_ms()],
        )?;
    }
    auto_match(conn)?;
    Ok(added)
}

/// Dopasowuje nowe transakcje do znanych płatności (ten sam klucz i kwota w granicach 10%).
/// Każda transakcja jest dopasowywana co najwyżej raz, więc cofnięte odhaczenie nie wraca.
fn auto_match(conn: &Connection) -> Result<usize> {
    let pending: Vec<(i64, String, String, i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT t.id, t.counterparty, t.description, -t.amount, substr(t.booked_on, 1, 7)
               FROM fin_transactions t
              WHERE t.amount < 0 AND t.id NOT IN (SELECT transaction_id FROM fin_recurring_match)",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut matched = 0;
    for (tx_id, counterparty, description, amount, month) in pending {
        let Some(key) = merchant_key(&counterparty, &description) else { continue };
        let known: Option<(i64, i64)> = conn
            .query_row("SELECT id, amount FROM fin_recurring WHERE match_key = ?1 AND hidden = 0", [&key], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if let Some((recurring_id, expected)) = known {
            if within_tolerance(amount, expected) {
                conn.execute(
                    "INSERT INTO fin_recurring_match (transaction_id, recurring_id, month, dismissed) VALUES (?1, ?2, ?3, 0)",
                    params![tx_id, recurring_id, month],
                )?;
                // kategoria płatności dziedziczy się na transakcję, ale nie nadpisuje ręcznego przypisania
                conn.execute(
                    "INSERT OR IGNORE INTO fin_tx_envelope (transaction_id, envelope_id)
                     SELECT ?1, COALESCE(c.envelope_id, (SELECT id FROM fin_envelopes WHERE key = 'fixed'))
                       FROM (SELECT 1) LEFT JOIN fin_recurring_category c ON c.recurring_id = ?2",
                    params![tx_id, recurring_id],
                )?;
                matched += 1;
            }
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::finance::{add_account, insert_transaction};
    use crate::db::open_memory;
    use crate::finance::millennium::Row;

    const IBAN: &str = "PL61109010140000071219812874";

    fn tx(conn: &Connection, account: i64, date: &str, counterparty: &str, description: &str, amount: i64, balance_after: i64) {
        let row = Row {
            iban: IBAN.into(),
            booked_on: date.into(),
            kind: "ZAKUP".into(),
            counterparty_account: None,
            counterparty: counterparty.into(),
            description: description.into(),
            amount,
            currency: "PLN".into(),
            balance_after,
        };
        assert!(insert_transaction(conn, account, &row, "test").unwrap());
    }

    fn spend(conn: &Connection, account: i64, date: &str, counterparty: &str, amount: i64, balance_after: i64) {
        tx(conn, account, date, counterparty, "", amount, balance_after);
    }

    fn input(name: &str, kind: &str, amount: i64, installments: Option<i64>) -> RecurringInput {
        RecurringInput { id: None, name: name.into(), kind: kind.into(), amount, installments_left: installments, envelope_id: None }
    }

    #[test]
    fn merchant_key_skips_bank_noise_and_uses_description_for_card_purchases() {
        assert_eq!(merchant_key("", "ZAKUP - FIZ. UŻYCIE KARTY ZABKA Z123 WARSZAWA").as_deref(), Some("ZABKA"));
        assert_eq!(merchant_key("", "PŁATNOŚĆ KARTĄ ONLINE UBER *ONE AMSTERDAM").as_deref(), Some("UBER"));
        assert_eq!(merchant_key("Netflix.com", "PŁATNOŚĆ CYKLICZNA KARTĄ").as_deref(), Some("NETFLIX"));
        assert_eq!(merchant_key("", "PRZELEW WŁASNY"), None);
    }

    #[test]
    fn installments_are_read_from_description() {
        assert_eq!(installments_left("RATA 3/12 KREDYT"), Some(9));
        assert_eq!(installments_left("ZAKUP 12/12."), Some(0));
        assert_eq!(installments_left("ZAKUP 08/11/2026"), None);
    }

    #[test]
    fn day_numbers_differ_by_real_days() {
        assert_eq!(day_number("2026-11-03").unwrap() - day_number("2026-10-03").unwrap(), 31);
        assert_eq!(day_number("2024-03-01").unwrap() - day_number("2024-02-01").unwrap(), 29);
    }

    #[test]
    fn detects_monthly_payments_but_not_shopping() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        spend(&conn, acc, "2026-10-03", "Netflix", -4900, 1000);
        spend(&conn, acc, "2026-11-03", "Netflix", -4900, 500);
        spend(&conn, acc, "2026-10-10", "Santander", -45000, 900);
        spend(&conn, acc, "2026-11-10", "Santander", -45500, 400);
        spend(&conn, acc, "2026-10-12", "Biedronka", -3000, 800);
        spend(&conn, acc, "2026-11-12", "Biedronka", -9000, 0);
        spend(&conn, acc, "2026-11-20", "Meble", -100000, -100000);

        assert_eq!(detect(&conn).unwrap(), 2);
        let list = list(&conn, "2026-11").unwrap();
        let netflix = list.iter().find(|r| r.name == "Netflix").unwrap();
        assert_eq!((netflix.kind.as_str(), netflix.amount, netflix.source.as_str()), ("subscription", 4900, "auto"));
        let santander = list.iter().find(|r| r.name == "Santander").unwrap();
        assert_eq!(santander.kind, "other");
        assert_eq!(santander.amount, 45500, "kwota z ostatniego miesiąca");
        assert_eq!(detect(&conn).unwrap(), 0);
    }

    #[test]
    fn card_subscriptions_are_found_by_description() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        tx(&conn, acc, "2026-10-03", "", "UBER *ONE AMSTERDAM", -2300, 1000);
        tx(&conn, acc, "2026-11-03", "", "UBER *ONE AMSTERDAM", -2300, 500);
        tx(&conn, acc, "2026-11-08", "", "ZABKA Z123 WARSZAWA", -1200, 400);
        tx(&conn, acc, "2026-11-09", "", "ZABKA Z124 WARSZAWA", -800, 300);
        assert_eq!(detect(&conn).unwrap(), 1);
        let r = list(&conn, "2026-11").unwrap().remove(0);
        assert_eq!((r.name.as_str(), r.kind.as_str(), r.amount), ("Uber", "subscription", 2300));
    }

    #[test]
    fn bank_marked_recurring_card_payment_counts_even_once() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        // bank oznaczył tę płatność jako cykliczną, więc jedna wystarczy
        conn.execute(
            "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
             VALUES (?1, '2026-11-02', 'PŁATNOŚĆ CYKLICZNA KARTĄ', '', 'ALLEGRO PLUS', -9900, 'PLN', 100, 't', 'manual-1', 0)",
            [acc],
        )
        .unwrap();
        assert_eq!(detect(&conn).unwrap(), 1);
    }

    #[test]
    fn bank_marked_subscription_is_found_even_next_to_other_payments_to_same_vendor() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        // abonament Uber One oznaczony przez bank jako cykliczny
        conn.execute(
            "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
             VALUES (?1, '2026-10-10', 'PŁATNOŚĆ CYKLICZNA KARTĄ', '', 'UBER   *ONE MEMBERSHIP  AMSTERDAM', -1299, 'PLN', 1000, 't', 'uber-1', 0)",
            [acc],
        )
        .unwrap();
        // zwykłe płatności BLIK do uber.com w tym samym miesiącu
        for (i, amount) in [3514, 546, 6614].iter().enumerate() {
            conn.execute(
                "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
                 VALUES (?1, '2026-10-24', 'PŁATNOŚĆ BLIK W INTERNECIE', 'uber.com Meester Treublaan 7 Amsterdam', 'Uber', ?2, 'PLN', ?3, 't', ?4, 0)",
                params![acc, -amount, 900 - i as i64, format!("uber-blik-{i}")],
            )
            .unwrap();
        }
        assert_eq!(detect(&conn).unwrap(), 1);
        let r = list(&conn, "2026-10").unwrap().remove(0);
        assert_eq!((r.name.as_str(), r.kind.as_str(), r.amount), ("Uber", "subscription", 1299));
        assert!(r.paid_auto, "październikowy abonament jest opłacony z transakcji");
    }

    #[test]
    fn installments_from_description_set_the_remaining_count() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        tx(&conn, acc, "2026-10-05", "Santander", "RATA 3/12", -90000, 900);
        tx(&conn, acc, "2026-11-05", "Santander", "RATA 4/12", -90000, 800);
        detect(&conn).unwrap();
        assert_eq!(list(&conn, "2026-11").unwrap()[0].installments_left, Some(8));
    }

    #[test]
    fn matching_payment_is_ticked_automatically_and_undo_sticks() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        tx(&conn, acc, "2026-10-03", "", "SPOTIFY AB", -2300, 1000);
        tx(&conn, acc, "2026-11-03", "", "SPOTIFY AB", -2300, 500);
        assert_eq!(detect(&conn).unwrap(), 1);
        let id = list(&conn, "2026-11").unwrap()[0].id;
        assert!(list(&conn, "2026-11").unwrap()[0].paid_auto, "listopadowa transakcja odhacza płatność automatycznie");
        // płatność w grudniu pojawia się przy następnym imporcie
        tx(&conn, acc, "2026-12-03", "", "SPOTIFY AB", -2300, 200);
        detect(&conn).unwrap();
        let dec = list(&conn, "2026-12").unwrap().remove(0);
        assert!(dec.paid_this_month && dec.paid_auto);

        // cofnięcie: znika z grudnia i nie wraca przy kolejnym imporcie
        set_paid(&conn, id, "2026-12", false).unwrap();
        detect(&conn).unwrap();
        let after = list(&conn, "2026-12").unwrap().remove(0);
        assert!(!after.paid_this_month);
        // automatyczne odhaczenie nie zmienia rat, a ręczne tak
        assert_eq!(after.installments_left, None);
    }

    #[test]
    fn auto_match_respects_amount_tolerance() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        tx(&conn, acc, "2026-10-03", "", "SPOTIFY AB", -2300, 1000);
        tx(&conn, acc, "2026-11-03", "", "SPOTIFY AB", -2300, 500);
        detect(&conn).unwrap();
        // podwyżka o 50% nie jest tą samą płatnością
        tx(&conn, acc, "2026-12-03", "", "SPOTIFY AB", -3450, 100);
        detect(&conn).unwrap();
        assert!(!list(&conn, "2026-12").unwrap()[0].paid_this_month);
    }

    #[test]
    fn paying_a_month_counts_one_installment_and_undo_restores_it() {
        let conn = open_memory();
        save(&conn, &input("Rata auta", "other", 90000, Some(3))).unwrap();
        let id = list(&conn, "2026-11").unwrap()[0].id;

        set_paid(&conn, id, "2026-11", true).unwrap();
        set_paid(&conn, id, "2026-11", true).unwrap();
        let r = list(&conn, "2026-11").unwrap().remove(0);
        assert_eq!((r.installments_left, r.paid_this_month, r.ended), (Some(2), true, false));
        assert!(!list(&conn, "2026-12").unwrap()[0].paid_this_month);

        set_paid(&conn, id, "2026-11", false).unwrap();
        assert_eq!(list(&conn, "2026-11").unwrap()[0].installments_left, Some(3));
    }

    #[test]
    fn last_installment_ends_the_payment() {
        let conn = open_memory();
        save(&conn, &input("Ostatnia rata", "other", 10000, Some(1))).unwrap();
        let id = list(&conn, "2026-11").unwrap()[0].id;
        set_paid(&conn, id, "2026-11", true).unwrap();
        let r = list(&conn, "2026-11").unwrap().remove(0);
        assert!(r.ended);
        assert_eq!(monthly_total(&conn).unwrap(), 0, "zakończona rata nie wchodzi do planu");
    }

    #[test]
    fn deleting_a_detected_payment_hides_it_for_good() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        spend(&conn, acc, "2026-10-03", "Spotify", -2300, 1000);
        spend(&conn, acc, "2026-11-03", "Spotify", -2300, 500);
        assert_eq!(detect(&conn).unwrap(), 1);
        let id = list(&conn, "2026-11").unwrap()[0].id;
        delete(&conn, id).unwrap();
        assert!(list(&conn, "2026-11").unwrap().is_empty());
        assert_eq!(detect(&conn).unwrap(), 0, "ukryta płatność nie wraca przy kolejnym imporcie");
    }

    #[test]
    fn next_due_is_one_month_later_and_clamped_to_short_months() {
        assert_eq!(add_one_month("2026-01-31").as_deref(), Some("2026-02-28"));
        assert_eq!(add_one_month("2024-01-31").as_deref(), Some("2024-02-29"));
        assert_eq!(add_one_month("2026-12-15").as_deref(), Some("2027-01-15"));
        assert_eq!(add_one_month("zle"), None);
    }

    #[test]
    fn subscription_shows_next_due_and_bank_reported_price_change() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        // abonament Uber One oznaczony przez bank jako cykliczny; pierwsza płatność w październiku
        let cyclical = |date: &str, amount: i64, balance: i64, id: &str| {
            conn.execute(
                "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
                 VALUES (?1, ?2, 'PŁATNOŚĆ CYKLICZNA KARTĄ', '', 'UBER   *ONE MEMBERSHIP', ?3, 'PLN', ?4, 't', ?5, 0)",
                params![acc, date, -amount, balance, id],
            )
            .unwrap();
        };
        cyclical("2026-10-10", 1299, 1000, "u1");
        assert_eq!(detect(&conn).unwrap(), 1);
        let r = list(&conn, "2026-10").unwrap().remove(0);
        assert_eq!(r.last_paid_on.as_deref(), Some("2026-10-10"));
        assert_eq!(r.next_due_on.as_deref(), Some("2026-11-10"));
        assert_eq!(r.observed_amount, None);

        // w listopadzie abonament podrożał; kwota nie pasuje do zapisanej, więc to zmiana ceny, a nie płatność
        cyclical("2026-11-10", 1499, 500, "u2");
        detect(&conn).unwrap();
        let r = list(&conn, "2026-11").unwrap().remove(0);
        assert_eq!(r.amount, 1299, "zapisana kwota zostaje, dopóki użytkownik jej nie zmieni");
        assert_eq!(r.observed_amount, Some(1499));
        assert_eq!(r.last_paid_on.as_deref(), Some("2026-10-10"));
    }

    #[test]
    fn card_purchases_at_same_vendor_do_not_look_like_a_price_change() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        tx_cyclical_uber(&conn);
        for (i, amount) in [3514, 546].iter().enumerate() {
            conn.execute(
                "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
                 VALUES (?1, '2026-11-24', 'PŁATNOŚĆ BLIK W INTERNECIE', 'uber.com Amsterdam', 'Uber', ?2, 'PLN', ?3, 't', ?4, 0)",
                params![acc, -amount, 100 - i as i64, format!("blik-{i}")],
            )
            .unwrap();
        }
        detect(&conn).unwrap();
        let r = list(&conn, "2026-11").unwrap().remove(0);
        assert_eq!(r.observed_amount, None, "zwykłe zakupy BLIK nie są zmianą ceny abonamentu");
    }

    fn tx_cyclical_uber(conn: &Connection) {
        let acc: i64 = conn.query_row("SELECT id FROM fin_accounts LIMIT 1", [], |r| r.get(0)).unwrap();
        conn.execute(
            "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
             VALUES (?1, '2026-10-10', 'PŁATNOŚĆ CYKLICZNA KARTĄ', '', 'UBER   *ONE MEMBERSHIP', -1299, 'PLN', 1000, 't', 'u-base', 0)",
            [acc],
        )
        .unwrap();
    }

    fn envelope_key(conn: &Connection, key: &str) -> i64 {
        conn.query_row("SELECT id FROM fin_envelopes WHERE key = ?1", [key], |r| r.get(0)).unwrap()
    }

    #[test]
    fn monthly_transport_card_counts_once_in_its_own_category() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let transport = envelope_key(&conn, "transport");
        let fixed = envelope_key(&conn, "fixed");
        let id = save(&conn, &RecurringInput { id: None, name: "Karta miesięczna".into(), kind: "other".into(), amount: 11000, installments_left: None, envelope_id: Some(transport) }).unwrap();
        // płatność kartą transportu: kategoria Transport, nie Opłaty
        assert_eq!(list(&conn, "2026-11").unwrap().iter().find(|r| r.id == id).unwrap().envelope_id, transport);
        assert_eq!(monthly_total(&conn).unwrap(), 0, "plan Opłat nie zawiera karty z kategorii Transport");

        // dopasowana transakcja trafia do kategorii płatności
        conn.execute("UPDATE fin_recurring SET match_key = 'KOMUNIKACJA' WHERE id = ?1", [id]).unwrap();
        tx(&conn, acc, "2026-11-01", "Komunikacja", "", -11000, 500);
        assert_eq!(auto_match(&conn).unwrap(), 1);
        let tx_id: i64 = conn.query_row("SELECT id FROM fin_transactions", [], |r| r.get(0)).unwrap();
        // ponowne przypisanie płatności do innej kategorii przenosi już dopasowane transakcje
        save(&conn, &RecurringInput { id: Some(id), name: "Karta miesięczna".into(), kind: "other".into(), amount: 11000, installments_left: None, envelope_id: Some(fixed) }).unwrap();
        let cat: i64 = conn.query_row("SELECT envelope_id FROM fin_tx_envelope WHERE transaction_id = ?1", [tx_id], |r| r.get(0)).unwrap();
        assert_eq!(cat, fixed);
        assert_eq!(monthly_total(&conn).unwrap(), 11000);
    }

    #[test]
    fn invalid_input_is_rejected() {
        let conn = open_memory();
        assert!(save(&conn, &input("", "other", 100, None)).is_err());
        assert!(save(&conn, &input("X", "other", 0, None)).is_err());
        assert!(save(&conn, &input("X", "loan", 100, None)).is_err());
        assert!(save(&conn, &input("X", "other", 100, Some(-1))).is_err());
    }
}
