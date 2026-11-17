//! Import eksportu do finansów: sprawdza saldo, przypisuje wiersze do kont i dodaje tylko nowe transakcje.

use super::millennium;
use super::money::mask_iban;
use crate::db::finance;
use crate::error::{Error, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Nowe transakcje
    pub added: usize,
    /// Już znane (z poprzednich importów albo z tego samego pliku)
    pub skipped: usize,
    /// Nowe transakcje w podziale na konta (nazwa -> liczba)
    pub per_account: BTreeMap<String, usize>,
}

/// Import całego pliku naraz: najpierw sprawdzamy wszystko, dopiero potem zapisujemy w jednej transakcji bazy.
pub fn import_millennium(conn: &Connection, text: &str, file_name: &str) -> Result<ImportReport> {
    let rows = millennium::parse(text)?;
    millennium::verify_chain(&rows)?;

    let accounts = finance::accounts_by_iban(conn)?;
    let unknown: BTreeSet<String> = rows.iter().filter(|r| !accounts.contains_key(&r.iban)).map(|r| mask_iban(&r.iban)).collect();
    if !unknown.is_empty() {
        let list = unknown.into_iter().collect::<Vec<_>>().join(", ");
        return Err(Error(format!("Brak konta dla rachunków: {list}. Dodaj je w ustawieniach finansów, a potem zaimportuj plik ponownie.")));
    }

    let tx = conn.unchecked_transaction()?;
    let (mut added, mut skipped) = (0, 0);
    let mut per_account = BTreeMap::new();
    // plik jest od najnowszego; zapisujemy od najstarszego, żeby nowszy wpis zawsze miał wyższy id
    for row in rows.iter().rev() {
        let (account_id, name) = &accounts[&row.iban];
        if finance::insert_transaction(&tx, *account_id, row, "millennium_csv")? {
            added += 1;
            *per_account.entry(name.clone()).or_insert(0) += 1;
        } else {
            skipped += 1;
        }
    }
    tx.execute(
        "INSERT INTO fin_imports (file_name, imported_at, added, skipped) VALUES (?1, ?2, ?3, ?4)",
        params![file_name, crate::now_ms(), added as i64, skipped as i64],
    )?;
    tx.commit()?;
    // po każdym odczycie szukamy płatności cyklicznych; nowe trafiają na listę od razu
    super::refresh_derived(conn)?;
    Ok(ImportReport { added, skipped, per_account })
}

#[cfg(test)]
mod tests {
    use super::super::millennium::testkit::export;
    use super::*;
    use crate::db::{finance as fin, open_memory};

    const A: &str = "PL61 1090 1014 0000 0712 1981 2874";
    const B: &str = "PL27 1140 2004 0000 3002 0135 5387";

    fn accounts(conn: &Connection) {
        fin::add_account(conn, "Zwykłe", "bank", Some(A)).unwrap();
        fin::add_account(conn, "Oszczędnościowe", "savings", Some(B)).unwrap();
    }

    fn history() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
        vec![
            (A, "2026-10-01", "PRZELEW PRZYCHODZĄCY", "200.00"),
            (A, "2026-10-02", "ZAKUP - FIZ. UŻYCIE KARTY", "-36.29"),
            (B, "2026-10-02", "PRZELEW WEWNĘTRZNY PRZYCHODZĄCY", "1.00"),
            (A, "2026-11-04", "ZAKUP - FIZ. UŻYCIE KARTY", "-5.00"),
            (A, "2026-11-04", "ZAKUP - FIZ. UŻYCIE KARTY", "-5.00"),
        ]
    }

    #[test]
    fn first_import_adds_everything_per_account() {
        let conn = open_memory();
        accounts(&conn);
        let report = import_millennium(&conn, &export(&history()), "historia.csv").unwrap();
        assert_eq!((report.added, report.skipped), (5, 0));
        assert_eq!(report.per_account.get("Zwykłe"), Some(&4));
        assert_eq!(report.per_account.get("Oszczędnościowe"), Some(&1));
        assert_eq!(fin::count_transactions(&conn), 5);
    }

    #[test]
    fn identical_purchases_on_the_same_day_are_both_kept() {
        let conn = open_memory();
        accounts(&conn);
        import_millennium(&conn, &export(&history()), "historia.csv").unwrap();
        let same_day: i64 = conn
            .query_row("SELECT count(*) FROM fin_transactions WHERE booked_on = '2026-11-04' AND amount = -500", [], |r| r.get(0))
            .unwrap();
        assert_eq!(same_day, 2);
    }

    #[test]
    fn reimporting_the_same_file_adds_nothing() {
        let conn = open_memory();
        accounts(&conn);
        let text = export(&history());
        import_millennium(&conn, &text, "a.csv").unwrap();
        let again = import_millennium(&conn, &text, "a.csv").unwrap();
        assert_eq!((again.added, again.skipped), (0, 5));
        assert_eq!(fin::count_transactions(&conn), 5);
    }

    #[test]
    fn overlapping_export_adds_only_the_new_part() {
        let conn = open_memory();
        accounts(&conn);
        import_millennium(&conn, &export(&history()[..3]), "stary.csv").unwrap();
        let mut longer = history();
        longer.push((A, "2026-11-05", "PŁATNOŚĆ BLIK W INTERNECIE", "-29.20"));
        let report = import_millennium(&conn, &export(&longer), "nowy.csv").unwrap();
        // stary plik miał 3 wiersze, nowy 6: 3 już znane, 3 nowe
        assert_eq!((report.added, report.skipped), (3, 3));
        assert_eq!(fin::count_transactions(&conn), 6);
        let logged: i64 = conn.query_row("SELECT count(*) FROM fin_imports", [], |r| r.get(0)).unwrap();
        assert_eq!(logged, 2);
    }

    #[test]
    fn unknown_account_stops_the_whole_import() {
        let conn = open_memory();
        fin::add_account(&conn, "Zwykłe", "bank", Some(A)).unwrap();
        let err = import_millennium(&conn, &export(&history()), "a.csv").unwrap_err();
        assert!(err.0.contains("…5387") && err.0.contains("Dodaj je"), "{}", err.0);
        assert_eq!(fin::count_transactions(&conn), 0, "nic nie zapisano częściowo");
    }

    #[test]
    fn broken_balance_stops_the_import_before_any_write() {
        let conn = open_memory();
        accounts(&conn);
        let text = export(&history()).replacen("\"-36.29\"", "\"-36.30\"", 0).replacen("\"163.71\"", "\"163.72\"", 1);
        assert!(import_millennium(&conn, &text, "a.csv").is_err());
        assert_eq!(fin::count_transactions(&conn), 0);
    }

    #[test]
    fn account_can_be_added_with_spaces_and_is_unique() {
        let conn = open_memory();
        fin::add_account(&conn, "Zwykłe", "bank", Some(A)).unwrap();
        let dup = fin::add_account(&conn, "Drugie", "bank", Some(&A.replace(' ', "").to_lowercase())).unwrap_err();
        assert_eq!(dup.0, "To konto jest już dodane");
        assert!(fin::add_account(&conn, "Zły", "nieznany", None).is_err());
        assert!(fin::add_account(&conn, "  ", "bank", None).is_err());
    }

    /// Prawdziwy plik z eksportu: `ZIM_MILLENNIUM_CSV=/ścieżka/plik.csv cargo test -- --ignored real_export`
    #[test]
    #[ignore]
    fn real_export_imports_and_chains() {
        let path = std::env::var("ZIM_MILLENNIUM_CSV").expect("ustaw ZIM_MILLENNIUM_CSV");
        let text = std::fs::read_to_string(path).unwrap();
        let rows = millennium::parse(&text).unwrap();
        millennium::verify_chain(&rows).unwrap();
        let conn = open_memory();
        let mut ibans: Vec<String> = rows.iter().map(|r| r.iban.clone()).collect();
        ibans.sort();
        ibans.dedup();
        for (i, iban) in ibans.iter().enumerate() {
            fin::add_account(&conn, &format!("Konto {}", i + 1), "bank", Some(iban)).unwrap();
        }
        let report = import_millennium(&conn, &text, "real.csv").unwrap();
        println!("wierszy: {}, dodano: {}, pominięto: {}, konta: {:?}", rows.len(), report.added, report.skipped, report.per_account);
        assert_eq!(report.added, rows.len());
        assert_eq!(import_millennium(&conn, &text, "real.csv").unwrap().added, 0);
    }
}
