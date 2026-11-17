//! Poczta z ZIM-a: liczba nieodczytanych wiadomości w skrzynkach odbiorczych wszystkich kont
//! oraz 5 ostatnich wiadomości. Czytamy zim.sqlite tylko do odczytu.

use crate::paths;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailItem {
    pub id: i64,
    /// nadawca, tak jak zapisał go ZIM
    pub from: String,
    pub subject: String,
    /// konto, na które przyszła wiadomość (alias albo adres)
    pub account: String,
    /// czas odebrania, milisekundy od epoki
    pub received_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSummary {
    pub unread: i64,
    pub latest: Vec<MailItem>,
}

pub fn summary() -> Result<MailSummary, String> {
    let conn = paths::open_read(&paths::zim(), "ZIM")?;
    let err = |e: rusqlite::Error| format!("ZIM: {e}");

    let unread: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages WHERE read = 0 AND folder = 'inbox'", [], |r| r.get(0))
        .map_err(err)?;

    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.from_addr, m.subject, COALESCE(a.alias, a.email), m.date
               FROM messages m JOIN accounts a ON a.id = m.account_id
              WHERE m.folder = 'inbox'
              ORDER BY m.date DESC, m.id DESC
              LIMIT 3",
        )
        .map_err(err)?;
    let latest = stmt
        .query_map([], |r| {
            Ok(MailItem { id: r.get(0)?, from: r.get(1)?, subject: r.get(2)?, account: r.get(3)?, received_at: r.get(4)? })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;

    Ok(MailSummary { unread, latest })
}
