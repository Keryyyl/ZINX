use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ContactService {
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Contact {
    /// None = jeszcze niezapisany w bazie
    #[serde(default)]
    pub id: Option<i64>,
    /// Zdjęcie: data URL; None = brak
    #[serde(default)]
    pub photo: Option<String>,
    /// Jeden napis: "Imię Nazwisko" albo nazwa firmy
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub services: Vec<ContactService>,
}

/// Ostatnia wiadomość wymieniona z kontaktem, w którąkolwiek stronę
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ContactMail {
    pub id: i64,
    pub subject: String,
    pub date: i64,
    pub direction: &'static str,
}

fn parse_services(json: &str) -> Vec<ContactService> {
    serde_json::from_str::<Vec<ContactService>>(json).unwrap_or_default()
}

/// Walidacja i normalizacja jak w klasie Contact: dane z interfejsu są niezaufane.
fn normalize(raw: Contact) -> Result<Contact> {
    let name = raw.name.trim().to_string();
    let email = raw.email.trim().to_lowercase();
    if name.is_empty() {
        return Err("Kontakt musi mieć imię i nazwisko albo nazwę firmy".into());
    }
    let valid = email.split_once('@').is_some_and(|(l, d)| !l.is_empty() && !d.is_empty() && !email.chars().any(char::is_whitespace) && !d.contains('@'));
    if !valid {
        return Err("Nieprawidłowy adres e-mail".into());
    }
    let services = raw
        .services
        .into_iter()
        .map(|s| ContactService { service: s.service.trim().to_string(), username: s.username.trim().to_string() })
        .filter(|s| !s.service.is_empty() && !s.username.is_empty())
        .collect();
    Ok(Contact {
        id: raw.id.filter(|id| *id > 0),
        photo: raw.photo,
        name,
        email,
        city: raw.city.trim().to_string(),
        description: raw.description.trim().to_string(),
        services,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<Contact>> {
    let mut stmt = conn.prepare("SELECT id, photo, name, email, city, description, services FROM contacts ORDER BY lower(name)")?;
    let rows = stmt.query_map([], |r| {
        let services: String = r.get(6)?;
        Ok(Contact {
            id: Some(r.get(0)?),
            photo: r.get(1)?,
            name: r.get(2)?,
            email: r.get(3)?,
            city: r.get(4)?,
            description: r.get(5)?,
            services: parse_services(&services),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Dodaje (bez id) albo aktualizuje kontakt.
pub fn save(conn: &Connection, input: Contact) -> Result<Contact> {
    let mut contact = normalize(input)?;
    let services = serde_json::to_string(&contact.services)?;
    match contact.id {
        None => {
            conn.execute(
                "INSERT INTO contacts (photo, name, email, city, description, services) VALUES (?, ?, ?, ?, ?, ?)",
                params![contact.photo, contact.name, contact.email, contact.city, contact.description, services],
            )?;
            contact.id = Some(conn.last_insert_rowid());
        }
        Some(id) => {
            let changes = conn.execute(
                "UPDATE contacts SET photo = ?, name = ?, email = ?, city = ?, description = ?, services = ? WHERE id = ?",
                params![contact.photo, contact.name, contact.email, contact.city, contact.description, services, id],
            )?;
            if changes == 0 {
                return Err("Kontakt już nie istnieje".into());
            }
        }
    }
    Ok(contact)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM contacts WHERE id = ?", [id])?;
    Ok(())
}

/// Ostatnie wiadomości wymieniane z kontaktem: od niego albo do niego, ze wszystkich kont.
pub fn mail(conn: &Connection, contact_id: i64, limit: i64) -> Result<Vec<ContactMail>> {
    let email: Option<String> = conn.query_row("SELECT email FROM contacts WHERE id = ?", [contact_id], |r| r.get(0)).optional()?;
    let Some(email) = email else { return Ok(Vec::new()) };
    // instr zamiast LIKE: znaki % i _ w adresie nie mogą działać jak wzorzec
    let mut stmt = conn.prepare(
        "SELECT id, subject, date, instr(lower(from_addr), ?1) > 0
         FROM messages
         WHERE folder != 'deleted' AND (instr(lower(from_addr), ?1) > 0 OR instr(lower(to_addr), ?1) > 0)
         ORDER BY date DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![email, limit], |r| {
        let received: bool = r.get(3)?;
        Ok(ContactMail { id: r.get(0)?, subject: r.get(1)?, date: r.get(2)?, direction: if received { "received" } else { "sent" } })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    fn contact(name: &str, email: &str) -> Contact {
        Contact { id: None, photo: None, name: name.into(), email: email.into(), city: String::new(), description: String::new(), services: vec![] }
    }

    #[test]
    fn save_normalizes_and_updates() {
        let db = open_memory();
        let mut c = contact("  Jan Kowalski ", " Jan@X.pl ");
        c.services = vec![
            ContactService { service: "GitHub".into(), username: "jan".into() },
            ContactService { service: "".into(), username: "x".into() },
        ];
        let saved = save(&db, c).unwrap();
        assert_eq!((saved.name.as_str(), saved.email.as_str(), saved.services.len()), ("Jan Kowalski", "jan@x.pl", 1));
        let mut again = saved.clone();
        again.city = "Łódź".into();
        save(&db, again).unwrap();
        let all = list(&db).unwrap();
        assert_eq!((all.len(), all[0].city.as_str()), (1, "Łódź"));
        assert!(save(&db, Contact { id: Some(999), ..contact("A", "a@b.pl") }).is_err());
    }

    #[test]
    fn rejects_invalid() {
        let db = open_memory();
        assert!(save(&db, contact("", "a@b.pl")).is_err());
        assert!(save(&db, contact("A", "bez-malpy")).is_err());
    }

    #[test]
    fn mail_with_contact_both_directions() {
        let db = open_memory();
        db.execute("INSERT INTO accounts (email, imap_host, username) VALUES ('me@x.pl', 'imap.x.pl', 'me')", []).unwrap();
        for (uid, from, to, date) in [(1, "Jan <jan@x.pl>", "me@x.pl", 100), (2, "me@x.pl", "jan@x.pl", 200), (3, "inny@x.pl", "me@x.pl", 300)] {
            db.execute(
                "INSERT INTO messages (account_id, uid, from_addr, to_addr, subject, date) VALUES (1, ?, ?, ?, 's', ?)",
                params![uid, from, to, date],
            )
            .unwrap();
        }
        let saved = save(&db, contact("Jan", "jan@x.pl")).unwrap();
        let mails = mail(&db, saved.id.unwrap(), 3).unwrap();
        assert_eq!(mails.iter().map(|m| (m.date, m.direction)).collect::<Vec<_>>(), [(200, "sent"), (100, "received")]);
    }
}
