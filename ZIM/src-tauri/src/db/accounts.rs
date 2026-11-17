use crate::error::{Error, Result};
use crate::smtp::guess_smtp;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const MAX_ALIAS: usize = 40;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub email: String,
    /// Nazwa skrzynki widoczna w UI zamiast adresu, np. "Praca"
    pub alias: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
}

/// Dane z formularza; SMTP opcjonalnie (domyślnie zgadujemy z serwera IMAP). Hasło obsługuje wywołujący.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccount {
    pub email: String,
    #[serde(default)]
    pub alias: Option<String>,
    pub imap_host: String,
    pub imap_port: i64,
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default)]
    pub smtp_port: Option<i64>,
    #[serde(default)]
    pub username: String,
}

pub fn list(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare(
        "SELECT id, email, alias, imap_host, imap_port, COALESCE(smtp_host, ''), COALESCE(smtp_port, 465), username
         FROM accounts ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Account {
            id: r.get(0)?,
            email: r.get(1)?,
            alias: r.get(2)?,
            imap_host: r.get(3)?,
            imap_port: r.get(4)?,
            smtp_host: r.get(5)?,
            smtp_port: r.get(6)?,
            username: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn normalize_alias(alias: Option<&str>) -> Result<Option<String>> {
    let value = alias.map(str::trim).filter(|a| !a.is_empty());
    match value {
        Some(v) if v.chars().count() > MAX_ALIAS => {
            Err(Error(format!("Alias może mieć najwyżej {MAX_ALIAS} znaków")))
        }
        other => Ok(other.map(str::to_string)),
    }
}

fn port(value: i64) -> Option<u16> {
    u16::try_from(value).ok().filter(|p| *p >= 1)
}

fn valid_email(email: &str) -> bool {
    let mut parts = email.splitn(2, '@');
    let (Some(local), Some(domain)) = (parts.next(), parts.next()) else { return false };
    let ok = |s: &str| !s.is_empty() && !s.chars().any(char::is_whitespace);
    ok(local) && ok(domain) && !domain.contains('@') && domain.contains('.') && !domain.ends_with('.') && !domain.starts_with('.')
}

pub fn add(conn: &Connection, input: &NewAccount) -> Result<Account> {
    let email = input.email.trim().to_lowercase();
    let imap_host = input.imap_host.trim().to_string();
    let username = if input.username.trim().is_empty() { email.clone() } else { input.username.trim().to_string() };
    let alias = normalize_alias(input.alias.as_deref())?;
    if !valid_email(&email) {
        return Err("Nieprawidłowy adres e-mail".into());
    }
    if imap_host.is_empty() {
        return Err("Podaj serwer IMAP".into());
    }
    let imap_port = port(input.imap_port).ok_or("Nieprawidłowy port IMAP")?;
    let (guess_host, guess_port) = guess_smtp(&imap_host);
    let smtp_host = input.smtp_host.as_deref().map(str::trim).filter(|h| !h.is_empty()).map(str::to_string).unwrap_or(guess_host);
    let smtp_port = match input.smtp_port {
        Some(p) => port(p).ok_or("Nieprawidłowy port SMTP")?,
        None => guess_port,
    };
    let inserted = conn.execute(
        "INSERT INTO accounts (email, alias, imap_host, imap_port, smtp_host, smtp_port, username) VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![email, alias, imap_host, imap_port, smtp_host, smtp_port, username],
    );
    match inserted {
        Ok(_) => Ok(Account { id: conn.last_insert_rowid(), email, alias, imap_host, imap_port, smtp_host, smtp_port, username }),
        Err(e) if e.to_string().contains("UNIQUE") => Err("To konto jest już dodane".into()),
        Err(e) => Err(e.into()),
    }
}

/// Pusty alias = powrót do wyświetlania adresu e-mail.
pub fn set_alias(conn: &Connection, id: i64, alias: Option<&str>) -> Result<Option<String>> {
    let value = normalize_alias(alias)?;
    let changes = conn.execute("UPDATE accounts SET alias = ? WHERE id = ?", params![value, id])?;
    if changes == 0 {
        return Err("Nie ma takiego konta".into());
    }
    Ok(value)
}

pub fn remove(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM accounts WHERE id = ?", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    fn new(email: &str) -> NewAccount {
        NewAccount {
            email: email.into(),
            alias: None,
            imap_host: "imap.gmail.com".into(),
            imap_port: 993,
            smtp_host: None,
            smtp_port: None,
            username: String::new(),
        }
    }

    #[test]
    fn adds_with_guessed_smtp_and_rejects_duplicates() {
        let db = open_memory();
        let a = add(&db, &new(" Jan@Gmail.com ")).unwrap();
        assert_eq!((a.email.as_str(), a.smtp_host.as_str(), a.smtp_port), ("jan@gmail.com", "smtp.gmail.com", 465));
        assert_eq!(a.username, "jan@gmail.com");
        assert_eq!(add(&db, &new("jan@gmail.com")).unwrap_err().0, "To konto jest już dodane");
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn validates() {
        let db = open_memory();
        assert!(add(&db, &new("bez-malpy")).is_err());
        let mut bad = new("a@b.pl");
        bad.imap_port = 70000;
        assert!(add(&db, &bad).is_err());
    }

    #[test]
    fn alias_roundtrip_and_limit() {
        let db = open_memory();
        let a = add(&db, &new("a@b.pl")).unwrap();
        assert_eq!(set_alias(&db, a.id, Some("  Praca ")).unwrap().as_deref(), Some("Praca"));
        assert_eq!(set_alias(&db, a.id, Some("  ")).unwrap(), None);
        assert!(set_alias(&db, a.id, Some(&"x".repeat(41))).is_err());
        assert!(set_alias(&db, 999, Some("x")).is_err());
    }
}
