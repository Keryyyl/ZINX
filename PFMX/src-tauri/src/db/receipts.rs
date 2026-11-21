//! Zdjęcia paragonów przypięte do transakcji. Same pliki leżą w folderze `paragony` w danych aplikacji,
//! a baza pamięta nazwę pliku i nazwę, pod jaką go zapisano. Jeden paragon na transakcję.

use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS fin_receipts (
  transaction_id INTEGER PRIMARY KEY REFERENCES fin_transactions(id) ON DELETE CASCADE,
  file_name TEXT NOT NULL,
  stored_as TEXT NOT NULL,
  added_at INTEGER NOT NULL
);
";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptInfo {
    /// nazwa pliku, pod jaką paragon wybrano
    pub file_name: String,
    /// nazwa pliku w folderze `paragony`
    #[serde(skip)]
    pub stored_as: String,
    pub added_at: i64,
}

pub fn get(conn: &Connection, transaction_id: i64) -> Result<Option<ReceiptInfo>> {
    conn.query_row(
        "SELECT file_name, stored_as, added_at FROM fin_receipts WHERE transaction_id = ?1",
        [transaction_id],
        |r| Ok(ReceiptInfo { file_name: r.get(0)?, stored_as: r.get(1)?, added_at: r.get(2)? }),
    )
    .optional()
    .map_err(Into::into)
}

/// Zapisuje paragon (zastępuje poprzedni). Zwraca nazwę poprzedniego pliku, do usunięcia z dysku.
pub fn put(conn: &Connection, transaction_id: i64, file_name: &str, stored_as: &str, now_ms: i64) -> Result<Option<String>> {
    let previous = get(conn, transaction_id)?.map(|r| r.stored_as);
    conn.execute(
        "INSERT INTO fin_receipts (transaction_id, file_name, stored_as, added_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(transaction_id) DO UPDATE SET file_name = excluded.file_name, stored_as = excluded.stored_as, added_at = excluded.added_at",
        params![transaction_id, file_name, stored_as, now_ms],
    )?;
    Ok(previous.filter(|p| p != stored_as))
}

/// Usuwa paragon z transakcji. Zwraca nazwę pliku do skasowania z dysku (jeśli paragon był).
pub fn remove(conn: &Connection, transaction_id: i64) -> Result<Option<String>> {
    let previous = get(conn, transaction_id)?.map(|r| r.stored_as);
    conn.execute("DELETE FROM fin_receipts WHERE transaction_id = ?1", [transaction_id])?;
    Ok(previous)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::finance::{add_account, insert_transaction};
    use crate::db::open_memory;
    use crate::finance::millennium::Row;

    #[test]
    fn receipt_is_replaced_and_returns_old_file() {
        let conn = open_memory();
        let acc = add_account(&conn, "Zwykłe", "bank", Some("PL61109010140000071219812874")).unwrap().id;
        let row = Row {
            iban: "PL61109010140000071219812874".into(),
            booked_on: "2026-11-15".into(),
            kind: "PŁATNOŚĆ KARTĄ".into(),
            counterparty_account: None,
            counterparty: "Biedronka".into(),
            description: "ZAKUPY".into(),
            amount: -4590,
            currency: "PLN".into(),
            balance_after: 10000,
        };
        insert_transaction(&conn, acc, &row, "t").unwrap();
        let tx: i64 = conn.query_row("SELECT id FROM fin_transactions", [], |r| r.get(0)).unwrap();

        assert!(get(&conn, tx).unwrap().is_none());
        assert_eq!(put(&conn, tx, "paragon.jpg", "1-1.jpg", 1).unwrap(), None);
        let old = put(&conn, tx, "nowy.png", "1-2.png", 2).unwrap();
        assert_eq!(old.as_deref(), Some("1-1.jpg"));
        let info = get(&conn, tx).unwrap().unwrap();
        assert_eq!(info.file_name, "nowy.png");
        assert_eq!(info.stored_as, "1-2.png");

        assert_eq!(remove(&conn, tx).unwrap().as_deref(), Some("1-2.png"));
        assert!(get(&conn, tx).unwrap().is_none());
        assert_eq!(remove(&conn, tx).unwrap(), None);
    }
}
