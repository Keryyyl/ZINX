//! Testy synchronizacji na atrapie serwera IMAP.

use super::mock::MockServer;
use super::session::{BodyPart, Flags};
use super::sync::{sync_account, Progress, SyncOptions};
use crate::db::{messages, open_memory, Db};
use rusqlite::params;
use std::sync::{Arc, Mutex};

fn raw(id: &str, from: &str, subject: &str) -> String {
    format!(
        "From: {from}\r\nTo: me@x.pl\r\nSubject: {subject}\r\nMessage-ID: <{id}@x.pl>\r\nDate: Tue, 3 Nov 2026 10:00:00 +0000\r\n\r\nTreść {subject}\r\n"
    )
}

fn setup() -> (Db, MockServer) {
    let conn = open_memory();
    conn.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('me@x.pl', 'imap.x.pl', 'me')", []).unwrap();
    (Db::new(conn), MockServer::new())
}

async fn run(db: &Db, server: &MockServer) {
    sync_account(db, &mut server.session(), 1, &SyncOptions::default()).await.unwrap();
}

#[derive(Debug, PartialEq)]
struct Row {
    id: i64,
    folder: String,
    read: bool,
    starred: bool,
    mailbox: Option<String>,
    uid: Option<u32>,
}

fn row(db: &Db, mid: &str) -> Option<Row> {
    db.with(|c| {
        Ok(c.query_row(
            "SELECT id, folder, read, starred, mailbox, server_uid FROM messages WHERE message_id = ?",
            [format!("<{mid}@x.pl>")],
            |r| Ok(Row { id: r.get(0)?, folder: r.get(1)?, read: r.get::<_, i64>(2)? == 1, starred: r.get::<_, i64>(3)? == 1, mailbox: r.get(4)?, uid: r.get(5)? }),
        )
        .ok())
    })
    .unwrap()
}

fn count(db: &Db) -> i64 {
    db.with(|c| Ok(c.query_row("SELECT count(*) FROM messages", [], |r| r.get(0))?)).unwrap()
}

fn local(db: &Db, f: impl FnOnce(&rusqlite::Connection)) {
    db.with(|c| {
        f(c);
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn first_sync_downloads_inbox_with_flags_and_progress() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "Jan <jan@x.pl>", "Pierwszy"), true);
    server.add("INBOX", &raw("m2", "ola@x.pl", "Drugi"), false);
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let opts = SyncOptions { cancel: None, on_progress: Some(Box::new(move |p| sink.lock().unwrap().push(p))) };
    sync_account(&db, &mut server.session(), 1, &opts).await.unwrap();

    assert_eq!(count(&db), 2);
    let m1 = row(&db, "m1").unwrap();
    assert_eq!((m1.folder.as_str(), m1.read, m1.mailbox.as_deref(), m1.uid), ("inbox", true, Some("INBOX"), Some(1)));
    assert!(!row(&db, "m2").unwrap().read);
    let detail = messages::get(&db.lock().unwrap(), m1.id).unwrap().unwrap();
    assert_eq!((detail.row.from.as_str(), detail.row.subject.as_str(), detail.row.snippet.as_str()), ("Jan <jan@x.pl>", "Pierwszy", "Treść Pierwszy"));
    assert_eq!(events.lock().unwrap().last(), Some(&Progress { done: 2, total: 2 }));
}

#[tokio::test]
async fn incremental_sync_fetches_only_new_messages() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "a@x.pl", "A"), false);
    run(&db, &server).await;
    let after_first = server.0.lock().unwrap().full_fetches;
    // nic nowego: serwer zwraca ostatnią wiadomość dla „n:*”, ale jej nie pobieramy drugi raz
    run(&db, &server).await;
    assert_eq!(server.0.lock().unwrap().full_fetches, after_first);
    server.add("INBOX", &raw("m2", "b@x.pl", "B"), false);
    run(&db, &server).await;
    assert_eq!(server.0.lock().unwrap().full_fetches, after_first + 1);
    assert_eq!(count(&db), 2);
}

#[tokio::test]
async fn attachments_are_saved_as_metadata_only() {
    let (db, server) = setup();
    let uid = server.add("INBOX", &raw("m1", "a@x.pl", "Z plikiem"), false);
    server.set_structure(
        "INBOX",
        uid,
        BodyPart {
            mime: "multipart/mixed".into(),
            children: vec![
                BodyPart { part: Some("1".into()), mime: "text/plain".into(), ..Default::default() },
                BodyPart {
                    part: Some("2".into()),
                    mime: "application/pdf".into(),
                    filename: Some("CV.pdf".into()),
                    disposition: Some("attachment".into()),
                    size: 100,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    );
    run(&db, &server).await;
    let id = row(&db, "m1").unwrap().id;
    let detail = messages::get(&db.lock().unwrap(), id).unwrap().unwrap();
    assert_eq!(detail.attachments.len(), 1);
    assert_eq!((detail.attachments[0].filename.as_str(), detail.row.has_attachments), ("CV.pdf", true));
}

#[tokio::test]
async fn blocked_sender_lands_in_spam_on_server_too() {
    let (db, server) = setup();
    local(&db, |c| {
        c.execute("INSERT INTO spam_senders (email, created_at) VALUES ('spam@x.pl', 1)", []).unwrap();
    });
    server.add("INBOX", &raw("s1", "Spamer <spam@x.pl>", "Kup teraz"), false);
    server.add("INBOX", &raw("m1", "ok@x.pl", "Normalny"), false);
    run(&db, &server).await;

    let spam = row(&db, "s1").unwrap();
    assert_eq!((spam.folder.as_str(), spam.mailbox.as_deref()), ("spam", Some("Spam")));
    assert_eq!(server.uids("Spam").len(), 1);
    assert_eq!(server.uids("INBOX").len(), 1);
    assert_eq!(row(&db, "m1").unwrap().folder, "inbox");
}

#[tokio::test]
async fn local_actions_are_pushed_to_the_server() {
    let (db, server) = setup();
    for i in 1..=4 {
        server.add("INBOX", &raw(&format!("m{i}"), "a@x.pl", &format!("T{i}")), false);
    }
    run(&db, &server).await;
    let ids: Vec<i64> = ["m1", "m2", "m3", "m4"].iter().map(|m| row(&db, m).unwrap().id).collect();
    {
        let conn = db.lock().unwrap();
        messages::set_read(&conn, &[ids[0]], true).unwrap();
        messages::set_starred(&conn, &[ids[1]], true).unwrap();
        messages::move_to_folder(&conn, &[ids[2]], "archive").unwrap();
        messages::move_to_folder(&conn, &[ids[3]], "trash").unwrap();
    }
    server.clear_log();
    run(&db, &server).await;

    assert!(server.flags("INBOX", 1).seen);
    assert!(server.flags("INBOX", 2).flagged);
    assert_eq!(server.uids("Archive").len(), 1);
    assert_eq!(server.uids("Trash").len(), 1);
    let m3 = row(&db, "m3").unwrap();
    assert_eq!((m3.folder.as_str(), m3.mailbox.as_deref()), ("archive", Some("Archive")));
    assert!(m3.uid.is_some());
    // druga synchronizacja nie ma już nic do wypchnięcia
    server.clear_log();
    run(&db, &server).await;
    assert!(server.log().is_empty(), "niespodziewane polecenia: {:?}", server.log());
}

#[tokio::test]
async fn delete_forever_removes_from_server_and_database() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "a@x.pl", "T"), false);
    run(&db, &server).await;
    let id = row(&db, "m1").unwrap().id;
    messages::move_to_folder(&db.lock().unwrap(), &[id], "trash").unwrap();
    run(&db, &server).await;
    assert_eq!(server.uids("Trash").len(), 1);
    messages::delete_forever(&db.lock().unwrap(), &[id]).unwrap();
    run(&db, &server).await;
    assert!(server.uids("Trash").is_empty());
    assert!(row(&db, "m1").is_none());
}

#[tokio::test]
async fn server_side_flag_changes_are_pulled_unless_a_local_change_is_pending() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "a@x.pl", "T1"), false);
    server.add("INBOX", &raw("m2", "a@x.pl", "T2"), false);
    run(&db, &server).await;
    server.set_flags("INBOX", 1, Flags { seen: true, flagged: true });
    // lokalna, jeszcze niewypchnięta zmiana: m2 oznaczone jako przeczytane
    let m2 = row(&db, "m2").unwrap().id;
    messages::set_read(&db.lock().unwrap(), &[m2], true).unwrap();
    server.set_flags("INBOX", 2, Flags { seen: false, flagged: true });
    run(&db, &server).await;

    let m1 = row(&db, "m1").unwrap();
    assert!(m1.read && m1.starred);
    let m2 = row(&db, "m2").unwrap();
    assert!(m2.read, "lokalna zmiana wygrywa i trafia na serwer");
    assert!(server.flags("INBOX", 2).seen);
    assert!(m2.starred);
}

#[tokio::test]
async fn message_archived_elsewhere_is_found_by_message_id() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "a@x.pl", "T1"), false);
    server.add("INBOX", &raw("m2", "a@x.pl", "T2"), false);
    run(&db, &server).await;
    server.move_externally("INBOX", 1, "Archive");
    server.remove_externally("INBOX", 2);
    run(&db, &server).await;

    let m1 = row(&db, "m1").unwrap();
    assert_eq!((m1.folder.as_str(), m1.mailbox.as_deref()), ("archive", Some("Archive")));
    assert!(row(&db, "m2").is_none(), "usunięta na serwerze znika też lokalnie");
}

#[tokio::test]
async fn uidvalidity_change_relocates_messages_instead_of_duplicating() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "a@x.pl", "T1"), true);
    server.add("INBOX", &raw("m2", "a@x.pl", "T2"), false);
    run(&db, &server).await;
    server.reset_uids("INBOX", 2);
    run(&db, &server).await;

    assert_eq!(count(&db), 2);
    let m1 = row(&db, "m1").unwrap();
    assert_eq!((m1.folder.as_str(), m1.mailbox.as_deref(), m1.uid, m1.read), ("inbox", Some("INBOX"), Some(100), true));
    assert_eq!(row(&db, "m2").unwrap().uid, Some(101));
}

#[tokio::test]
async fn server_without_uidplus_is_resolved_by_message_id() {
    let (db, server) = setup();
    server.0.lock().unwrap().uidplus = false;
    server.add("INBOX", &raw("m1", "a@x.pl", "T1"), false);
    run(&db, &server).await;
    let id = row(&db, "m1").unwrap().id;
    messages::move_to_folder(&db.lock().unwrap(), &[id], "archive").unwrap();
    run(&db, &server).await;

    let m1 = row(&db, "m1").unwrap();
    assert_eq!((m1.folder.as_str(), m1.mailbox.as_deref()), ("archive", Some("Archive")));
    assert_eq!(m1.uid, Some(server.uids("Archive")[0]));
    server.clear_log();
    run(&db, &server).await;
    assert!(server.log().is_empty(), "{:?}", server.log());
}

#[tokio::test]
async fn missing_server_folders_are_created() {
    let (db, server) = setup();
    {
        let mut st = server.0.lock().unwrap();
        st.mailboxes.remove("Archive");
        st.mailboxes.remove("Spam");
    }
    run(&db, &server).await;
    let log = server.log();
    assert!(log.contains(&"CREATE Archive".to_string()) && log.contains(&"CREATE Spam".to_string()));
    assert!(!log.contains(&"CREATE Trash".to_string()));
}

#[tokio::test]
async fn gmail_archive_copies_instead_of_moving_out_of_all_mail() {
    let (db, server) = setup();
    {
        let mut st = server.0.lock().unwrap();
        st.gmail = true;
        st.mailboxes.remove("Archive");
        st.mailboxes.insert(
            "[Gmail]/All Mail".into(),
            super::mock::MockMailbox { special_use: Some("\\All".into()), uid_validity: 1, next_uid: 1, ..Default::default() },
        );
    }
    let uid = server.add("INBOX", &raw("m1", "a@x.pl", "T1"), false);
    run(&db, &server).await;
    // archiwizacja w Gmailu: wiadomość trafia do All Mail (kopia z INBOX), a lokalnie folder = archive
    let id = row(&db, "m1").unwrap().id;
    messages::move_to_folder(&db.lock().unwrap(), &[id], "archive").unwrap();
    run(&db, &server).await;
    assert_eq!(server.uids("[Gmail]/All Mail").len(), 1);
    assert!(!server.uids("INBOX").contains(&uid) || server.uids("INBOX").is_empty());
    assert_eq!(row(&db, "m1").unwrap().folder, "archive");

    // wiadomość w All Mail wraca do Odebranych przez kopię, nie przeniesienie
    messages::move_to_folder(&db.lock().unwrap(), &[id], "inbox").unwrap();
    server.clear_log();
    run(&db, &server).await;
    assert!(server.log().iter().any(|l| l.starts_with("COPY [Gmail]/All Mail->INBOX")), "{:?}", server.log());
    assert_eq!(server.uids("[Gmail]/All Mail").len(), 1, "w All Mail zostaje");
}

#[tokio::test]
async fn cancelled_sync_stops_before_downloading() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "a@x.pl", "T"), false);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let opts = SyncOptions { cancel: Some(cancel), on_progress: None };
    sync_account(&db, &mut server.session(), 1, &opts).await.unwrap();
    assert_eq!(count(&db), 0);
    run(&db, &server).await;
    assert_eq!(count(&db), 1);
}

#[tokio::test]
async fn failed_push_is_retried_on_the_next_sync() {
    let (db, server) = setup();
    server.add("INBOX", &raw("m1", "a@x.pl", "T"), false);
    run(&db, &server).await;
    let id = row(&db, "m1").unwrap().id;
    messages::set_read(&db.lock().unwrap(), &[id], true).unwrap();
    // serwer odrzuca przeniesienie (np. chwilowy błąd): zmiana musi przetrwać do następnej synchronizacji
    messages::move_to_folder(&db.lock().unwrap(), &[id], "archive").unwrap();
    server.0.lock().unwrap().fail_moves = true;
    let result = sync_account(&db, &mut server.session(), 1, &SyncOptions::default()).await;
    assert!(result.is_err());
    // stan lokalny został: zmiana nie zginęła
    let m = row(&db, "m1").unwrap();
    assert!((m.read, m.folder.as_str()) == (true, "archive"));
    server.0.lock().unwrap().fail_moves = false;
    run(&db, &server).await;
    assert_eq!(server.uids("Archive").len(), 1);
    local(&db, |c| {
        let n: i64 = c.query_row("SELECT count(*) FROM messages WHERE server_seen IS NOT read", params![], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    });
}
