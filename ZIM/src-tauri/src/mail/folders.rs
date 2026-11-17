/// Ścieżki folderów ZIM na konkretnym serwerze IMAP
#[derive(Debug, Clone, PartialEq)]
pub struct ServerFolders {
    pub inbox: String,
    pub archive: String,
    pub spam: String,
    pub trash: String,
    /// Gmail: archiwum to "All Mail", które zawiera też wiadomości z INBOX
    pub gmail: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListedMailbox {
    pub path: String,
    pub special_use: Option<String>,
}

impl ListedMailbox {
    #[cfg(test)]
    pub fn new(path: &str, special_use: Option<&str>) -> Self {
        Self { path: path.into(), special_use: special_use.map(str::to_string) }
    }
}

/// Foldery do utworzenia, gdy serwer nie ma odpowiednika
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    Archive,
    Spam,
    Trash,
}

impl Missing {
    pub fn default_path(self) -> &'static str {
        match self {
            Missing::Archive => "Archive",
            Missing::Spam => "Spam",
            Missing::Trash => "Trash",
        }
    }
}

/// Dobiera foldery po flagach special-use. Brakujące zwraca jako `missing`.
pub fn pick_folders(list: &[ListedMailbox], gmail: bool) -> (ServerFolders, Vec<Missing>) {
    let by_use = |u: &str| list.iter().find(|m| m.special_use.as_deref() == Some(u)).map(|m| m.path.clone());
    let by_name = |n: &str| list.iter().find(|m| m.path.eq_ignore_ascii_case(n)).map(|m| m.path.clone());

    // w Gmailu archiwizacja = zdjęcie etykiety Inbox, wiadomość zostaje w All Mail
    let archive = if gmail { by_use("\\All").or_else(|| by_use("\\Archive")) } else { by_use("\\Archive").or_else(|| by_name("Archive")) };
    let spam = by_use("\\Junk");
    let trash = by_use("\\Trash");

    let mut missing = Vec::new();
    if archive.is_none() {
        missing.push(Missing::Archive);
    }
    if spam.is_none() {
        missing.push(Missing::Spam);
    }
    if trash.is_none() {
        missing.push(Missing::Trash);
    }
    let folders = ServerFolders {
        inbox: "INBOX".into(),
        archive: archive.unwrap_or_else(|| Missing::Archive.default_path().into()),
        spam: spam.unwrap_or_else(|| Missing::Spam.default_path().into()),
        trash: trash.unwrap_or_else(|| Missing::Trash.default_path().into()),
        gmail,
    };
    (folders, missing)
}

/// Folder ZIM odpowiadający ścieżce na serwerze; nieznana ścieżka = archiwum
pub fn folder_of(folders: &ServerFolders, mailbox: &str) -> &'static str {
    if mailbox.eq_ignore_ascii_case("INBOX") {
        "inbox"
    } else if mailbox == folders.trash {
        "trash"
    } else if mailbox == folders.spam {
        "spam"
    } else {
        "archive"
    }
}

pub fn path_of<'a>(folders: &'a ServerFolders, folder: &str) -> &'a str {
    match folder {
        "inbox" => &folders.inbox,
        "spam" => &folders.spam,
        "trash" => &folders.trash,
        _ => &folders.archive,
    }
}

/// "1,2,3,5,7,8,9" -> "1:3,5,7:9", żeby duże zaznaczenia mieściły się w jednej komendzie IMAP
pub fn pack_uids(uids: &[u32]) -> String {
    let mut sorted: Vec<u32> = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        while i + 1 < sorted.len() && sorted[i + 1] == sorted[i] + 1 {
            i += 1;
        }
        parts.push(if start == sorted[i] { start.to_string() } else { format!("{start}:{}", sorted[i]) });
        i += 1;
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_archive_is_all_mail() {
        let list = [
            ListedMailbox::new("INBOX", Some("\\Inbox")),
            ListedMailbox::new("[Gmail]/All Mail", Some("\\All")),
            ListedMailbox::new("[Gmail]/Spam", Some("\\Junk")),
            ListedMailbox::new("[Gmail]/Trash", Some("\\Trash")),
        ];
        let (f, missing) = pick_folders(&list, true);
        assert_eq!((f.archive.as_str(), f.spam.as_str(), f.trash.as_str()), ("[Gmail]/All Mail", "[Gmail]/Spam", "[Gmail]/Trash"));
        assert!(missing.is_empty());
    }

    #[test]
    fn other_server_reports_missing_folders() {
        let list = [
            ListedMailbox::new("INBOX", None),
            ListedMailbox::new("Kosz", Some("\\Trash")),
            ListedMailbox::new("Wszystkie", Some("\\All")),
        ];
        let (f, missing) = pick_folders(&list, false);
        assert_eq!((f.trash.as_str(), f.archive.as_str()), ("Kosz", "Archive"));
        assert_eq!(missing, [Missing::Archive, Missing::Spam]);
    }

    #[test]
    fn folder_mapping() {
        let list = [
            ListedMailbox::new("Junk", Some("\\Junk")),
            ListedMailbox::new("Trash", Some("\\Trash")),
            ListedMailbox::new("Archive", Some("\\Archive")),
        ];
        let (f, _) = pick_folders(&list, false);
        assert_eq!(folder_of(&f, "INBOX"), "inbox");
        assert_eq!(folder_of(&f, "Junk"), "spam");
        assert_eq!(folder_of(&f, "Trash"), "trash");
        assert_eq!(folder_of(&f, "Archive"), "archive");
        assert_eq!(folder_of(&f, "Projekty/2026"), "archive");
        assert_eq!(path_of(&f, "spam"), "Junk");
    }

    #[test]
    fn packs_uid_ranges() {
        assert_eq!(pack_uids(&[9, 1, 2, 3, 5, 7, 8, 3]), "1:3,5,7:9");
        assert_eq!(pack_uids(&[42]), "42");
        assert_eq!(pack_uids(&[]), "");
    }
}
