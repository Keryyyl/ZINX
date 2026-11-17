//! Eksport historii z Millennium (CSV, UTF-8 z BOM). Jeden plik zawiera wszystkie konta naraz,
//! a kolumna „Saldo” pozwala sprawdzić, czy import jest kompletny i poprawny.

use super::money::{format_grosze, mask_iban, parse_grosze};
use crate::error::{Error, Result};
use std::collections::HashMap;

pub const HEADER: [&str; 11] = [
    "Numer rachunku/karty",
    "Data transakcji",
    "Data rozliczenia",
    "Rodzaj transakcji",
    "Na konto/Z konta",
    "Odbiorca/Zleceniodawca",
    "Opis",
    "Obciążenia",
    "Uznania",
    "Saldo",
    "Waluta",
];

/// Jedna transakcja z eksportu. Kwota ma znak: ujemna = wypłata/obciążenie, dodatnia = wpływ.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub iban: String,
    /// Data transakcji, RRRR-MM-DD
    pub booked_on: String,
    pub kind: String,
    pub counterparty_account: Option<String>,
    pub counterparty: String,
    pub description: String,
    /// w groszach
    pub amount: i64,
    pub currency: String,
    /// Saldo konta po tej transakcji, w groszach
    pub balance_after: i64,
}

pub fn parse(text: &str) -> Result<Vec<Row>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new().has_headers(true).from_reader(text.as_bytes());
    let headers = reader.headers().map_err(|e| Error(format!("Nie udało się odczytać nagłówka: {e}")))?;
    if headers.iter().collect::<Vec<_>>() != HEADER {
        return Err("To nie wygląda na eksport historii z Millennium (nieoczekiwany nagłówek)".into());
    }
    let mut rows = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let line = i + 2;
        let record = record.map_err(|e| Error(format!("Wiersz {line}: {e}")))?;
        rows.push(parse_row(&record).map_err(|e| Error(format!("Wiersz {line}: {}", e.0)))?);
    }
    Ok(rows)
}

fn valid_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn parse_row(r: &csv::StringRecord) -> Result<Row> {
    if r.len() != HEADER.len() {
        return Err("zła liczba kolumn".into());
    }
    let f = |i: usize| r.get(i).unwrap_or("").trim();

    let iban = f(0).chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if iban.is_empty() {
        return Err("brak numeru rachunku".into());
    }
    let booked_on = f(1);
    if !valid_date(booked_on) {
        return Err(format!("nieprawidłowa data: {booked_on}").into());
    }
    // wypłaty są w kolumnie „Obciążenia” (już z minusem), wpływy w „Uznania”; znak bierzemy z kolumny
    let (debit, credit) = (f(7), f(8));
    let amount = match (debit.is_empty(), credit.is_empty()) {
        (false, true) => -parse_grosze(debit)?.abs(),
        (true, false) => parse_grosze(credit)?.abs(),
        _ => return Err("kwota musi być w jednej z kolumn: obciążenia albo uznania".into()),
    };
    let currency = f(10);
    if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(format!("nieprawidłowa waluta: {currency}").into());
    }
    let counterparty_account: String = f(4).chars().filter(|c| !c.is_whitespace()).collect();
    Ok(Row {
        iban,
        booked_on: booked_on.to_string(),
        kind: f(3).to_string(),
        counterparty_account: (!counterparty_account.is_empty()).then_some(counterparty_account),
        counterparty: f(5).to_string(),
        description: f(6).to_string(),
        amount,
        currency: currency.to_string(),
        balance_after: parse_grosze(f(9))?,
    })
}

/// Saldo musi się zgadzać krok po kroku: saldo po nowszej transakcji = saldo po starszej + jej kwota.
/// Plik jest w kolejności od najnowszej, więc każdy wiersz porównujemy z poprzednim wierszem tego samego konta.
pub fn verify_chain(rows: &[Row]) -> Result<()> {
    let mut newer: HashMap<&str, &Row> = HashMap::new();
    for row in rows {
        if let Some(n) = newer.get(row.iban.as_str()) {
            if n.balance_after != row.balance_after + n.amount {
                return Err(Error(format!(
                    "Saldo się nie zgadza na koncie {}: po transakcji z {} jest {}, a powinno być {}. \
                     Plik mógł być uszkodzony albo niepełny.",
                    mask_iban(&row.iban),
                    n.booked_on,
                    format_grosze(n.balance_after),
                    format_grosze(row.balance_after + n.amount),
                )));
            }
        }
        newer.insert(&row.iban, row);
    }
    Ok(())
}

/// Eksport w układzie pliku banku: ten sam nagłówek i kolumny, wszystkie pola w cudzysłowach,
/// najnowsze wiersze na górze. Plik da się ponownie zaimportować bez zmian.
pub fn export(rows: &[Row]) -> String {
    use super::money::format_grosze;
    let quote = |field: &str| format!("\"{}\"", field.replace('"', "\"\""));
    let header = HEADER.iter().map(|h| quote(h)).collect::<Vec<_>>().join(",");
    let lines: Vec<String> = rows
        .iter()
        .map(|r| {
            let (debit, credit) = if r.amount < 0 { (format_grosze(r.amount), String::new()) } else { (String::new(), format_grosze(r.amount)) };
            let balance = format_grosze(r.balance_after);
            let fields = [
                r.iban.as_str(),
                r.booked_on.as_str(),
                r.booked_on.as_str(),
                r.kind.as_str(),
                r.counterparty_account.as_deref().unwrap_or(""),
                r.counterparty.as_str(),
                r.description.as_str(),
                debit.as_str(),
                credit.as_str(),
                balance.as_str(),
                r.currency.as_str(),
            ];
            fields.iter().map(|f| quote(f)).collect::<Vec<_>>().join(",")
        })
        .collect();
    format!("\u{feff}{header}\n{}\n", lines.join("\n"))
}

/// Generator eksportów do testów: tworzy plik w takim samym formacie z listy transakcji (od najstarszej).
#[cfg(test)]
pub mod testkit {
    use super::super::money::{format_grosze, parse_grosze};
    use super::HEADER;

    /// (rachunek, data, rodzaj, kwota jako tekst) od najstarszej; saldo liczy się od zera dla każdego konta.
    pub fn export(items: &[(&str, &str, &str, &str)]) -> String {
        let mut balances: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
        let mut lines = Vec::new();
        for (iban, date, kind, amount) in items {
            let value = parse_grosze(amount).unwrap();
            let balance = balances.entry(iban).or_insert(0);
            *balance += value;
            let (debit, credit) = if value < 0 { (format_grosze(value), String::new()) } else { (String::new(), format_grosze(value)) };
            lines.push(format!(
                "\"{iban}\",\"{date}\",\"{date}\",\"{kind}\",\"\",\"Odbiorca testowy\",\"Opis {date}\",\"{debit}\",\"{credit}\",\"{}\",\"PLN\"",
                format_grosze(*balance)
            ));
        }
        let header = HEADER.iter().map(|h| format!("\"{h}\"")).collect::<Vec<_>>().join(",");
        // w pliku banku najnowsze wiersze są na górze
        lines.reverse();
        format!("\u{feff}{header}\n{}\n", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::export;
    use super::*;

    const A: &str = "PL61 1090 1014 0000 0712 1981 2874";
    const B: &str = "PL27 1140 2004 0000 3002 0135 5387";

    fn sample() -> String {
        export(&[
            (A, "2026-10-01", "PRZELEW PRZYCHODZĄCY", "200.00"),
            (A, "2026-10-02", "ZAKUP - FIZ. UŻYCIE KARTY", "-36.29"),
            (B, "2026-10-02", "PRZELEW WEWNĘTRZNY PRZYCHODZĄCY", "1.00"),
            (A, "2026-11-03", "PŁATNOŚĆ BLIK W INTERNECIE", "-29.20"),
            (A, "2026-11-04", "ZAKUP - FIZ. UŻYCIE KARTY", "-5.00"),
            (A, "2026-11-04", "ZAKUP - FIZ. UŻYCIE KARTY", "-5.00"),
        ])
    }

    #[test]
    fn parses_all_accounts_with_signed_amounts() {
        let rows = parse(&sample()).unwrap();
        assert_eq!(rows.len(), 6);
        // najnowszy wiersz jest pierwszy
        assert_eq!((rows[0].booked_on.as_str(), rows[0].amount), ("2026-11-04", -500));
        assert_eq!(rows[0].iban, "PL61109010140000071219812874");
        assert_eq!(rows[0].currency, "PLN");
        let income = rows.iter().find(|r| r.iban.starts_with("PL27")).unwrap();
        assert_eq!((income.amount, income.balance_after), (100, 100));
        assert_eq!(rows.last().unwrap().counterparty_account, None);
    }

    #[test]
    fn chain_is_consistent_for_generated_file() {
        verify_chain(&parse(&sample()).unwrap()).unwrap();
    }

    #[test]
    fn chain_detects_a_changed_balance() {
        let text = sample().replacen("\"134.51\"", "\"134.52\"", 1);
        let err = verify_chain(&parse(&text).unwrap()).unwrap_err();
        assert!(err.0.contains("Saldo się nie zgadza") && err.0.contains("…2874"), "{}", err.0);
    }

    #[test]
    fn rejects_other_files_and_bad_rows() {
        assert!(parse("a;b;c\n1;2;3").unwrap_err().0.contains("nieoczekiwany nagłówek"));
        let text = sample().replacen("\"-5.00\"", "\"-5,00\"", 0).replacen("\"PLN\"", "\"ZŁ\"", 1);
        assert!(parse(&text).unwrap_err().0.contains("Wiersz"));
    }

    #[test]
    fn rejects_rows_with_both_or_no_amount() {
        let both = sample().replacen("\"\",\"200.00\"", "\"-1.00\",\"200.00\"", 1);
        assert!(parse(&both).is_err());
        let none = sample().replacen("\"200.00\"", "\"\"", 1);
        assert!(parse(&none).is_err());
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn exported_file_imports_back_to_the_same_rows() {
        let rows = vec![
            Row { iban: "PL61109010140000071219812874".into(), booked_on: "2026-11-04".into(), kind: "ZAKUP - FIZ. UŻYCIE KARTY".into(), counterparty_account: None, counterparty: "Sklep \"Nr 1\"".into(), description: "Opis, z przecinkiem".into(), amount: -3629, currency: "PLN".into(), balance_after: 16371 },
            Row { iban: "PL61109010140000071219812874".into(), booked_on: "2026-11-03".into(), kind: "PRZELEW".into(), counterparty_account: Some("PL27114020040000300201355387".into()), counterparty: "Ja".into(), description: "Własny".into(), amount: 20000, currency: "PLN".into(), balance_after: 20000 },
        ];
        let text = export(&rows);
        assert!(text.starts_with('\u{feff}'));
        let back = parse(&text).unwrap();
        assert_eq!(back, rows);
        verify_chain(&back).unwrap();
    }
}
