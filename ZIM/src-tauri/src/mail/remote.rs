//! Wypychanie zmian z ZIM na serwer. Stan docelowy to kolumny folder/read/starred, a mailbox/server_uid/
//! server_seen/server_flagged opisują, co jest na serwerze. Każda różnica między nimi jest zmianą do wykonania;
//! po sukcesie kolumny serwera się wyrównują, więc nieudana operacja zostanie powtórzona przy kolejnej synchronizacji.

use super::folders::{folder_of, path_of, ServerFolders};
use super::session::{Flag, ImapSession};
use crate::db::Db;
use crate::error::Result;
use rusqlite::{params, Connection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Przerwanie synchronizacji (np. usunięcie konta); sprawdzane między krokami.
pub type Cancel = Arc<AtomicBool>;

pub fn is_cancelled(cancel: &Option<Cancel>) -> bool {
    cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
}

/// Zapisuje, gdzie wiadomość jest na serwerze. Ten sam (folder, UID) to ta sama wiadomość,
/// więc ewentualny inny wiersz z tą lokalizacją jest nieaktualnym duplikatem.
pub fn set_location(conn: &Connection, account_id: i64, id: i64, mailbox: Option<&str>, uid: Option<u32>) -> Result<()> {
    if let (Some(mailbox), Some(uid)) = (mailbox, uid) {
        conn.execute(
            "DELETE FROM messages WHERE account_id = ? AND mailbox = ? AND server_uid = ? AND id != ?",
            params![account_id, mailbox, uid, id],
        )?;
    }
    conn.execute("UPDATE messages SET mailbox = ?, server_uid = ? WHERE id = ?", params![mailbox, uid, id])?;
    Ok(())
}

/// Grupowanie z zachowaniem kolejności pierwszego wystąpienia klucza.
pub fn group_by<T, K: PartialEq + Clone>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<(K, Vec<T>)> {
    let mut groups: Vec<(K, Vec<T>)> = Vec::new();
    for item in items {
        let k = key(&item);
        match groups.iter_mut().find(|(gk, _)| *gk == k) {
            Some((_, list)) => list.push(item),
            None => groups.push((k, vec![item])),
        }
    }
    groups
}

/// Uzupełnia brakujące UID (np. gdy serwer nie zwrócił COPYUID po przeniesieniu).
async fn resolve_missing_uids<S: ImapSession + ?Sized>(db: &Db, s: &mut S, account_id: i64) -> Result<()> {
    let rows: Vec<(i64, Option<String>, String)> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, message_id, mailbox FROM messages WHERE account_id = ? AND mailbox IS NOT NULL AND server_uid IS NULL",
        )?;
        let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    })?;
    for (mailbox, group) in group_by(rows, |r| r.2.clone()) {
        s.select(&mailbox).await?;
        for (id, message_id, _) in group {
            let uid = match &message_id {
                Some(m) => s.search_message_id(m).await?,
                None => None,
            };
            // nie znaleziono: odszuka ją krok lokalizowania w pozostałych folderach
            db.with(|c| set_location(c, account_id, id, uid.map(|_| mailbox.as_str()), uid))?;
        }
    }
    Ok(())
}

struct LocalRow {
    id: i64,
    folder: String,
    read: bool,
    starred: bool,
    mailbox: String,
    server_uid: u32,
    server_seen: Option<bool>,
    server_flagged: Option<bool>,
}

/// Usunięte na zawsze najpierw trafiają do kosza na serwerze.
fn target_of(r: &LocalRow) -> &str {
    if r.folder == "deleted" {
        "trash"
    } else {
        r.folder.as_str()
    }
}

pub async fn push_changes<S: ImapSession + ?Sized>(
    db: &Db,
    s: &mut S,
    account_id: i64,
    folders: &ServerFolders,
    cancel: &Option<Cancel>,
) -> Result<()> {
    resolve_missing_uids(db, s, account_id).await?;

    let rows: Vec<LocalRow> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, folder, read, starred, mailbox, server_uid, server_seen, server_flagged
             FROM messages
             WHERE account_id = ?1 AND mailbox IS NOT NULL AND server_uid IS NOT NULL AND (
               folder = 'deleted'
               OR read IS NOT server_seen
               OR (server_flagged IS NOT NULL AND starred != server_flagged)
               OR folder != CASE
                    WHEN upper(mailbox) = 'INBOX' THEN 'inbox'
                    WHEN mailbox = ?2 THEN 'trash'
                    WHEN mailbox = ?3 THEN 'spam'
                    ELSE 'archive' END
             )",
        )?;
        let rows = stmt.query_map(params![account_id, folders.trash, folders.spam], |r| {
            Ok(LocalRow {
                id: r.get(0)?,
                folder: r.get(1)?,
                read: r.get::<_, i64>(2)? == 1,
                starred: r.get::<_, i64>(3)? == 1,
                mailbox: r.get(4)?,
                server_uid: r.get(5)?,
                server_seen: r.get::<_, Option<i64>>(6)?.map(|v| v == 1),
                server_flagged: r.get::<_, Option<i64>>(7)?.map(|v| v == 1),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    })?;
    if rows.is_empty() {
        return Ok(());
    }

    for (mailbox, group) in group_by(rows, |r| r.mailbox.clone()) {
        if is_cancelled(cancel) {
            return Ok(());
        }
        s.select(&mailbox).await?;

        // 1. flagi, zanim wiadomość zmieni folder (przenoszą się razem z nią)
        type Pick = fn(&LocalRow) -> bool;
        let flag_ops: [(Pick, Flag, bool); 4] = [
            (|r| r.read && r.server_seen != Some(true), Flag::Seen, true),
            (|r| !r.read && r.server_seen != Some(false), Flag::Seen, false),
            (|r| r.server_flagged == Some(false) && r.starred, Flag::Flagged, true),
            (|r| r.server_flagged == Some(true) && !r.starred, Flag::Flagged, false),
        ];
        for (pick, flag, add) in flag_ops {
            let items: Vec<&LocalRow> = group.iter().filter(|r| pick(r)).collect();
            if items.is_empty() {
                continue;
            }
            let uids: Vec<u32> = items.iter().map(|r| r.server_uid).collect();
            s.set_flag(&uids, flag, add).await?;
            let column = if flag == Flag::Seen { "server_seen" } else { "server_flagged" };
            db.with(|c| {
                for r in &items {
                    c.execute(&format!("UPDATE messages SET {column} = ? WHERE id = ?"), params![add as i64, r.id])?;
                }
                Ok(())
            })?;
        }

        // 2. przeniesienia; trwałe usunięcie idzie najpierw do kosza
        let current = folder_of(folders, &mailbox);
        let moving: Vec<&LocalRow> = group.iter().filter(|r| target_of(r) != current).collect();
        for (target, items) in group_by(moving, |r| path_of(folders, target_of(r)).to_string()) {
            let uids: Vec<u32> = items.iter().map(|r| r.server_uid).collect();
            // Gmail: usunięcie z All Mail kasuje wiadomość ze wszystkich etykiet, więc tylko kopiujemy
            // (kopia do INBOX dodaje etykietę, do Kosza/Spamu przenosi)
            let map = if folders.gmail && mailbox == folders.archive {
                s.copy_to(&uids, &target).await?
            } else {
                s.move_to(&uids, &target).await?
            };
            db.with(|c| {
                let tx = c.unchecked_transaction()?;
                for r in &items {
                    let new_uid = map.as_ref().and_then(|m| m.get(&r.server_uid)).copied();
                    set_location(&tx, account_id, r.id, Some(&target), new_uid)?;
                }
                tx.commit()?;
                Ok(())
            })?;
        }
    }

    delete_from_trash(db, s, account_id, folders).await
}

/// Usuwa na zawsze wiadomości oznaczone 'deleted', które są już w koszu na serwerze.
async fn delete_from_trash<S: ImapSession + ?Sized>(db: &Db, s: &mut S, account_id: i64, folders: &ServerFolders) -> Result<()> {
    resolve_missing_uids(db, s, account_id).await?;
    let rows: Vec<(i64, u32)> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, server_uid FROM messages WHERE account_id = ? AND folder = 'deleted' AND mailbox = ? AND server_uid IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![account_id, folders.trash], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    })?;
    if rows.is_empty() {
        return Ok(());
    }
    s.select(&folders.trash).await?;
    let uids: Vec<u32> = rows.iter().map(|r| r.1).collect();
    s.delete(&uids).await?;
    db.with(|c| {
        let tx = c.unchecked_transaction()?;
        for (id, _) in &rows {
            tx.execute("DELETE FROM messages WHERE id = ?", [id])?;
        }
        tx.commit()?;
        Ok(())
    })
}
