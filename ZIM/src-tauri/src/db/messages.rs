use super::{attachments, placeholders, tags};
use crate::error::Result;
use crate::priority::{INBOX_RANKS, NO_PRIORITY_RANK};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};

const FOLDERS: [&str; 4] = ["inbox", "archive", "spam", "trash"];
const VIEWS: [&str; 6] = ["inbox", "archive", "spam", "trash", "starred", "all"];
const LIMIT: i64 = 1000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageQuery {
    /// None = wszystkie konta
    pub account_id: Option<i64>,
    pub view: String,
    /// Tylko wiadomości z tym tagiem (z odebranych i archiwum), niezależnie od `view`
    #[serde(default)]
    pub tag_id: Option<i64>,
    /// Cała korespondencja z tym adresem (od niego i do niego, wszystkie foldery poza usuniętymi)
    #[serde(default)]
    pub correspondent: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub account_id: Option<i64>,
    pub text: String,
}

/// Wiersz listy wiadomości (bez treści)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub id: i64,
    pub account_id: i64,
    pub from: String,
    pub subject: String,
    pub date: i64,
    pub snippet: String,
    pub read: bool,
    pub starred: bool,
    pub folder: String,
    pub priority: String,
    /// Ma załączniki inne niż obrazki osadzone w treści (ikona spinacza)
    pub has_attachments: bool,
    pub tags: Vec<tags::Tag>,
}

/// Pełna wiadomość do czytnika
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub row: MessageRow,
    pub to: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub attachments: Vec<attachments::Attachment>,
    /// Skrót od asystenta AI (punkty rozdzielone \n); None = jeszcze nie powstał
    pub summary: Option<String>,
}

/// Odebrane: Pilne, potem Luźne, potem bez tagu priorytetu (np. jeszcze nieprzetworzone), na końcu Zachować; w grupie od najnowszych.
fn inbox_order() -> String {
    let cases: String = INBOX_RANKS.iter().map(|(key, rank)| format!("WHEN '{key}' THEN {rank}")).collect::<Vec<_>>().join(" ");
    format!(
        "(SELECT COALESCE(MIN(CASE t.system_key {cases} END), {NO_PRIORITY_RANK})
          FROM message_tags mt JOIN tags t ON t.id = mt.tag_id
          WHERE mt.message_id = messages.id), date DESC"
    )
}

pub fn is_folder(value: &str) -> bool {
    FOLDERS.contains(&value)
}

const ROW_COLUMNS: &str = "id, account_id, from_addr, subject, date, snippet, read, starred, folder, priority,
  EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = messages.id AND a.inline = 0)";

fn raw_row(r: &rusqlite::Row) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        from: r.get(2)?,
        subject: r.get(3)?,
        date: r.get(4)?,
        snippet: r.get(5)?,
        read: r.get::<_, i64>(6)? == 1,
        starred: r.get::<_, i64>(7)? == 1,
        folder: r.get(8)?,
        priority: r.get(9)?,
        has_attachments: r.get::<_, i64>(10)? == 1,
        tags: Vec::new(),
    })
}

fn with_tags(conn: &Connection, mut rows: Vec<MessageRow>) -> Result<Vec<MessageRow>> {
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut by_message = tags::by_message(conn, &ids)?;
    for row in &mut rows {
        row.tags = by_message.remove(&row.id).unwrap_or_default();
    }
    Ok(rows)
}

fn query_rows(conn: &Connection, sql: &str, params: Vec<Value>) -> Result<Vec<MessageRow>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params_from_iter(params), raw_row)?;
    with_tags(conn, rows.collect::<std::result::Result<_, _>>()?)
}

pub fn list(conn: &Connection, query: &MessageQuery) -> Result<Vec<MessageRow>> {
    let view = if VIEWS.contains(&query.view.as_str()) { query.view.as_str() } else { "inbox" };
    let correspondent = query.correspondent.as_deref().map(|c| c.trim().to_lowercase()).filter(|c| !c.is_empty());
    let mut params = vec![query.account_id.map_or(Value::Null, Value::Integer)];
    let date_desc = "date DESC".to_string();
    // instr zamiast LIKE: znaki % i _ w adresie nie mogą działać jak wzorzec
    let (filter, order) = if let Some(c) = correspondent {
        params.push(Value::Text(c));
        (
            "folder != 'deleted' AND (instr(lower(from_addr), ?2) > 0 OR instr(lower(to_addr), ?2) > 0)".to_string(),
            date_desc,
        )
    } else if let Some(tag_id) = query.tag_id {
        params.push(Value::Integer(tag_id));
        (
            "folder IN ('inbox', 'archive') AND EXISTS (SELECT 1 FROM message_tags mt WHERE mt.message_id = messages.id AND mt.tag_id = ?2)"
                .to_string(),
            date_desc,
        )
    } else {
        match view {
            "starred" => ("starred = 1 AND folder IN ('inbox', 'archive')".to_string(), date_desc),
            "all" => ("folder IN ('inbox', 'archive')".to_string(), date_desc),
            other => {
                params.push(Value::Text(other.to_string()));
                ("folder = ?2".to_string(), if other == "inbox" { inbox_order() } else { date_desc })
            }
        }
    };
    let sql = format!(
        "SELECT {ROW_COLUMNS} FROM messages WHERE {filter} AND (?1 IS NULL OR account_id = ?1) ORDER BY {order} LIMIT {LIMIT}"
    );
    query_rows(conn, &sql, params)
}

/// Zapytanie użytkownika -> FTS5: każde słowo jako prefiks, wszystkie muszą wystąpić ("fakt" znajdzie "faktura").
pub fn to_fts_query(text: &str) -> Option<String> {
    let words: Vec<String> =
        text.split_whitespace().map(|w| w.replace('"', "")).filter(|w| !w.is_empty()).take(10).collect();
    if words.is_empty() {
        None
    } else {
        Some(words.iter().map(|w| format!("\"{w}\"*")).collect::<Vec<_>>().join(" "))
    }
}

/// Szukanie w temacie, nadawcy i treści, we wszystkich folderach (poza trwale usuniętymi).
pub fn search(conn: &Connection, query: &SearchQuery) -> Result<Vec<MessageRow>> {
    let Some(m) = to_fts_query(&query.text) else { return Ok(Vec::new()) };
    let sql = format!(
        "SELECT {ROW_COLUMNS} FROM messages
         WHERE id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?1)
           AND folder != 'deleted' AND (?2 IS NULL OR account_id = ?2)
         ORDER BY date DESC LIMIT {LIMIT}"
    );
    query_rows(conn, &sql, vec![Value::Text(m), query.account_id.map_or(Value::Null, Value::Integer)])
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<MessageDetail>> {
    let sql = format!(
        "SELECT {ROW_COLUMNS}, to_addr, body_text, body_html, summary FROM messages WHERE id = ? AND folder != 'deleted'"
    );
    let found = conn.query_row(&sql, [id], |r| Ok((raw_row(r)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?)));
    let (mut row, to, body_text, body_html, summary): (MessageRow, String, String, Option<String>, Option<String>) = match found {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let attachments = attachments::list(conn, id)?;
    row.tags = tags::by_message(conn, &[id])?.remove(&id).unwrap_or_default();
    row.has_attachments = attachments.iter().any(|a| !a.inline);
    Ok(Some(MessageDetail { row, to, body_text, body_html, attachments, summary }))
}

/// Duże zaznaczenia dzielimy na paczki: SQLite ma limit parametrów w zapytaniu.
fn for_chunks(ids: &[i64], mut f: impl FnMut(&[i64], &str) -> Result<()>) -> Result<()> {
    for chunk in ids.chunks(500) {
        f(chunk, &placeholders(chunk.len()))?;
    }
    Ok(())
}

fn update_flag(conn: &Connection, column: &str, ids: &[i64], value: bool) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for_chunks(ids, |chunk, ph| {
        let mut p: Vec<Value> = vec![Value::Integer(value as i64)];
        p.extend(chunk.iter().map(|&i| Value::Integer(i)));
        tx.execute(&format!("UPDATE messages SET {column} = ? WHERE id IN ({ph})"), params_from_iter(p))?;
        Ok(())
    })?;
    tx.commit()?;
    Ok(())
}

pub fn set_read(conn: &Connection, ids: &[i64], read: bool) -> Result<()> {
    update_flag(conn, "read", ids, read)
}

pub fn set_starred(conn: &Connection, ids: &[i64], starred: bool) -> Result<()> {
    update_flag(conn, "starred", ids, starred)
}

/// Archiwizacja, kosz i przywracanie do odebranych. Spam ma osobne funkcje, bo blokuje nadawcę.
pub fn move_to_folder(conn: &Connection, ids: &[i64], folder: &str) -> Result<()> {
    if !is_folder(folder) {
        return Err("Nieznany folder".into());
    }
    let tx = conn.unchecked_transaction()?;
    for_chunks(ids, |chunk, ph| {
        let mut p: Vec<Value> = vec![Value::Text(folder.to_string())];
        p.extend(chunk.iter().map(|&i| Value::Integer(i)));
        tx.execute(&format!("UPDATE messages SET folder = ? WHERE id IN ({ph}) AND folder != 'deleted'"), params_from_iter(p))?;
        Ok(())
    })?;
    tx.commit()?;
    Ok(())
}

fn senders_of(conn: &Connection, ids: &[i64]) -> Result<Vec<String>> {
    let mut senders: Vec<String> = Vec::new();
    for_chunks(ids, |chunk, ph| {
        let mut stmt = conn.prepare(&format!("SELECT DISTINCT from_email FROM messages WHERE id IN ({ph}) AND from_email IS NOT NULL"))?;
        for email in stmt.query_map(params_from_iter(chunk), |r| r.get::<_, String>(0))? {
            let email = email?;
            if !senders.contains(&email) {
                senders.push(email);
            }
        }
        Ok(())
    })?;
    Ok(senders)
}

/// Blokuje nadawców zaznaczonych wiadomości: wszystkie ich wiadomości (ze wszystkich kont)
/// idą do spamu, a nowe trafią tam już przy synchronizacji. Zwraca liczbę zablokowanych adresów.
pub fn mark_spam(conn: &Connection, ids: &[i64], now_ms: i64) -> Result<usize> {
    let senders = senders_of(conn, ids)?;
    for email in &senders {
        conn.execute("INSERT OR IGNORE INTO spam_senders (email, created_at) VALUES (?, ?)", params![email, now_ms])?;
        conn.execute("UPDATE messages SET folder = 'spam' WHERE from_email = ? AND folder IN ('inbox', 'archive')", [email])?;
    }
    // wiadomości bez rozpoznanego adresu też przenosimy, choć nadawcy nie da się zablokować
    move_to_folder(conn, ids, "spam")?;
    Ok(senders.len())
}

/// Odblokowuje nadawców i wraca ich wiadomości ze spamu do odebranych.
pub fn mark_not_spam(conn: &Connection, ids: &[i64]) -> Result<()> {
    for email in senders_of(conn, ids)? {
        conn.execute("DELETE FROM spam_senders WHERE email = ?", [&email])?;
        conn.execute("UPDATE messages SET folder = 'inbox' WHERE from_email = ? AND folder = 'spam'", [&email])?;
    }
    move_to_folder(conn, ids, "inbox")
}

/// Tylko z kosza. Wiersz zostaje jako 'deleted' (niewidoczny), dopóki synchronizacja
/// nie usunie wiadomości z serwera; wiadomości spoza serwera znikają od razu.
pub fn delete_forever(conn: &Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for_chunks(ids, |chunk, ph| {
        tx.execute(&format!("DELETE FROM messages WHERE id IN ({ph}) AND folder = 'trash' AND mailbox IS NULL"), params_from_iter(chunk))?;
        tx.execute(&format!("UPDATE messages SET folder = 'deleted' WHERE id IN ({ph}) AND folder = 'trash'"), params_from_iter(chunk))?;
        Ok(())
    })?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_memory, tags as tag_db};

    fn setup() -> Connection {
        let db = open_memory();
        db.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('me@x.pl', 'imap.x.pl', 'me')", []).unwrap();
        db
    }

    fn add(db: &Connection, uid: i64, from: &str, subject: &str, date: i64, folder: &str) -> i64 {
        db.execute(
            "INSERT INTO messages (account_id, uid, from_addr, from_email, to_addr, subject, date, folder, body_text, mailbox, server_uid)
             VALUES (1, ?1, ?2, ?3, 'me@x.pl', ?4, ?5, ?6, 'Treść faktury', 'INBOX', ?1)",
            params![uid, from, crate::address::extract_email(from), subject, date, folder],
        )
        .unwrap();
        db.last_insert_rowid()
    }

    fn q(view: &str) -> MessageQuery {
        MessageQuery { account_id: None, view: view.into(), tag_id: None, correspondent: None }
    }

    fn system(db: &Connection, key: &str) -> i64 {
        tag_db::list(db).unwrap().into_iter().find(|t| t.system.as_deref() == Some(key)).unwrap().id
    }

    #[test]
    fn inbox_is_ordered_by_priority_then_date() {
        let db = setup();
        let keep = add(&db, 1, "a@x.pl", "zachować", 400, "inbox");
        let none = add(&db, 2, "b@x.pl", "bez tagu", 300, "inbox");
        let loose = add(&db, 3, "c@x.pl", "luźne", 200, "inbox");
        let urgent = add(&db, 4, "d@x.pl", "pilne", 100, "inbox");
        for (id, key) in [(keep, "keep"), (loose, "loose"), (urgent, "urgent")] {
            tag_db::set_message_tag(&db, &[id], system(&db, key), true).unwrap();
        }
        let order: Vec<i64> = list(&db, &q("inbox")).unwrap().iter().map(|m| m.id).collect();
        assert_eq!(order, [urgent, loose, none, keep]);
    }

    #[test]
    fn folders_starred_and_account_filter() {
        let db = setup();
        let a = add(&db, 1, "a@x.pl", "a", 100, "inbox");
        add(&db, 2, "b@x.pl", "b", 200, "archive");
        add(&db, 3, "c@x.pl", "c", 300, "trash");
        set_starred(&db, &[a], true).unwrap();
        assert_eq!(list(&db, &q("inbox")).unwrap().len(), 1);
        assert_eq!(list(&db, &q("all")).unwrap().len(), 2);
        assert_eq!(list(&db, &q("starred")).unwrap()[0].id, a);
        assert!(list(&db, &MessageQuery { account_id: Some(2), ..q("all") }).unwrap().is_empty());
        assert_eq!(list(&db, &q("nieznany")).unwrap().len(), 1);
    }

    #[test]
    fn search_finds_prefixes_and_ignores_deleted() {
        let db = setup();
        add(&db, 1, "a@x.pl", "Faktura za prąd", 100, "inbox");
        let gone = add(&db, 2, "b@x.pl", "Faktura stara", 200, "inbox");
        db.execute("UPDATE messages SET folder = 'deleted' WHERE id = ?", [gone]).unwrap();
        let found = search(&db, &SearchQuery { account_id: None, text: "fakt prą".into() }).unwrap();
        assert_eq!(found.len(), 1);
        assert!(search(&db, &SearchQuery { account_id: None, text: "   ".into() }).unwrap().is_empty());
        assert!(search(&db, &SearchQuery { account_id: None, text: "\"*".into() }).is_ok());
    }

    #[test]
    fn spam_blocks_sender_everywhere_and_can_be_undone() {
        let db = setup();
        let a = add(&db, 1, "Spamer <s@x.pl>", "1", 100, "inbox");
        let b = add(&db, 2, "s@x.pl", "2", 200, "archive");
        let other = add(&db, 3, "inny@x.pl", "3", 300, "inbox");
        assert_eq!(mark_spam(&db, &[a], 1).unwrap(), 1);
        let spam: Vec<i64> = list(&db, &q("spam")).unwrap().iter().map(|m| m.id).collect();
        assert_eq!(spam.len(), 2);
        assert!(spam.contains(&b));
        assert_eq!(list(&db, &q("inbox")).unwrap()[0].id, other);
        mark_not_spam(&db, &[a]).unwrap();
        // ze spamu wracają wszystkie wiadomości tego nadawcy, także ta z archiwum
        assert_eq!(list(&db, &q("inbox")).unwrap().len(), 3);
        let blocked: i64 = db.query_row("SELECT count(*) FROM spam_senders", [], |r| r.get(0)).unwrap();
        assert_eq!(blocked, 0);
    }

    #[test]
    fn delete_forever_only_from_trash() {
        let db = setup();
        let inbox = add(&db, 1, "a@x.pl", "a", 100, "inbox");
        let on_server = add(&db, 2, "b@x.pl", "b", 200, "trash");
        let local = add(&db, 3, "c@x.pl", "c", 300, "trash");
        db.execute("UPDATE messages SET mailbox = NULL WHERE id = ?", [local]).unwrap();
        delete_forever(&db, &[inbox, on_server, local]).unwrap();
        assert!(get(&db, inbox).unwrap().is_some());
        assert!(get(&db, on_server).unwrap().is_none());
        let exists: bool = db.query_row("SELECT EXISTS (SELECT 1 FROM messages WHERE id = ?)", [on_server], |r| r.get(0)).unwrap();
        assert!(exists);
        let exists: bool = db.query_row("SELECT EXISTS (SELECT 1 FROM messages WHERE id = ?)", [local], |r| r.get(0)).unwrap();
        assert!(!exists);
    }

    #[test]
    fn get_returns_detail_with_tags() {
        let db = setup();
        let id = add(&db, 1, "a@x.pl", "temat", 100, "inbox");
        tag_db::set_message_tag(&db, &[id], system(&db, "loose"), true).unwrap();
        db.execute("INSERT INTO attachments (message_id, part, filename, mime, size, inline) VALUES (?, '2', 'a.pdf', 'application/pdf', 10, 0)", [id]).unwrap();
        let m = get(&db, id).unwrap().unwrap();
        assert_eq!((m.row.tags.len(), m.row.has_attachments, m.attachments.len()), (1, true, 1));
        assert_eq!(m.body_text, "Treść faktury");
        assert!(get(&db, 999).unwrap().is_none());
        assert!(move_to_folder(&db, &[id], "deleted").is_err());
    }

    #[test]
    fn json_shape_matches_the_ui_contract() {
        let db = setup();
        let id = add(&db, 1, "a@x.pl", "t", 100, "inbox");
        let json = serde_json::to_value(get(&db, id).unwrap().unwrap()).unwrap();
        for key in ["id", "accountId", "from", "to", "subject", "date", "snippet", "bodyText", "bodyHtml", "read", "starred", "folder", "priority", "hasAttachments", "tags", "attachments", "summary"] {
            assert!(json.get(key).is_some(), "brak pola {key}");
        }
    }
}
