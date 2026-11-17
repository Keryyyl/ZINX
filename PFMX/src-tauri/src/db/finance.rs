//! Konta i transakcje finansów osobistych. Tabele są osobne od poczty; poczta nie zapisuje tu niczego.

use crate::error::{Error, Result};
use crate::finance::millennium::Row;
use crate::finance::money::normalize_iban;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS fin_accounts (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  iban TEXT UNIQUE,
  currency TEXT NOT NULL DEFAULT 'PLN',
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS fin_transactions (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES fin_accounts(id) ON DELETE CASCADE,
  booked_on TEXT NOT NULL,
  kind TEXT NOT NULL,
  counterparty_account TEXT,
  counterparty TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',
  amount INTEGER NOT NULL,
  currency TEXT NOT NULL,
  balance_after INTEGER NOT NULL,
  source TEXT NOT NULL,
  dedup_key TEXT NOT NULL UNIQUE,
  imported_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_fin_tx_account_date ON fin_transactions(account_id, booked_on);
CREATE TABLE IF NOT EXISTS fin_imports (
  id INTEGER PRIMARY KEY,
  file_name TEXT NOT NULL,
  imported_at INTEGER NOT NULL,
  added INTEGER NOT NULL,
  skipped INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS fin_envelopes (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  position INTEGER NOT NULL
);
INSERT OR IGNORE INTO fin_envelopes (id, key, name, position) VALUES
  (1, 'fixed', 'Stałe opłaty', 1),
  (2, 'investments', 'Inwestycje', 2),
  (3, 'living', 'Życie', 3),
  (4, 'spending', 'Wydatki', 4);
/* planowana kwota koperty w danym miesiącu (YYYY-MM), w groszach */
CREATE TABLE IF NOT EXISTS fin_budget (
  envelope_id INTEGER NOT NULL REFERENCES fin_envelopes(id) ON DELETE CASCADE,
  month TEXT NOT NULL,
  planned INTEGER NOT NULL,
  PRIMARY KEY (envelope_id, month)
);
/* transakcja przypisana do koperty; od tagowania przez AI to samo miejsce */
CREATE TABLE IF NOT EXISTS fin_tx_envelope (
  transaction_id INTEGER PRIMARY KEY REFERENCES fin_transactions(id) ON DELETE CASCADE,
  envelope_id INTEGER NOT NULL REFERENCES fin_envelopes(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_fin_tx_envelope ON fin_tx_envelope(envelope_id);
CREATE TABLE IF NOT EXISTS fin_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT OR IGNORE INTO fin_settings (key, value) VALUES ('cushion_target', '200000');
/* stałe płatności i subskrypcje; auto = wykryte z historii, manual = dodane ręcznie */
CREATE TABLE IF NOT EXISTS fin_recurring (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('subscription', 'other')),
  amount INTEGER NOT NULL CHECK (amount > 0),
  installments_left INTEGER CHECK (installments_left IS NULL OR installments_left >= 0),
  source TEXT NOT NULL CHECK (source IN ('auto', 'manual')),
  /* klucz wykrytej płatności; zostaje też po ukryciu, żeby wykrywanie jej nie dodało ponownie */
  match_key TEXT UNIQUE,
  hidden INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS fin_recurring_paid (
  recurring_id INTEGER NOT NULL REFERENCES fin_recurring(id) ON DELETE CASCADE,
  month TEXT NOT NULL,
  PRIMARY KEY (recurring_id, month)
);
/* transakcja dopasowana do płatności cyklicznej; dismissed = użytkownik cofnął automatyczne odhaczenie */
CREATE TABLE IF NOT EXISTS fin_recurring_match (
  transaction_id INTEGER PRIMARY KEY REFERENCES fin_transactions(id) ON DELETE CASCADE,
  recurring_id INTEGER NOT NULL REFERENCES fin_recurring(id) ON DELETE CASCADE,
  month TEXT NOT NULL,
  dismissed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_fin_match_recurring ON fin_recurring_match(recurring_id, month);
/* rodzaj wpływu: wypłata (powtarza się co miesiąc) albo jednorazowy; manual = ustawiony ręcznie */
CREATE TABLE IF NOT EXISTS fin_income (
  transaction_id INTEGER PRIMARY KEY REFERENCES fin_transactions(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('salary', 'one_off')),
  manual INTEGER NOT NULL DEFAULT 0
);
/* reguła zapamiętana z poprawki: odbiorca -> kategoria */
CREATE TABLE IF NOT EXISTS fin_tx_rule (
  merchant_key TEXT PRIMARY KEY,
  envelope_id INTEGER NOT NULL REFERENCES fin_envelopes(id) ON DELETE CASCADE,
  updated_at INTEGER NOT NULL
);
/* transakcje, które AI już oceniło (envelope_id NULL = nie wiedziało); nie pytamy o nie drugi raz */
CREATE TABLE IF NOT EXISTS fin_tx_ai (
  transaction_id INTEGER PRIMARY KEY REFERENCES fin_transactions(id) ON DELETE CASCADE,
  envelope_id INTEGER REFERENCES fin_envelopes(id) ON DELETE SET NULL,
  checked_at INTEGER NOT NULL
);
/* kategorie = koperty budżetu; nazwy i kolejność są idempotentne, więc można je uruchamiać przy każdym starcie */
UPDATE fin_envelopes SET name = 'Opłaty i Subskrypcje', position = 8 WHERE key = 'fixed';
UPDATE fin_envelopes SET key = 'food', name = 'Jedzenie', position = 2 WHERE key = 'living';
UPDATE fin_envelopes SET key = 'oneoff', name = 'Wydatki jednorazowe', position = 5 WHERE key = 'spending';
UPDATE fin_envelopes SET position = 6 WHERE key = 'investments';
UPDATE fin_envelopes SET name = 'Jedzenie i Wydatki Domowe', position = 2 WHERE key = 'food';
UPDATE fin_envelopes SET name = 'Wyjścia', position = 3 WHERE key = 'outings';
UPDATE fin_envelopes SET name = 'Transport', position = 4 WHERE key = 'transport';
UPDATE fin_envelopes SET name = 'Wydatki Jednorazowe', position = 5 WHERE key = 'oneoff';
UPDATE fin_envelopes SET name = 'Rozwój', position = 7 WHERE key = 'development';
UPDATE fin_envelopes SET name = 'Opłaty i Subskrypcje', position = 8 WHERE key = 'fixed';
INSERT OR IGNORE INTO fin_envelopes (id, key, name, position) VALUES
  (5, 'transport', 'Transport', 4),
  (6, 'outings', 'Wyjścia', 3),
  (7, 'development', 'Rozwój', 7);
/* kategoria płatności cyklicznej; brak wiersza = „Opłaty i subskrypcje” */
CREATE TABLE IF NOT EXISTS fin_recurring_category (
  recurring_id INTEGER PRIMARY KEY REFERENCES fin_recurring(id) ON DELETE CASCADE,
  envelope_id INTEGER NOT NULL REFERENCES fin_envelopes(id) ON DELETE CASCADE
);
/* gotówka: suma wpisów to aktualny stan; wpisy z bankomatu są powiązane z transakcją */
CREATE TABLE IF NOT EXISTS fin_cash (
  id INTEGER PRIMARY KEY,
  amount INTEGER NOT NULL CHECK (amount <> 0),
  note TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL CHECK (source IN ('manual', 'atm', 'correction')),
  transaction_id INTEGER UNIQUE REFERENCES fin_transactions(id) ON DELETE SET NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS inv_ops (
  id INTEGER PRIMARY KEY,
  xtb_id TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('deposit', 'withdrawal', 'interest', 'interest_tax', 'buy', 'sell')),
  ticker TEXT NOT NULL DEFAULT '',
  instrument TEXT NOT NULL DEFAULT '',
  position_id TEXT NOT NULL DEFAULT '',
  volume INTEGER NOT NULL DEFAULT 0,
  price INTEGER NOT NULL DEFAULT 0,
  amount INTEGER NOT NULL,
  occurred_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS inv_ops_time ON inv_ops(occurred_at, id);
";

/// Rodzaje kont. „broker” jest zarezerwowany pod inwestycje (XTB), które dojdą później.
pub const KINDS: [&str; 4] = ["bank", "savings", "buffer", "broker"];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FinAccount {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// Numer rachunku do dopasowania importów; konta bez banku (np. broker) go nie mają
    pub iban: Option<String>,
    pub currency: String,
}

pub fn add_account(conn: &Connection, name: &str, kind: &str, iban: Option<&str>) -> Result<FinAccount> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Nazwa konta: od 1 do 40 znaków".into());
    }
    if !KINDS.contains(&kind) {
        return Err("Nieznany rodzaj konta".into());
    }
    let iban = iban.map(normalize_iban).transpose()?;
    conn.execute(
        "INSERT INTO fin_accounts (name, kind, iban, currency, created_at) VALUES (?1, ?2, ?3, 'PLN', ?4)",
        params![name, kind, iban, crate::now_ms()],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            Error::from("To konto jest już dodane")
        } else {
            Error::from(e)
        }
    })?;
    Ok(FinAccount { id: conn.last_insert_rowid(), name: name.to_string(), kind: kind.to_string(), iban, currency: "PLN".into() })
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<FinAccount>> {
    let mut stmt = conn.prepare("SELECT id, name, kind, iban, currency FROM fin_accounts ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(FinAccount { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, iban: r.get(3)?, currency: r.get(4)? })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Numer rachunku -> (id konta, nazwa konta)
pub fn accounts_by_iban(conn: &Connection) -> Result<HashMap<String, (i64, String)>> {
    Ok(list_accounts(conn)?.into_iter().filter_map(|a| a.iban.map(|iban| (iban, (a.id, a.name)))).collect())
}

/// Dodaje transakcję, jeśli jej jeszcze nie ma. Zwraca false dla duplikatu.
///
/// Klucz to konto, data, kwota i saldo po transakcji. Saldo zmienia się przy każdej operacji, więc dwie
/// identyczne kawy tego samego dnia mają różne klucze, a ponowny import tego samego pliku niczego nie dubluje.
pub fn insert_transaction(conn: &Connection, account_id: i64, row: &Row, source: &str) -> Result<bool> {
    let dedup_key = format!("{account_id}|{}|{}|{}", row.booked_on, row.amount, row.balance_after);
    let changed = conn.execute(
        "INSERT OR IGNORE INTO fin_transactions
           (account_id, booked_on, kind, counterparty_account, counterparty, description, amount, currency,
            balance_after, source, dedup_key, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            account_id,
            row.booked_on,
            row.kind,
            row.counterparty_account,
            row.counterparty,
            row.description,
            row.amount,
            row.currency,
            row.balance_after,
            source,
            dedup_key,
            crate::now_ms()
        ],
    )?;
    Ok(changed == 1)
}

/// Przelew między własnymi kontami (w obie strony). Dwa sygnały: numer konta z listy aplikacji albo opis „Przelew własny”.
/// Sam rodzaj „wewnętrzny” nie wystarcza: bank używa go też dla wpływów z kont spoza aplikacji.
/// Wielkość liter sprawdzamy w obu wariantach, bo SQLite nie zmienia polskich liter spoza ASCII.
pub const OWN_TRANSFER_SQL: &str = "(IFNULL(t.counterparty_account IN (SELECT iban FROM fin_accounts WHERE iban IS NOT NULL), 0)
  OR t.description LIKE '%WŁASNY%' OR t.description LIKE '%własny%')";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// Numer rachunku w postaci „…2874”; pełny zostaje w bazie
    pub iban_masked: Option<String>,
    pub currency: String,
    /// Saldo po ostatniej transakcji, w groszach (0, gdy konto nie ma jeszcze transakcji)
    pub balance: i64,
    pub last_booked_on: Option<String>,
}

pub fn account_summaries(conn: &Connection) -> Result<Vec<AccountSummary>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, a.kind, a.iban, a.currency,
                (SELECT t.balance_after FROM fin_transactions t
                   WHERE t.account_id = a.id ORDER BY t.booked_on DESC, t.id DESC LIMIT 1),
                (SELECT max(t.booked_on) FROM fin_transactions t WHERE t.account_id = a.id)
           FROM fin_accounts a ORDER BY a.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let iban: Option<String> = r.get(3)?;
        Ok(AccountSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            iban_masked: iban.as_deref().map(crate::finance::money::mask_iban),
            currency: r.get(4)?,
            balance: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
            last_booked_on: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRow {
    pub id: i64,
    pub account_id: i64,
    pub account_name: String,
    pub booked_on: String,
    pub kind: String,
    pub counterparty: String,
    pub description: String,
    /// Kwota ze znakiem, w groszach
    pub amount: i64,
    pub currency: String,
    pub balance_after: i64,
    /// Koperta przypisana ręcznie albo przez AI; None = jeszcze nieotagowana
    pub envelope_id: Option<i64>,
    /// Dla wpływów: „salary” (wypłata) albo „one_off” (jednorazowy); None dla wydatków
    pub income_kind: Option<String>,
    /// Przelew między własnymi kontami (w obie strony); nie jest wydatkiem ani wpływem
    pub own_transfer: bool,
}

/// Najnowsze transakcje; bez `account_id` wszystkie konta razem. Kolejność: od najnowszej.
pub fn list_transactions(conn: &Connection, account_id: Option<i64>, limit: i64) -> Result<Vec<TransactionRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id, t.account_id, a.name, t.booked_on, t.kind, t.counterparty, t.description,
                t.amount, t.currency, t.balance_after, e.envelope_id, i.kind,
                {OWN_TRANSFER_SQL} AS own
           FROM fin_transactions t
           JOIN fin_accounts a ON a.id = t.account_id
           LEFT JOIN fin_tx_envelope e ON e.transaction_id = t.id
           LEFT JOIN fin_income i ON i.transaction_id = t.id
          WHERE (?1 IS NULL OR t.account_id = ?1)
          ORDER BY t.booked_on DESC, t.id DESC
          LIMIT ?2",
    ))?;
    let rows = stmt.query_map(params![account_id, limit.clamp(1, 500)], |r| {
        Ok(TransactionRow {
            id: r.get(0)?,
            account_id: r.get(1)?,
            account_name: r.get(2)?,
            booked_on: r.get(3)?,
            kind: r.get(4)?,
            counterparty: r.get(5)?,
            description: r.get(6)?,
            amount: r.get(7)?,
            currency: r.get(8)?,
            balance_after: r.get(9)?,
            envelope_id: r.get(10)?,
            income_kind: r.get(11)?,
            own_transfer: r.get::<_, i64>(12)? != 0,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Transakcje w układzie eksportu banku, najnowsze pierwsze. Bez `account_id` wszystkie konta z numerem rachunku.
pub fn export_rows(conn: &Connection, account_id: Option<i64>) -> Result<Vec<crate::finance::millennium::Row>> {
    let mut stmt = conn.prepare(
        "SELECT a.iban, t.booked_on, t.kind, t.counterparty_account, t.counterparty, t.description, t.amount, t.currency, t.balance_after
           FROM fin_transactions t JOIN fin_accounts a ON a.id = t.account_id
          WHERE a.iban IS NOT NULL AND (?1 IS NULL OR t.account_id = ?1)
          ORDER BY t.booked_on DESC, t.id DESC",
    )?;
    let rows = stmt.query_map([account_id], |r| {
        Ok(crate::finance::millennium::Row {
            iban: r.get(0)?,
            booked_on: r.get(1)?,
            kind: r.get(2)?,
            counterparty_account: r.get(3)?,
            counterparty: r.get(4)?,
            description: r.get(5)?,
            amount: r.get(6)?,
            currency: r.get(7)?,
            balance_after: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Kryteria wyszukiwania transakcji. Puste pole = bez ograniczenia.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionQuery {
    pub account_id: Option<i64>,
    /// id kategorii; 0 = wydatki bez kategorii („Bez Koperty”)
    pub category: Option<i64>,
    /// „income” (wpływ) albo „spending” (płatność)
    pub kind: Option<String>,
    /// RRRR-MM-DD, włącznie
    pub date_from: Option<String>,
    /// RRRR-MM-DD, włącznie
    pub date_to: Option<String>,
    /// wielkość transakcji w groszach (wartość bezwzględna)
    pub amount_min: Option<i64>,
    pub amount_max: Option<i64>,
}

/// Wyszukiwanie transakcji po kategorii, rodzaju, dacie i wielkości. Najnowsze pierwsze, do 500 wyników.
pub fn search_transactions(conn: &Connection, q: &TransactionQuery) -> Result<Vec<TransactionRow>> {
    for date in [&q.date_from, &q.date_to].into_iter().flatten() {
        if date.len() != 10 || date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' {
            return Err(Error::from("Zła data (oczekiwano RRRR-MM-DD)"));
        }
    }
    if let Some(kind) = &q.kind {
        if kind != "income" && kind != "spending" {
            return Err(Error::from("Nieznany rodzaj transakcji"));
        }
    }
    let spending = crate::db::categorize::spending_sql();
    let sql = format!(
        "SELECT t.id, t.account_id, a.name, t.booked_on, t.kind, t.counterparty, t.description,
                t.amount, t.currency, t.balance_after, e.envelope_id, i.kind,
                {OWN_TRANSFER_SQL} AS own
           FROM fin_transactions t
           JOIN fin_accounts a ON a.id = t.account_id
           LEFT JOIN fin_tx_envelope e ON e.transaction_id = t.id
           LEFT JOIN fin_income i ON i.transaction_id = t.id
          WHERE (?1 IS NULL OR t.account_id = ?1)
            AND (?2 IS NULL OR e.envelope_id = ?2 OR (?2 = 0 AND e.envelope_id IS NULL AND {spending}))
            AND (?3 IS NULL OR (?3 = 'income' AND t.amount > 0) OR (?3 = 'spending' AND t.amount < 0))
            AND (?4 IS NULL OR t.booked_on >= ?4)
            AND (?5 IS NULL OR t.booked_on <= ?5)
            AND (?6 IS NULL OR ABS(t.amount) >= ?6)
            AND (?7 IS NULL OR ABS(t.amount) <= ?7)
          ORDER BY t.booked_on DESC, t.id DESC
          LIMIT 500"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![q.account_id, q.category, q.kind, q.date_from, q.date_to, q.amount_min, q.amount_max],
        |r| {
            Ok(TransactionRow {
                id: r.get(0)?,
                account_id: r.get(1)?,
                account_name: r.get(2)?,
                booked_on: r.get(3)?,
                kind: r.get(4)?,
                counterparty: r.get(5)?,
                description: r.get(6)?,
                amount: r.get(7)?,
                currency: r.get(8)?,
                balance_after: r.get(9)?,
                envelope_id: r.get(10)?,
                income_kind: r.get(11)?,
                own_transfer: r.get::<_, i64>(12)? != 0,
            })
        },
    )?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyFlow {
    /// RRRR-MM-DD
    pub date: String,
    /// Suma kwot wszystkich kont tego dnia, ze znakiem, w groszach
    pub net: i64,
}

/// Przepływ netto dzień po dniu od `since` (RRRR-MM-DD). Dni bez transakcji nie występują w wyniku.
pub fn daily_flow(conn: &Connection, since: &str) -> Result<Vec<DailyFlow>> {
    if since.len() != 10 || since.as_bytes()[4] != b'-' || since.as_bytes()[7] != b'-' {
        return Err(Error::from("Zła data początku (oczekiwano RRRR-MM-DD)"));
    }
    let mut stmt = conn.prepare(
        "SELECT booked_on, SUM(amount) FROM fin_transactions
          WHERE booked_on >= ?1
          GROUP BY booked_on
          ORDER BY booked_on",
    )?;
    let rows = stmt.query_map([since], |r| Ok(DailyFlow { date: r.get(0)?, net: r.get(1)? }))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::db::open_memory;

    #[test]
    fn daily_flow_sums_all_accounts_per_day_from_the_start_date() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some("PL61109010140000071219812874")).unwrap();
        // (data, kwota, saldo po transakcji): saldo wyróżnia wiersze w kluczu deduplikacji
        for (date, amount, balance_after) in [("2026-10-01", 1000, 1000), ("2026-10-01", -300, 700), ("2026-10-05", -200, 500), ("2026-09-20", 50, 50)] {
            let row = Row {
                iban: "PL61109010140000071219812874".into(),
                booked_on: date.into(),
                kind: "X".into(),
                counterparty_account: None,
                counterparty: "Y".into(),
                description: String::new(),
                amount,
                currency: "PLN".into(),
                balance_after,
            };
            assert!(insert_transaction(&conn, acc.id, &row, "t").unwrap());
        }
        let flow = daily_flow(&conn, "2026-10-01").unwrap();
        assert_eq!(flow, vec![DailyFlow { date: "2026-10-01".into(), net: 700 }, DailyFlow { date: "2026-10-05".into(), net: -200 }]);
        assert!(daily_flow(&conn, "1.10.2026").is_err());
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::db::open_memory;

    const IBAN: &str = "PL61109010140000071219812874";

    fn add(conn: &Connection, acc: i64, date: &str, counterparty: &str, description: &str, amount: i64, balance: i64) -> i64 {
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
        insert_transaction(conn, acc, &row, "t").unwrap();
        conn.query_row("SELECT id FROM fin_transactions WHERE dedup_key = ?1", [format!("{acc}|{date}|{amount}|{balance}")], |r| r.get(0)).unwrap()
    }

    #[test]
    fn search_combines_category_kind_date_and_amount() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some(IBAN)).unwrap().id;
        let taxi = add(&conn, acc, "2026-11-02", "Uber", "UBER TRIP", -1200, 800);
        let food = add(&conn, acc, "2026-11-03", "Biedronka", "ZAKUP", -4500, 300);
        let unassigned = add(&conn, acc, "2026-11-04", "Kiosk", "ZAKUP", -300, 0);
        let salary = add(&conn, acc, "2026-10-05", "Pracodawca", "WYNAGRODZENIE", 800000, 800000);
        let transport: i64 = conn.query_row("SELECT id FROM fin_envelopes WHERE key = 'transport'", [], |r| r.get(0)).unwrap();
        let food_env: i64 = conn.query_row("SELECT id FROM fin_envelopes WHERE key = 'food'", [], |r| r.get(0)).unwrap();
        crate::db::budget::tag_transaction(&conn, taxi, Some(transport)).unwrap();
        crate::db::budget::tag_transaction(&conn, food, Some(food_env)).unwrap();

        let ids = |q: TransactionQuery| -> Vec<i64> { search_transactions(&conn, &q).unwrap().iter().map(|r| r.id).collect() };
        // wszystkie: bez ograniczeń
        assert_eq!(ids(TransactionQuery::default()).len(), 4);
        // kategoria: tylko ta kategoria
        assert_eq!(ids(TransactionQuery { category: Some(transport), ..Default::default() }), vec![taxi]);
        // „Bez Koperty” to wydatki bez kategorii
        assert_eq!(ids(TransactionQuery { category: Some(0), ..Default::default() }), vec![unassigned]);
        // rodzaj: wpływ / płatność
        assert_eq!(ids(TransactionQuery { kind: Some("income".into()), ..Default::default() }), vec![salary]);
        assert_eq!(ids(TransactionQuery { kind: Some("spending".into()), ..Default::default() }).len(), 3);
        // data od / do, włącznie
        assert_eq!(ids(TransactionQuery { date_from: Some("2026-11-03".into()), date_to: Some("2026-11-03".into()), ..Default::default() }), vec![food]);
        // wielkość: wartość bezwzględna między progami
        assert_eq!(ids(TransactionQuery { amount_min: Some(1000), amount_max: Some(1200), ..Default::default() }), vec![taxi]);
        // kategoria i wielkość razem
        assert!(ids(TransactionQuery { category: Some(food_env), amount_min: Some(10000), ..Default::default() }).is_empty());
        assert!(search_transactions(&conn, &TransactionQuery { date_from: Some("3.11.2026".into()), ..Default::default() }).is_err());
    }
}

#[cfg(test)]
mod own_transfer_tests {
    use super::*;
    use crate::db::open_memory;

    #[test]
    fn transfers_between_own_accounts_are_marked() {
        let conn = open_memory();
        let main = add_account(&conn, "Zwykłe", "bank", Some("PL61109010140000071219812874")).unwrap();
        let savings = add_account(&conn, "Oszczędnościowe", "savings", Some("PL27114020040000300201355387")).unwrap();
        let row = |date: &str, counterparty_account: Option<&str>, description: &str, amount: i64, balance: i64| Row {
            iban: "PL61109010140000071219812874".into(),
            booked_on: date.into(),
            kind: "PRZELEW".into(),
            counterparty_account: counterparty_account.map(str::to_string),
            counterparty: String::new(),
            description: description.into(),
            amount,
            currency: "PLN".into(),
            balance_after: balance,
        };
        insert_transaction(&conn, main.id, &row("2026-11-01", Some("PL27114020040000300201355387"), "PRZELEW", -10000, 1000), "t").unwrap();
        insert_transaction(&conn, savings.id, &row("2026-11-01", None, "Przelew własny", 10000, 10000), "t").unwrap();
        insert_transaction(&conn, main.id, &row("2026-11-02", None, "ZAKUP - FIZ. UŻYCIE KARTY", -500, 500), "t").unwrap();
        let rows = list_transactions(&conn, None, 50).unwrap();
        let own: Vec<bool> = rows.iter().map(|r| r.own_transfer).collect();
        assert_eq!(own.iter().filter(|x| **x).count(), 2);
        assert!(rows.iter().any(|r| r.own_transfer && r.amount > 0));
        assert!(rows.iter().any(|r| !r.own_transfer && r.amount < 0));
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;
    use crate::db::open_memory;

    fn row(booked_on: &str, amount: i64, balance_after: i64) -> Row {
        Row {
            iban: "PL61109010140000071219812874".into(),
            booked_on: booked_on.into(),
            kind: "ZAKUP".into(),
            counterparty_account: None,
            counterparty: "Sklep".into(),
            description: String::new(),
            amount,
            currency: "PLN".into(),
            balance_after,
        }
    }

    #[test]
    fn balance_is_taken_from_the_latest_transaction_of_each_account() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some("PL61109010140000071219812874")).unwrap();
        add_account(&conn, "Poduszka", "buffer", None).unwrap();
        // w pliku wiersze są od najnowszego; import zapisuje od najstarszego, więc wyższy id = nowszy wpis
        insert_transaction(&conn, acc.id, &row("2026-10-01", 20000, 20000), "t").unwrap();
        insert_transaction(&conn, acc.id, &row("2026-10-02", -3629, 16371), "t").unwrap();
        insert_transaction(&conn, acc.id, &row("2026-10-02", -500, 15871), "t").unwrap();

        let summaries = account_summaries(&conn).unwrap();
        assert_eq!(summaries[0].balance, 15871);
        assert_eq!(summaries[0].last_booked_on.as_deref(), Some("2026-10-02"));
        assert_eq!(summaries[0].iban_masked.as_deref(), Some("…2874"));
        assert_eq!(summaries[1].balance, 0);
        assert_eq!(summaries[1].last_booked_on, None);
    }

    #[test]
    fn transactions_are_newest_first_and_filterable_by_account() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some("PL61109010140000071219812874")).unwrap();
        let other = add_account(&conn, "Poduszka", "buffer", None).unwrap();
        insert_transaction(&conn, acc.id, &row("2026-10-01", 20000, 20000), "t").unwrap();
        insert_transaction(&conn, acc.id, &row("2026-10-02", -3629, 16371), "t").unwrap();
        insert_transaction(&conn, other.id, &row("2026-09-30", 100, 100), "t").unwrap();

        let all = list_transactions(&conn, None, 50).unwrap();
        assert_eq!(all.iter().map(|t| t.booked_on.as_str()).collect::<Vec<_>>(), ["2026-10-02", "2026-10-01", "2026-09-30"]);
        let only_main = list_transactions(&conn, Some(acc.id), 50).unwrap();
        assert_eq!(only_main.len(), 2);
        assert_eq!(only_main[0].account_name, "Zwykłe");
        assert_eq!(list_transactions(&conn, None, 1).unwrap().len(), 1);
    }
}

#[cfg(test)]
pub fn count_transactions(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fin_transactions", [], |r| r.get(0)).unwrap()
}
