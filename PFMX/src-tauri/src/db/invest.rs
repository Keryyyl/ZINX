//! Inwestycje z eksportu XTB: zapis operacji, lista aktualnie posiadanych aktywów (koszt zakupu)
//! i historia ruchów. Bez cen bieżących: wartości rynkowej i zmiany od zakupu nie liczymy.

use crate::error::Result;
use crate::finance::xtb::XtbParse;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Holding {
    pub ticker: String,
    pub instrument: String,
    /// ilość w dziesięciotysięcznych
    pub volume: i64,
    /// koszt zakupu tego, co zostało, w groszach (PLN)
    pub cost: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEvent {
    pub id: i64,
    pub kind: String,
    pub ticker: String,
    pub instrument: String,
    pub volume: i64,
    pub price: i64,
    /// PLN w groszach, ze znakiem z punktu widzenia konta
    pub amount: i64,
    pub occurred_at: i64,
    /// dla kupna i sprzedaży: ile sztuk tego waloru zostało po operacji
    pub left_after: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InvestImportReport {
    /// nowe operacje dopisane do bazy
    pub added: i64,
    /// operacje, które były już w bazie (ponowny import niczego nie dubluje)
    pub existing: i64,
    /// wiersze pominięte, bo nie dało się ich odczytać
    pub skipped: i64,
}

pub fn import(conn: &Connection, parsed: &XtbParse) -> Result<InvestImportReport> {
    let mut added = 0;
    let mut existing = 0;
    for op in &parsed.ops {
        let changed = conn.execute(
            "INSERT OR IGNORE INTO inv_ops (xtb_id, kind, ticker, instrument, position_id, volume, price, amount, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![op.xtb_id, op.kind, op.ticker, op.instrument, op.position_id, op.volume, op.price, op.amount, op.occurred_at],
        )?;
        if changed > 0 {
            added += 1;
        } else {
            existing += 1;
        }
    }
    Ok(InvestImportReport { added, existing, skipped: parsed.skipped as i64 })
}

/// Aktywa: dla każdej pozycji (Position ID) kupione minus sprzedane; koszt zostaje proporcjonalnie.
pub fn holdings(conn: &Connection) -> Result<Vec<Holding>> {
    let mut stmt = conn.prepare(
        "SELECT ticker, instrument, position_id, kind, volume, amount FROM inv_ops
         WHERE kind IN ('buy', 'sell') ORDER BY occurred_at, id",
    )?;
    struct Lot {
        ticker: String,
        instrument: String,
        bought: i64,
        cost: i64,
        sold: i64,
    }
    let mut lots: BTreeMap<String, Lot> = BTreeMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?))
    })?;
    for row in rows {
        let (ticker, instrument, position, kind, volume, amount) = row?;
        let lot = lots.entry(position).or_insert_with(|| Lot { ticker: ticker.clone(), instrument: instrument.clone(), bought: 0, cost: 0, sold: 0 });
        if kind == "buy" {
            lot.bought += volume;
            lot.cost += amount.abs();
            lot.instrument = instrument;
        } else {
            lot.sold += volume;
        }
    }

    // per ticker: suma tego, co zostało, i jego koszt
    let mut by_ticker: BTreeMap<String, Holding> = BTreeMap::new();
    for lot in lots.values() {
        let left = lot.bought - lot.sold;
        if lot.bought <= 0 || left <= 0 {
            continue;
        }
        let cost_left = (lot.cost as i128 * left as i128 / lot.bought as i128) as i64;
        let entry = by_ticker.entry(lot.ticker.clone()).or_insert_with(|| Holding {
            ticker: lot.ticker.clone(),
            instrument: lot.instrument.clone(),
            volume: 0,
            cost: 0,
        });
        entry.volume += left;
        entry.cost += cost_left;
    }
    let mut out: Vec<Holding> = by_ticker.into_values().collect();
    out.sort_by_key(|h| std::cmp::Reverse(h.cost));
    Ok(out)
}

/// Historia od najnowszej. Dla kupna i sprzedaży liczy, ile sztuk waloru zostało po operacji.
pub fn history(conn: &Connection) -> Result<Vec<HistoryEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, ticker, instrument, volume, price, amount, occurred_at FROM inv_ops ORDER BY occurred_at, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(HistoryEvent {
            id: r.get(0)?,
            kind: r.get(1)?,
            ticker: r.get(2)?,
            instrument: r.get(3)?,
            volume: r.get(4)?,
            price: r.get(5)?,
            amount: r.get(6)?,
            occurred_at: r.get(7)?,
            left_after: None,
        })
    })?;
    let mut events: Vec<HistoryEvent> = rows.collect::<std::result::Result<_, _>>()?;

    // stan ilości liczony chronologicznie, osobno dla każdego waloru
    let mut balance: BTreeMap<String, i64> = BTreeMap::new();
    for event in events.iter_mut() {
        match event.kind.as_str() {
            "buy" => {
                let b = balance.entry(event.ticker.clone()).or_insert(0);
                *b += event.volume;
                event.left_after = Some(*b);
            }
            "sell" => {
                let b = balance.entry(event.ticker.clone()).or_insert(0);
                *b -= event.volume;
                event.left_after = Some(*b);
            }
            _ => {}
        }
    }
    events.reverse();
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::finance::xtb::XtbOp;

    fn op(xtb_id: &str, kind: &'static str, ticker: &str, position: &str, volume: i64, price: i64, amount: i64, at: i64) -> XtbOp {
        XtbOp {
            xtb_id: xtb_id.into(),
            kind,
            ticker: ticker.into(),
            instrument: format!("{ticker} name"),
            position_id: position.into(),
            volume,
            price,
            amount,
            occurred_at: at,
        }
    }

    #[test]
    fn import_is_idempotent_and_holdings_follow_position_ids() {
        let conn = db::open_memory();
        let parsed = XtbParse {
            ops: vec![
                op("1", "buy", "SXR8.DE", "P1", 10_000, 6_000_000, -20_000, 1),
                op("2", "buy", "SXR8.DE", "P2", 5_000, 6_100_000, -10_000, 2),
                op("3", "sell", "SXR8.DE", "P1", 4_000, 7_000_000, 28_000, 3),
                op("4", "deposit", "", "", 0, 0, 100_000, 0),
            ],
            skipped: 2,
        };
        let first = import(&conn, &parsed).unwrap();
        assert_eq!(first, InvestImportReport { added: 4, existing: 0, skipped: 2 });
        let again = import(&conn, &parsed).unwrap();
        assert_eq!(again, InvestImportReport { added: 0, existing: 4, skipped: 2 });

        // P1: kupiono 1,0 za 200 zł, sprzedano 0,4 -> zostało 0,6 za 120 zł; P2: 0,5 za 100 zł
        let held = holdings(&conn).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].volume, 11_000);
        assert_eq!(held[0].cost, 22_000);
    }

    #[test]
    fn history_tracks_quantity_after_each_trade() {
        let conn = db::open_memory();
        let parsed = XtbParse {
            ops: vec![
                op("1", "buy", "VUAA.UK", "P1", 20_000, 1_300_000, -26_000, 1),
                op("2", "sell", "VUAA.UK", "P1", 5_000, 1_400_000, 7_000, 2),
            ],
            skipped: 0,
        };
        import(&conn, &parsed).unwrap();
        let events = history(&conn).unwrap();
        assert_eq!(events[0].kind, "sell");
        assert_eq!(events[0].left_after, Some(15_000));
        assert_eq!(events[1].left_after, Some(20_000));
    }
}
