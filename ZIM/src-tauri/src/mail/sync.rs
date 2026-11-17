//! Dwukierunkowa synchronizacja konta:
//! 1. wypchnięcie zmian z ZIM (foldery, przeczytane, gwiazdki, usunięcia),
//! 2. pobranie nowych wiadomości z INBOX,
//! 3. pobranie flag i wykrycie wiadomości przeniesionych/usuniętych na serwerze,
//! 4. ponowne wypchnięcie (np. nowe maile od zablokowanych nadawców idą do spamu).

use super::attachments::{collect_attachments, AttachmentInfo};
use super::folders::{folder_of, pick_folders, Missing, ServerFolders};
use super::parse::{parse_message, ParsedMail};
use super::remote::{is_cancelled, push_changes, set_location, Cancel};
use super::session::{Fetched, ImapSession};
use crate::db::{attachments as attachment_db, Db};
use crate::error::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

const INBOX: &str = "INBOX";
const BATCH: usize = 25;
const BACKFILL_BATCH: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Default)]
pub struct SyncOptions {
    pub cancel: Option<Cancel>,
    /// Wołane po każdej zapisanej paczce, żeby interfejs mógł odświeżyć listę.
    pub on_progress: Option<Box<dyn Fn(Progress) + Send + Sync>>,
}

pub async fn sync_account<S: ImapSession + ?Sized>(db: &Db, s: &mut S, account_id: i64, opts: &SyncOptions) -> Result<()> {
    let (folders, available) = discover_folders(s).await?;
    push_changes(db, s, account_id, &folders, &opts.cancel).await?;
    if is_cancelled(&opts.cancel) {
        return Ok(());
    }

    let watched: Vec<String> = db.with(|c| {
        let mut stmt = c.prepare("SELECT DISTINCT mailbox FROM messages WHERE account_id = ? AND mailbox IS NOT NULL")?;
        let rows = stmt.query_map([account_id], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    })?;
    let mut mailboxes = vec![INBOX.to_string()];
    mailboxes.extend(watched.into_iter().filter(|m| !m.eq_ignore_ascii_case(INBOX)));

    let mut vanished: Vec<Vanished> = Vec::new();
    for mailbox in &mailboxes {
        if is_cancelled(&opts.cancel) {
            return Ok(());
        }
        let uid_validity = s.select(mailbox).await?;
        db.with(|c| check_uid_validity(c, account_id, mailbox, uid_validity))?;
        if mailbox == INBOX {
            download_new(db, s, account_id, opts).await?;
        }
        vanished.extend(refresh_flags(db, s, account_id, mailbox, &folders).await?);
        if !is_cancelled(&opts.cancel) {
            backfill_attachments(db, s, account_id, mailbox).await?;
        }
    }
    if is_cancelled(&opts.cancel) {
        return Ok(());
    }
    locate_moved(db, s, account_id, &folders, &available, vanished).await?;
    push_changes(db, s, account_id, &folders, &opts.cancel).await?;
    s.logout().await
}

async fn discover_folders<S: ImapSession + ?Sized>(s: &mut S) -> Result<(ServerFolders, HashSet<String>)> {
    let gmail = s.is_gmail();
    let mut list = s.list().await?;
    let (folders, missing) = pick_folders(&list, gmail);
    if !missing.is_empty() {
        // serwer bez folderu archiwum/spamu/kosza: zakładamy go, jak robią inne klienty
        for key in missing {
            let _: Missing = key;
            let _ = s.create(key.default_path()).await;
        }
        list = s.list().await?;
    }
    Ok((folders, list.into_iter().map(|m| m.path).collect()))
}

/// Zmiana UIDVALIDITY = stare UID w tym folderze nic nie znaczą; wiadomości odszukamy po Message-ID.
fn check_uid_validity(conn: &Connection, account_id: i64, mailbox: &str, uid_validity: u32) -> Result<()> {
    let stored: Option<String> = conn
        .query_row("SELECT uid_validity FROM sync_state WHERE account_id = ? AND mailbox = ?", params![account_id, mailbox], |r| r.get(0))
        .ok();
    let current = uid_validity.to_string();
    if stored.as_deref().is_some_and(|s| s != current) {
        let tx = conn.unchecked_transaction()?;
        // uid = -id: stara kolumna uid ma UNIQUE, a nowe UID z INBOX zaczną się od początku
        tx.execute(
            "UPDATE messages SET mailbox = NULL, server_uid = NULL, uid = -id WHERE account_id = ? AND mailbox = ?",
            params![account_id, mailbox],
        )?;
        tx.execute("UPDATE sync_state SET last_uid = 0 WHERE account_id = ? AND mailbox = ?", params![account_id, mailbox])?;
        tx.commit()?;
    }
    conn.execute(
        "INSERT INTO sync_state (account_id, mailbox, uid_validity, last_uid) VALUES (?, ?, ?, 0)
         ON CONFLICT (account_id, mailbox) DO UPDATE SET uid_validity = excluded.uid_validity",
        params![account_id, mailbox, current],
    )?;
    Ok(())
}

struct NewMessage {
    uid: u32,
    mail: ParsedMail,
    seen: bool,
    flagged: bool,
    attachments: Option<Vec<AttachmentInfo>>,
}

fn into_new_message(f: Fetched) -> Option<NewMessage> {
    if f.source.is_empty() {
        return None;
    }
    let mail = parse_message(&f.source, f.internal_date)?;
    Some(NewMessage {
        uid: f.uid,
        attachments: f.structure.as_ref().map(collect_attachments),
        seen: f.flags.seen,
        flagged: f.flags.flagged,
        mail,
    })
}

fn save_new_messages(conn: &Connection, account_id: i64, rows: &[NewMessage]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for row in rows {
        // wiadomość, którą już mamy, ale zgubiliśmy jej położenie (np. po zmianie UIDVALIDITY)
        let lost: Option<i64> = match &row.mail.message_id {
            Some(mid) => conn
                .query_row(
                    "SELECT id FROM messages WHERE account_id = ? AND message_id = ? AND mailbox IS NULL AND folder != 'deleted'",
                    params![account_id, mid],
                    |r| r.get(0),
                )
                .ok(),
            None => None,
        };
        if let Some(id) = lost {
            set_location(&tx, account_id, id, Some(INBOX), Some(row.uid))?;
            tx.execute(
                "UPDATE messages SET folder = 'inbox', read = ?1, starred = ?2, server_seen = ?1, server_flagged = ?2 WHERE id = ?3",
                params![row.seen as i64, row.flagged as i64, id],
            )?;
            continue;
        }
        let m = &row.mail;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO messages
               (account_id, uid, message_id, thread_id, from_addr, from_email, to_addr, subject, date,
                snippet, body_text, body_html, bulk, read, starred, folder, mailbox, server_uid, server_seen, server_flagged)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                -- nadawca oznaczony wcześniej jako spam; przeniesienie na serwerze zrobi push_changes
                CASE WHEN EXISTS (SELECT 1 FROM spam_senders WHERE email = ?6) THEN 'spam' ELSE 'inbox' END,
                'INBOX', ?2, ?14, ?15)",
            params![
                account_id, row.uid, m.message_id, m.thread_id, m.from, m.from_email, m.to, m.subject, m.date, m.snippet,
                m.body_text, m.body_html, m.bulk as i64, row.seen as i64, row.flagged as i64
            ],
        )?;
        if inserted > 0 {
            if let Some(attachments) = &row.attachments {
                attachment_db::save(&tx, tx.last_insert_rowid(), attachments)?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

/// Pobiera nowe wiadomości z otwartego INBOX, od najnowszych, paczkami.
async fn download_new<S: ImapSession + ?Sized>(db: &Db, s: &mut S, account_id: i64, opts: &SyncOptions) -> Result<()> {
    let last_uid: u32 = db.with(|c| {
        Ok(c.query_row("SELECT last_uid FROM sync_state WHERE account_id = ? AND mailbox = ?", params![account_id, INBOX], |r| r.get(0))?)
    })?;
    let found = s.search_uids_from(last_uid + 1).await?;
    // "n:*" zwraca zawsze ostatnią wiadomość, nawet gdy jej UID < n
    let existing: HashSet<u32> = db.with(|c| {
        let mut stmt = c.prepare("SELECT server_uid FROM messages WHERE account_id = ? AND mailbox = ? AND server_uid IS NOT NULL")?;
        let rows = stmt.query_map(params![account_id, INBOX], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    })?;
    let mut uids: Vec<u32> = found.iter().copied().filter(|u| *u > last_uid && !existing.contains(u)).collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    let total = uids.len();
    if total > 0 {
        if let Some(cb) = &opts.on_progress {
            cb(Progress { done: 0, total });
        }
    }

    for (i, chunk) in uids.chunks(BATCH).enumerate() {
        if is_cancelled(&opts.cancel) {
            return Ok(());
        }
        let rows: Vec<NewMessage> = s.fetch_full(chunk).await?.into_iter().filter_map(into_new_message).collect();
        db.with(|c| save_new_messages(c, account_id, &rows))?;
        if let Some(cb) = &opts.on_progress {
            cb(Progress { done: ((i + 1) * BATCH).min(total), total });
        }
    }

    // stan zapisujemy dopiero po pełnym przebiegu; przerwany sync dokończy się następnym razem
    let max_uid = found.iter().copied().fold(last_uid, u32::max);
    db.with(|c| {
        c.execute("UPDATE sync_state SET last_uid = ? WHERE account_id = ? AND mailbox = ?", params![max_uid, account_id, INBOX])?;
        Ok(())
    })
}

/// Wiadomości pobrane przed obsługą załączników nie mają zapisanych metadanych.
/// Pobieramy tylko BODYSTRUCTURE (bez treści), po kilkaset na synchronizację na folder.
async fn backfill_attachments<S: ImapSession + ?Sized>(db: &Db, s: &mut S, account_id: i64, mailbox: &str) -> Result<()> {
    let pending = db.with(|c| attachment_db::unchecked(c, account_id, mailbox, BACKFILL_BATCH))?;
    if pending.is_empty() {
        return Ok(());
    }
    let id_by_uid: HashMap<u32, i64> = pending.iter().map(|(id, uid)| (*uid, *id)).collect();
    let uids: Vec<u32> = id_by_uid.keys().copied().collect();
    let structures = s.fetch_structures(&uids).await?;
    db.with(|c| {
        let tx = c.unchecked_transaction()?;
        for (uid, structure) in &structures {
            if let Some(id) = id_by_uid.get(uid) {
                attachment_db::save(&tx, *id, &collect_attachments(structure))?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

pub struct Vanished {
    id: i64,
    message_id: Option<String>,
    from: &'static str,
}

/// Przeczytane/gwiazdki zmienione na serwerze przyjmujemy, o ile w ZIM nie ma niewypchniętej zmiany
/// (read == server_seen). Zwraca wiadomości, których już nie ma w tym folderze na serwerze.
async fn refresh_flags<S: ImapSession + ?Sized>(db: &Db, s: &mut S, account_id: i64, mailbox: &str, folders: &ServerFolders) -> Result<Vec<Vanished>> {
    let known: Vec<(i64, Option<String>, u32)> = db.with(|c| {
        let mut stmt = c.prepare("SELECT id, message_id, server_uid FROM messages WHERE account_id = ? AND mailbox = ? AND server_uid IS NOT NULL")?;
        let rows = stmt.query_map(params![account_id, mailbox], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    })?;
    if known.is_empty() {
        return Ok(Vec::new());
    }
    let by_uid: HashMap<u32, i64> = known.iter().map(|k| (k.2, k.0)).collect();
    let uids: Vec<u32> = known.iter().map(|k| k.2).collect();
    let mut seen_uids: HashSet<u32> = HashSet::new();
    let mut updates: Vec<(i64, bool, bool)> = Vec::new();
    for (uid, flags) in s.fetch_flags(&uids).await? {
        if let Some(id) = by_uid.get(&uid) {
            seen_uids.insert(uid);
            updates.push((*id, flags.seen, flags.flagged));
        }
    }
    db.with(|c| {
        let tx = c.unchecked_transaction()?;
        for (id, seen, flagged) in &updates {
            tx.execute(
                "UPDATE messages SET
                   read = CASE WHEN server_seen IS NULL OR read = server_seen THEN ?1 ELSE read END,
                   -- NULL = gwiazdki z serwera jeszcze nie znamy (dane sprzed synchronizacji): scalamy
                   starred = CASE WHEN server_flagged IS NULL THEN max(starred, ?2)
                                  WHEN starred = server_flagged THEN ?2 ELSE starred END,
                   server_seen = ?1, server_flagged = ?2
                 WHERE id = ?3",
                params![*seen as i64, *flagged as i64, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    })?;
    let from = folder_of(folders, mailbox);
    Ok(known
        .into_iter()
        .filter(|k| !seen_uids.contains(&k.2))
        .map(|k| Vanished { id: k.0, message_id: k.1, from })
        .collect())
}

/// Wiadomości, które zniknęły z folderu (przeniesione lub usunięte w innym kliencie), szukamy po Message-ID
/// w pozostałych folderach. Kolejność ma znaczenie w Gmailu: All Mail (archiwum) zawiera też INBOX,
/// więc sprawdzamy je na końcu.
async fn locate_moved<S: ImapSession + ?Sized>(
    db: &Db,
    s: &mut S,
    account_id: i64,
    folders: &ServerFolders,
    available: &HashSet<String>,
    vanished: Vec<Vanished>,
) -> Result<()> {
    db.with(|c| {
        let tx = c.unchecked_transaction()?;
        for v in &vanished {
            set_location(&tx, account_id, v.id, None, None)?;
            if v.message_id.is_some() {
                continue;
            }
            // bez Message-ID nie odszukamy; z INBOX najpewniej zarchiwizowana, z innych folderów usunięta
            if v.from == "inbox" {
                tx.execute("UPDATE messages SET folder = 'archive' WHERE id = ?", [v.id])?;
            } else {
                tx.execute("DELETE FROM messages WHERE id = ?", [v.id])?;
            }
        }
        tx.commit()?;
        Ok(())
    })?;

    // także te zgubione wcześniej (np. przerwana synchronizacja, zmiana UIDVALIDITY)
    let mut pending: Vec<(i64, String)> = db.with(|c| {
        let mut stmt = c.prepare("SELECT id, message_id FROM messages WHERE account_id = ? AND mailbox IS NULL AND message_id IS NOT NULL")?;
        let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    })?;
    if pending.is_empty() {
        return Ok(());
    }

    for folder in ["trash", "spam", "inbox", "archive"] {
        if pending.is_empty() {
            break;
        }
        let path = match folder {
            "trash" => &folders.trash,
            "spam" => &folders.spam,
            "inbox" => &folders.inbox,
            _ => &folders.archive,
        };
        // brak folderu na serwerze (nie udało się go założyć): szukamy w pozostałych
        if path != INBOX && !available.contains(path) {
            continue;
        }
        s.select(path).await?;
        let mut found: Vec<(i64, u32, bool, bool)> = Vec::new();
        for (id, message_id) in &pending {
            let Some(uid) = s.search_message_id(message_id).await? else { continue };
            let flags = s.fetch_flags(&[uid]).await?.into_iter().next().map(|f| f.1).unwrap_or_default();
            found.push((*id, uid, flags.seen, flags.flagged));
        }
        db.with(|c| {
            let tx = c.unchecked_transaction()?;
            for (id, uid, seen, flagged) in &found {
                set_location(&tx, account_id, *id, Some(path), Some(*uid))?;
                tx.execute(
                    "UPDATE messages SET folder = CASE WHEN folder = 'deleted' THEN 'deleted' ELSE ?1 END,
                       read = ?2, starred = ?3, server_seen = ?2, server_flagged = ?3 WHERE id = ?4",
                    params![folder, *seen as i64, *flagged as i64, id],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;
        let located: HashSet<i64> = found.iter().map(|f| f.0).collect();
        pending.retain(|p| !located.contains(&p.0));
    }

    // nie ma jej w żadnym folderze: usunięta na serwerze
    db.with(|c| {
        let tx = c.unchecked_transaction()?;
        for (id, _) in &pending {
            tx.execute("DELETE FROM messages WHERE id = ?", [id])?;
        }
        tx.commit()?;
        Ok(())
    })
}
