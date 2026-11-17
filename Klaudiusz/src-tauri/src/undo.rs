//! Wykonuje operację odwrotną zapisaną w dzienniku. Każda operacja działa tylko na rekordzie, który Klaudiusz sam utworzył.

use crate::{alerts, cthx_write, finance, notes};
use serde_json::Value;

pub fn execute(op: &Value) -> Result<(), String> {
    let kind = op["op"].as_str().ok_or("Nieznana operacja w dzienniku")?;
    let id = op["id"].as_i64().unwrap_or(0);
    match kind {
        "delete_item" => {
            let conn = cthx_write::connect_write()?;
            cthx_write::delete_item(&conn, id)
        }
        "restore_item" => {
            let conn = cthx_write::connect_write()?;
            let draft = cthx_write::draft_from_json(&op["draft"])?;
            cthx_write::replace(&conn, id, &draft)
        }
        "set_done" => {
            let conn = cthx_write::connect_write()?;
            let table = op["table"].as_str().ok_or("brak tabeli")?;
            let column = op["column"].as_str().ok_or("brak kolumny")?;
            let date = op["date"].as_str().ok_or("brak daty")?;
            let was = op["was_done"].as_bool().unwrap_or(false);
            cthx_write::write_done_pub(&conn, table, column, id, date, was)
        }
        "reinsert_item" => {
            let conn = cthx_write::connect_write()?;
            let draft = cthx_write::draft_from_json(&op["draft"])?;
            cthx_write::insert_with_id(&conn, id, &draft)
        }
        "delete_alert" => {
            let conn = alerts::open()?;
            alerts::delete(&conn, id)
        }
        "delete_cash" => {
            let conn = finance::connect_write()?;
            finance::delete_manual_cash(&conn, id)
        }
        "delete_note" => notes::delete(id),
        other => Err(format!("Nieznana operacja: {other}")),
    }
}
