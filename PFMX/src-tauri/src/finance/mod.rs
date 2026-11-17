//! Finanse osobiste: konta, transakcje i import wyciągów. Kwoty są w groszach (liczby całkowite),
//! nigdy w liczbach zmiennoprzecinkowych. Moduł nie zależy od interfejsu ani od poczty.

pub mod ai;
pub mod auto_import;
pub mod import;
pub mod millennium;
pub mod xtb;
pub mod money;

/// Wszystko, co liczy się z nowych transakcji: płatności cykliczne, ich dopasowanie i wypłaty z bankomatu.
/// Wywoływane po każdym imporcie, po starcie i w cyklu w tle.
pub fn refresh_derived(conn: &rusqlite::Connection) -> crate::error::Result<()> {
    crate::db::recurring::detect(conn)?;
    crate::db::cash::sync_atm(conn)?;
    // przelewy własne nie dostają kategorii; wcześniej model mógł im przypisać jedną, więc ją zdejmujemy
    conn.execute(
        &format!(
            "DELETE FROM fin_tx_envelope WHERE transaction_id IN (SELECT transaction_id FROM fin_tx_ai)
               AND transaction_id IN (SELECT t.id FROM fin_transactions t WHERE {})",
            crate::db::finance::OWN_TRANSFER_SQL
        ),
        [],
    )?;
    crate::db::categorize::apply_rules(conn)?;
    crate::db::income::classify(conn)?;
    Ok(())
}
