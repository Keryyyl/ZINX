//! Atrapa serwera IMAP do testów synchronizacji: foldery, UID-y, flagi, UIDVALIDITY, przenoszenie z UIDPLUS
//! albo bez, a także zachowanie `UID SEARCH n:*`, które zawsze zwraca ostatnią wiadomość.

use super::folders::ListedMailbox;
use super::session::{BodyPart, Fetched, Flag, Flags, ImapSession, UidMap};
use crate::error::{Error, Result};
use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct MockMessage {
    pub source: Vec<u8>,
    pub flags: Flags,
    pub structure: Option<BodyPart>,
}

#[derive(Debug, Default)]
pub struct MockMailbox {
    pub special_use: Option<String>,
    pub uid_validity: u32,
    pub next_uid: u32,
    pub messages: BTreeMap<u32, MockMessage>,
}

#[derive(Debug, Default)]
pub struct ServerState {
    pub mailboxes: BTreeMap<String, MockMailbox>,
    pub gmail: bool,
    /// false = serwer bez UIDPLUS: po przeniesieniu nie zna nowych UID
    pub uidplus: bool,
    /// Dziennik poleceń, np. "MOVE INBOX->Archive [1, 2]"
    pub log: Vec<String>,
    /// Liczba pobrań pełnej treści (do sprawdzania, że sync nie ściąga wszystkiego od nowa)
    pub full_fetches: usize,
    /// true = każde przeniesienie kończy się błędem serwera
    pub fail_moves: bool,
    /// Zdekodowana treść części (folder, uid, numer części)
    pub parts: HashMap<(String, u32, String), Vec<u8>>,
    pub part_fetches: usize,
}

#[derive(Clone)]
pub struct MockServer(pub Arc<Mutex<ServerState>>);

impl MockServer {
    pub fn new() -> Self {
        let mut st = ServerState { uidplus: true, ..Default::default() };
        for (path, use_) in [("INBOX", None), ("Archive", Some("\\Archive")), ("Spam", Some("\\Junk")), ("Trash", Some("\\Trash"))] {
            st.mailboxes.insert(path.into(), MockMailbox { special_use: use_.map(str::to_string), uid_validity: 1, next_uid: 1, ..Default::default() });
        }
        MockServer(Arc::new(Mutex::new(st)))
    }

    pub fn session(&self) -> MockSession {
        MockSession { server: self.clone(), current: String::new() }
    }

    /// Dokłada wiadomość i zwraca jej UID.
    pub fn add(&self, mailbox: &str, source: &str, seen: bool) -> u32 {
        let mut st = self.0.lock().unwrap();
        let mb = st.mailboxes.get_mut(mailbox).expect("brak folderu");
        let uid = mb.next_uid;
        mb.next_uid += 1;
        mb.messages.insert(uid, MockMessage { source: source.as_bytes().to_vec(), flags: Flags { seen, flagged: false }, structure: None });
        uid
    }

    pub fn set_structure(&self, mailbox: &str, uid: u32, structure: BodyPart) {
        self.0.lock().unwrap().mailboxes.get_mut(mailbox).unwrap().messages.get_mut(&uid).unwrap().structure = Some(structure);
    }

    pub fn uids(&self, mailbox: &str) -> Vec<u32> {
        self.0.lock().unwrap().mailboxes[mailbox].messages.keys().copied().collect()
    }

    pub fn flags(&self, mailbox: &str, uid: u32) -> Flags {
        self.0.lock().unwrap().mailboxes[mailbox].messages[&uid].flags
    }

    pub fn set_flags(&self, mailbox: &str, uid: u32, flags: Flags) {
        self.0.lock().unwrap().mailboxes.get_mut(mailbox).unwrap().messages.get_mut(&uid).unwrap().flags = flags;
    }

    /// Przenosi wiadomość tak, jak zrobiłby to inny klient (poza ZIM).
    pub fn move_externally(&self, from: &str, uid: u32, to: &str) -> u32 {
        let mut st = self.0.lock().unwrap();
        let msg = st.mailboxes.get_mut(from).unwrap().messages.remove(&uid).unwrap();
        let target = st.mailboxes.get_mut(to).unwrap();
        let new_uid = target.next_uid;
        target.next_uid += 1;
        target.messages.insert(new_uid, msg);
        new_uid
    }

    pub fn remove_externally(&self, mailbox: &str, uid: u32) {
        self.0.lock().unwrap().mailboxes.get_mut(mailbox).unwrap().messages.remove(&uid);
    }

    /// Zmiana UIDVALIDITY: serwer „zapomniał” UID-y, wiadomości dostają nowe numery.
    pub fn reset_uids(&self, mailbox: &str, new_validity: u32) {
        let mut st = self.0.lock().unwrap();
        let mb = st.mailboxes.get_mut(mailbox).unwrap();
        let old = std::mem::take(&mut mb.messages);
        mb.uid_validity = new_validity;
        mb.next_uid = 100;
        for (_, msg) in old {
            mb.messages.insert(mb.next_uid, msg);
            mb.next_uid += 1;
        }
    }

    pub fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().log.clone()
    }

    pub fn clear_log(&self) {
        self.0.lock().unwrap().log.clear();
    }
}

pub struct MockSession {
    server: MockServer,
    current: String,
}

fn message_id_matches(source: &[u8], wanted: &str) -> bool {
    let text = String::from_utf8_lossy(source).to_lowercase();
    let wanted = wanted.to_lowercase();
    text.lines().any(|l| l.starts_with("message-id:") && l.contains(&wanted))
}

#[async_trait]
impl ImapSession for MockSession {
    fn is_gmail(&self) -> bool {
        self.server.0.lock().unwrap().gmail
    }

    async fn list(&mut self) -> Result<Vec<ListedMailbox>> {
        let st = self.server.0.lock().unwrap();
        Ok(st.mailboxes.iter().map(|(p, m)| ListedMailbox { path: p.clone(), special_use: m.special_use.clone() }).collect())
    }

    async fn create(&mut self, path: &str) -> Result<()> {
        let mut st = self.server.0.lock().unwrap();
        st.log.push(format!("CREATE {path}"));
        let special = match path {
            "Archive" => Some("\\Archive"),
            "Spam" => Some("\\Junk"),
            "Trash" => Some("\\Trash"),
            _ => None,
        };
        st.mailboxes.entry(path.into()).or_insert(MockMailbox { special_use: special.map(str::to_string), uid_validity: 1, next_uid: 1, ..Default::default() });
        Ok(())
    }

    async fn select(&mut self, path: &str) -> Result<u32> {
        let st = self.server.0.lock().unwrap();
        let mb = st.mailboxes.get(path).ok_or_else(|| format!("Nie udało się otworzyć {path}"))?;
        self.current = path.to_string();
        Ok(mb.uid_validity)
    }

    async fn search_uids_from(&mut self, from: u32) -> Result<Vec<u32>> {
        let st = self.server.0.lock().unwrap();
        let mb = &st.mailboxes[&self.current];
        let mut found: Vec<u32> = mb.messages.keys().copied().filter(|u| *u >= from).collect();
        // „n:*” obejmuje zawsze ostatnią wiadomość, także gdy jej UID < n
        if found.is_empty() {
            if let Some(last) = mb.messages.keys().next_back() {
                found.push(*last);
            }
        }
        Ok(found)
    }

    async fn search_message_id(&mut self, message_id: &str) -> Result<Option<u32>> {
        let st = self.server.0.lock().unwrap();
        let mb = &st.mailboxes[&self.current];
        Ok(mb.messages.iter().filter(|(_, m)| message_id_matches(&m.source, message_id)).map(|(u, _)| *u).max())
    }

    async fn fetch_full(&mut self, uids: &[u32]) -> Result<Vec<Fetched>> {
        let mut st = self.server.0.lock().unwrap();
        st.full_fetches += uids.len();
        let mb = &st.mailboxes[&self.current];
        Ok(uids
            .iter()
            .filter_map(|u| mb.messages.get(u).map(|m| Fetched { uid: *u, flags: m.flags, internal_date: Some(1_000), source: m.source.clone(), structure: m.structure.clone() }))
            .collect())
    }

    async fn fetch_flags(&mut self, uids: &[u32]) -> Result<Vec<(u32, Flags)>> {
        let st = self.server.0.lock().unwrap();
        let mb = &st.mailboxes[&self.current];
        Ok(uids.iter().filter_map(|u| mb.messages.get(u).map(|m| (*u, m.flags))).collect())
    }

    async fn fetch_structures(&mut self, uids: &[u32]) -> Result<Vec<(u32, BodyPart)>> {
        let st = self.server.0.lock().unwrap();
        let mb = &st.mailboxes[&self.current];
        Ok(uids
            .iter()
            .filter_map(|u| mb.messages.get(u).map(|m| (*u, m.structure.clone().unwrap_or(BodyPart { mime: "text/plain".into(), ..Default::default() }))))
            .collect())
    }

    async fn set_flag(&mut self, uids: &[u32], flag: Flag, add: bool) -> Result<()> {
        let mut st = self.server.0.lock().unwrap();
        st.log.push(format!("{} {:?} {} {:?}", if add { "ADD" } else { "REMOVE" }, flag, self.current, uids));
        let current = self.current.clone();
        let mb = st.mailboxes.get_mut(&current).unwrap();
        for u in uids {
            if let Some(m) = mb.messages.get_mut(u) {
                match flag {
                    Flag::Seen => m.flags.seen = add,
                    Flag::Flagged => m.flags.flagged = add,
                }
            }
        }
        Ok(())
    }

    async fn move_to(&mut self, uids: &[u32], target: &str) -> Result<UidMap> {
        let mut st = self.server.0.lock().unwrap();
        st.log.push(format!("MOVE {}->{} {:?}", self.current, target, uids));
        if st.fail_moves {
            return Err("Serwer odrzucił przeniesienie".into());
        }
        let current = self.current.clone();
        let uidplus = st.uidplus;
        let mut map = HashMap::new();
        for u in uids {
            let Some(msg) = st.mailboxes.get_mut(&current).unwrap().messages.remove(u) else { continue };
            let t = st.mailboxes.get_mut(target).ok_or_else(|| format!("Nie ma folderu {target}"))?;
            map.insert(*u, t.next_uid);
            t.messages.insert(t.next_uid, msg);
            t.next_uid += 1;
        }
        Ok(uidplus.then_some(map))
    }

    async fn copy_to(&mut self, uids: &[u32], target: &str) -> Result<UidMap> {
        let mut st = self.server.0.lock().unwrap();
        st.log.push(format!("COPY {}->{} {:?}", self.current, target, uids));
        let current = self.current.clone();
        let uidplus = st.uidplus;
        let mut map = HashMap::new();
        for u in uids {
            let Some(msg) = st.mailboxes[&current].messages.get(u).cloned() else { continue };
            let t = st.mailboxes.get_mut(target).ok_or_else(|| format!("Nie ma folderu {target}"))?;
            map.insert(*u, t.next_uid);
            t.messages.insert(t.next_uid, msg);
            t.next_uid += 1;
        }
        Ok(uidplus.then_some(map))
    }

    async fn delete(&mut self, uids: &[u32]) -> Result<()> {
        let mut st = self.server.0.lock().unwrap();
        st.log.push(format!("DELETE {} {:?}", self.current, uids));
        let current = self.current.clone();
        let mb = st.mailboxes.get_mut(&current).unwrap();
        for u in uids {
            mb.messages.remove(u);
        }
        Ok(())
    }

    async fn append(&mut self, path: &str, raw: &[u8], seen: bool) -> Result<()> {
        let mut st = self.server.0.lock().unwrap();
        st.log.push(format!("APPEND {path}"));
        let mb = st.mailboxes.get_mut(path).ok_or_else(|| format!("Nie ma folderu {path}"))?;
        let uid = mb.next_uid;
        mb.next_uid += 1;
        mb.messages.insert(uid, MockMessage { source: raw.to_vec(), flags: Flags { seen, flagged: false }, structure: None });
        Ok(())
    }

    async fn fetch_part(&mut self, uid: u32, part: &str) -> Result<Vec<u8>> {
        let mut st = self.server.0.lock().unwrap();
        st.log.push(format!("FETCH_PART {} {uid} {part}", self.current));
        st.part_fetches += 1;
        let mb = &st.mailboxes[&self.current];
        let msg = mb.messages.get(&uid).ok_or("Nie ma takiej wiadomości na serwerze")?;
        st.parts.get(&(self.current.clone(), uid, part.to_string())).cloned().ok_or_else(|| Error::from("Nie ma takiej części"))
            .or_else(|_| Ok(msg.source.clone()))
    }

    async fn logout(&mut self) -> Result<()> {
        Ok(())
    }
}
