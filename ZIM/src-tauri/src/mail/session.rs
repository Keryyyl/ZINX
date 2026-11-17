//! Granica z serwerem IMAP. Logika synchronizacji zna tylko ten interfejs, więc testujemy ją na atrapie
//! (`mock.rs`), a prawdziwe połączenie (`imap_real.rs`) jest cienką warstwą nad `async-imap`.

use super::folders::ListedMailbox;
use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Seen,
    Flagged,
}

/// Węzeł drzewa BODYSTRUCTURE
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyPart {
    /// Numer części na serwerze ("1", "1.2"); None dla korzenia
    pub part: Option<String>,
    /// "typ/podtyp"
    pub mime: String,
    pub children: Vec<BodyPart>,
    /// Nazwa z parametrów Content-Disposition albo Content-Type
    pub filename: Option<String>,
    pub disposition: Option<String>,
    pub id: Option<String>,
    pub size: u32,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Fetched {
    pub uid: u32,
    pub flags: Flags,
    /// Data odbioru na serwerze (unix ms)
    pub internal_date: Option<i64>,
    pub source: Vec<u8>,
    pub structure: Option<BodyPart>,
}

/// Mapa starych UID na nowe po przeniesieniu; None, gdy serwer jej nie zwrócił (brak UIDPLUS).
pub type UidMap = Option<HashMap<u32, u32>>;

#[async_trait]
pub trait ImapSession: Send {
    /// Gmail ma własne rozszerzenie (X-GM-EXT-1): archiwum to "All Mail"
    fn is_gmail(&self) -> bool;
    async fn list(&mut self) -> Result<Vec<ListedMailbox>>;
    async fn create(&mut self, path: &str) -> Result<()>;
    /// Otwiera folder; kolejne polecenia dotyczą jego. Zwraca UIDVALIDITY.
    async fn select(&mut self, path: &str) -> Result<u32>;
    /// UID wiadomości o UID >= `from` w otwartym folderze
    async fn search_uids_from(&mut self, from: u32) -> Result<Vec<u32>>;
    /// Najnowszy UID wiadomości o danym nagłówku Message-ID w otwartym folderze
    async fn search_message_id(&mut self, message_id: &str) -> Result<Option<u32>>;
    async fn fetch_full(&mut self, uids: &[u32]) -> Result<Vec<Fetched>>;
    async fn fetch_flags(&mut self, uids: &[u32]) -> Result<Vec<(u32, Flags)>>;
    async fn fetch_structures(&mut self, uids: &[u32]) -> Result<Vec<(u32, BodyPart)>>;
    async fn set_flag(&mut self, uids: &[u32], flag: Flag, add: bool) -> Result<()>;
    async fn move_to(&mut self, uids: &[u32], target: &str) -> Result<UidMap>;
    async fn copy_to(&mut self, uids: &[u32], target: &str) -> Result<UidMap>;
    /// Trwale usuwa wiadomości z otwartego folderu (\Deleted + EXPUNGE)
    async fn delete(&mut self, uids: &[u32]) -> Result<()>;
    /// Dopisuje gotową wiadomość do folderu (kopia w „Wysłanych”)
    async fn append(&mut self, path: &str, raw: &[u8], seen: bool) -> Result<()>;
    /// Treść jednej części MIME otwartego folderu, już zdekodowana (base64/quoted-printable)
    async fn fetch_part(&mut self, uid: u32, part: &str) -> Result<Vec<u8>>;
    async fn logout(&mut self) -> Result<()>;
}
