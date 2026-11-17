//! Eksport historii z XTB (plik .xlsx). Czytamy arkusz „Cash Operations”: wpłaty, wypłaty,
//! odsetki, podatek od odsetek oraz kupno i sprzedaż akcji/ETF. Arkusze z pozycjami nie są potrzebne:
//! każda transakcja ma tam swój wpis z `Position ID`, który łączy kupno ze sprzedażą.

use crate::error::{Error, Result};
use calamine::{open_workbook_auto, Data, Reader};
use std::path::Path;

const SHEET: &str = "Cash Operations";

#[derive(Debug, Clone, PartialEq)]
pub struct XtbOp {
    /// ID operacji z XTB; po nim deduplikujemy ponowne importy
    pub xtb_id: String,
    /// deposit, withdrawal, interest, interest_tax, buy, sell
    pub kind: &'static str,
    pub ticker: String,
    pub instrument: String,
    pub position_id: String,
    /// ilość w dziesięciotysięcznych (0,0761 -> 761)
    pub volume: i64,
    /// cena w walucie instrumentu, w dziesięciotysięcznych
    pub price: i64,
    /// kwota w PLN w groszach, ze znakiem z punktu widzenia konta
    pub amount: i64,
    /// czas w milisekundach UTC
    pub occurred_at: i64,
}

#[derive(Debug, Default, PartialEq)]
pub struct XtbParse {
    pub ops: Vec<XtbOp>,
    /// wiersze, których nie umiemy rozpoznać (inny typ albo nieczytelna komenda)
    pub skipped: usize,
}

/// Czas Excela (dni od 1899-12-30, UTC) na milisekundy Unix.
fn excel_to_ms(serial: f64) -> i64 {
    ((serial - 25569.0) * 86_400_000.0).round() as i64
}

fn to_scaled(value: f64) -> i64 {
    (value * 10_000.0).round() as i64
}

fn text(cell: Option<&Data>) -> String {
    match cell {
        Some(Data::String(s)) => s.trim().to_string(),
        Some(Data::Float(f)) => format!("{}", *f as i64),
        Some(Data::Int(i)) => i.to_string(),
        _ => String::new(),
    }
}

fn number(cell: Option<&Data>) -> Option<f64> {
    match cell {
        Some(Data::Float(f)) => Some(*f),
        Some(Data::Int(i)) => Some(*i as f64),
        Some(Data::DateTime(dt)) => Some(dt.as_f64()),
        Some(Data::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Komentarz transakcji: „OPEN BUY 0.0761 @ 614.00” albo „CLOSE BUY 0.0761/0.1382 @ 709.40”.
/// Zwraca (ilość, cenę) w dziesięciotysięcznych; przy zamknięciu pierwsza liczba to sprzedana ilość.
fn parse_trade_comment(comment: &str) -> Option<(i64, i64)> {
    let rest = comment.strip_prefix("OPEN BUY ").or_else(|| comment.strip_prefix("CLOSE BUY "))?;
    let (quantity, price) = rest.split_once(" @ ")?;
    let volume: f64 = quantity.split('/').next()?.trim().parse().ok()?;
    let price: f64 = price.trim().parse().ok()?;
    Some((to_scaled(volume), to_scaled(price)))
}

pub fn parse_file(path: &Path) -> Result<XtbParse> {
    let mut book = open_workbook_auto(path).map_err(|e| Error(format!("Nie udało się otworzyć pliku XTB: {e}")))?;
    let range = book.worksheet_range(SHEET).map_err(|_| Error::from("W pliku brak arkusza „Cash Operations”. To nie jest eksport historii z XTB."))?;
    let rows: Vec<Vec<Data>> = range.rows().map(|r| r.to_vec()).collect();

    let header_at = rows
        .iter()
        .position(|r| matches!(r.first(), Some(Data::String(s)) if s.trim() == "Type"))
        .ok_or_else(|| Error::from("Nie znaleziono nagłówka operacji w arkuszu XTB"))?;
    let header = &rows[header_at];
    let column = |name: &str| -> Result<usize> {
        header
            .iter()
            .position(|c| matches!(c, Data::String(s) if s.trim() == name))
            .ok_or_else(|| Error(format!("W eksporcie XTB brakuje kolumny „{name}”")))
    };
    let (c_type, c_instrument, c_ticker, c_time, c_amount, c_id, c_comment, c_position) = (
        column("Type")?,
        column("Instrument")?,
        column("Ticker")?,
        column("Time")?,
        column("Amount")?,
        column("ID")?,
        column("Comment")?,
        column("Position ID")?,
    );

    let mut parsed = XtbParse::default();
    for row in &rows[header_at + 1..] {
        let kind_text = text(row.get(c_type));
        if kind_text.is_empty() || kind_text == "Total" {
            continue;
        }
        let kind: Option<&'static str> = match kind_text.as_str() {
            "Deposit" => Some("deposit"),
            "Withdrawal" => Some("withdrawal"),
            "Free funds interest" => Some("interest"),
            "Free funds interest tax" => Some("interest_tax"),
            "Stock purchase" => Some("buy"),
            "Stock sell" => Some("sell"),
            _ => None,
        };
        let (Some(kind), Some(amount), Some(time)) = (kind, number(row.get(c_amount)), number(row.get(c_time))) else {
            parsed.skipped += 1;
            continue;
        };
        let xtb_id = text(row.get(c_id));
        if xtb_id.is_empty() {
            parsed.skipped += 1;
            continue;
        }
        let (volume, price) = if kind == "buy" || kind == "sell" {
            match parse_trade_comment(&text(row.get(c_comment))) {
                Some(v) => v,
                None => {
                    parsed.skipped += 1;
                    continue;
                }
            }
        } else {
            (0, 0)
        };
        parsed.ops.push(XtbOp {
            xtb_id,
            kind,
            ticker: text(row.get(c_ticker)),
            instrument: text(row.get(c_instrument)),
            position_id: text(row.get(c_position)),
            volume,
            price,
            amount: (amount * 100.0).round() as i64,
            occurred_at: excel_to_ms(time),
        });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_comments() {
        assert_eq!(parse_trade_comment("OPEN BUY 0.0761 @ 614.00"), Some((761, 6_140_000)));
        assert_eq!(parse_trade_comment("CLOSE BUY 0.0761/0.1382 @ 709.40"), Some((761, 7_094_000)));
        assert_eq!(parse_trade_comment("Withdrawal from 1"), None);
    }

    /// Prawdziwy eksport: `XTB_TEST_FILE=/ścieżka/plik.xlsx cargo test -- --ignored xtb_real_file`
    #[test]
    #[ignore]
    fn xtb_real_file() {
        let Ok(path) = std::env::var("XTB_TEST_FILE") else { return };
        let parsed = parse_file(Path::new(&path)).unwrap();
        let count = |k: &str| parsed.ops.iter().filter(|o| o.kind == k).count();
        println!("ops={} skipped={} deposit={} withdrawal={} interest={} tax={} buy={} sell={}", parsed.ops.len(), parsed.skipped, count("deposit"), count("withdrawal"), count("interest"), count("interest_tax"), count("buy"), count("sell"));
        for op in parsed.ops.iter().filter(|o| o.kind == "buy" || o.kind == "sell").take(3) {
            println!("{op:?}");
        }
    }

    #[test]
    fn excel_serial_to_ms() {
        // 1970-01-01 00:00 UTC
        assert_eq!(excel_to_ms(25569.0), 0);
        assert_eq!(excel_to_ms(25570.5), 86_400_000 + 43_200_000);
    }
}
