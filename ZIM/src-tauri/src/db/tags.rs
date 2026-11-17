use super::placeholders;
use crate::error::{Error, Result};
use crate::priority::SYSTEM_TAGS;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::collections::HashMap;

const NAME_MAX: usize = 30;
const DESCRIPTION_MAX: usize = 200;
const COLUMNS: &str = "id, name, color, description, system_key";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    /// Opis dla asystenta AI; pusty = model zna tylko nazwę
    pub description: String,
    /// Wbudowany tag ważności (urgent/loose/keep): nie da się go usunąć, ale można zmienić nazwę, kolor i opis
    pub system: Option<String>,
}

fn from_row(r: &Row) -> rusqlite::Result<Tag> {
    Ok(Tag { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, description: r.get(3)?, system: r.get(4)? })
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_name(name: &str) -> Result<String> {
    let value = collapse_spaces(name);
    if value.is_empty() {
        return Err("Podaj nazwę tagu".into());
    }
    if value.chars().count() > NAME_MAX {
        return Err(Error(format!("Nazwa tagu może mieć najwyżej {NAME_MAX} znaków")));
    }
    Ok(value)
}

/// Tylko #rrggbb; cokolwiek innego = brak koloru.
fn clean_color(color: Option<&str>) -> Option<String> {
    color
        .filter(|c| c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit()))
        .map(str::to_lowercase)
}

fn clean_description(description: &str) -> Result<String> {
    let value = collapse_spaces(description);
    if value.chars().count() > DESCRIPTION_MAX {
        return Err(Error(format!("Opis tagu może mieć najwyżej {DESCRIPTION_MAX} znaków")));
    }
    Ok(value)
}

fn assert_free(conn: &Connection, name: &str, except_id: Option<i64>) -> Result<()> {
    let taken: Option<i64> = conn
        .query_row("SELECT id FROM tags WHERE lower(name) = lower(?) AND id IS NOT ?", params![name, except_id], |r| r.get(0))
        .optional()?;
    if taken.is_some() {
        return Err("Tag o takiej nazwie już istnieje".into());
    }
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM tags ORDER BY position, id"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Przesuwa tag o jedno miejsce w górę lub w dół listy (kolejność z Ustawień i paska bocznego).
pub fn move_tag(conn: &Connection, id: i64, direction: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let mut ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT id FROM tags ORDER BY position, id")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let Some(from) = ids.iter().position(|&t| t == id) else { return Ok(()) };
    let to = match direction {
        "up" if from > 0 => from - 1,
        "down" if from + 1 < ids.len() => from + 1,
        _ => return Ok(()),
    };
    ids.swap(from, to);
    for (i, tag_id) in ids.iter().enumerate() {
        tx.execute("UPDATE tags SET position = ? WHERE id = ?", params![i as i64 + 1, tag_id])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn create(conn: &Connection, name: &str, color: Option<&str>, description: &str) -> Result<Tag> {
    let name = clean_name(name)?;
    assert_free(conn, &name, None)?;
    let color = clean_color(color);
    let description = clean_description(description)?;
    conn.execute(
        "INSERT INTO tags (name, color, description, position) VALUES (?, ?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM tags))",
        params![name, color, description],
    )?;
    Ok(Tag { id: conn.last_insert_rowid(), name, color, description, system: None })
}

/// Bez `description` (None) opis zostaje taki, jaki był.
pub fn update(conn: &Connection, id: i64, name: &str, color: Option<&str>, description: Option<&str>) -> Result<Tag> {
    let name = clean_name(name)?;
    assert_free(conn, &name, Some(id))?;
    let color = clean_color(color);
    let changes = match description {
        None => conn.execute("UPDATE tags SET name = ?, color = ? WHERE id = ?", params![name, color, id])?,
        Some(d) => conn.execute(
            "UPDATE tags SET name = ?, color = ?, description = ? WHERE id = ?",
            params![name, color, clean_description(d)?, id],
        )?,
    };
    if changes == 0 {
        return Err("Tag już nie istnieje".into());
    }
    Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM tags WHERE id = ?"), [id], from_row)?)
}

/// Usuwa tag razem z jego przypisaniami do wiadomości (ON DELETE CASCADE). Wbudowanych tagów ważności nie usuwa.
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let system: Option<Option<String>> =
        conn.query_row("SELECT system_key FROM tags WHERE id = ?", [id], |r| r.get(0)).optional()?;
    if let Some(Some(_)) = system {
        return Err("Tag wbudowany: można zmienić jego nazwę, kolor i opis, ale nie usunąć".into());
    }
    conn.execute("DELETE FROM tags WHERE id = ?", [id])?;
    Ok(())
}

/// Wbudowane tagi ważności istnieją zawsze. Przy pierwszym uruchomieniu powstają z domyślnymi opisami;
/// w istniejącej bazie przejmujemy tag o tej samej nazwie (z jego kolorem i opisem), a brakujące tworzymy.
pub fn ensure_system_tags(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for def in &SYSTEM_TAGS {
        let exists: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM tags WHERE system_key = ?)", [def.key], |r| r.get(0))?;
        if exists {
            continue;
        }
        let same: Option<i64> = tx
            .query_row("SELECT id FROM tags WHERE lower(name) = lower(?) AND system_key IS NULL", [def.name], |r| r.get(0))
            .optional()?;
        match same {
            Some(id) => tx.execute(
                "UPDATE tags SET system_key = ?, color = COALESCE(color, ?),
                   description = CASE WHEN description = '' THEN ? ELSE description END WHERE id = ?",
                params![def.key, def.color, def.description, id],
            )?,
            None => tx.execute(
                "INSERT INTO tags (name, color, description, position, system_key)
                 VALUES (?, ?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM tags), ?)",
                params![def.name, def.color, def.description, def.key],
            )?,
        };
    }
    tx.commit()?;
    Ok(())
}

/// Dodaje albo zdejmuje istniejący tag; tagów nie da się tu tworzyć.
pub fn set_message_tag(conn: &Connection, message_ids: &[i64], tag_id: i64, on: bool) -> Result<()> {
    let exists: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM tags WHERE id = ?)", [tag_id], |r| r.get(0))?;
    if !exists {
        return Err("Tag już nie istnieje".into());
    }
    let tx = conn.unchecked_transaction()?;
    for id in message_ids {
        if on {
            tx.execute("INSERT OR IGNORE INTO message_tags (message_id, tag_id, source) VALUES (?, ?, 'manual')", params![id, tag_id])?;
        } else {
            tx.execute("DELETE FROM message_tags WHERE message_id = ? AND tag_id = ?", params![id, tag_id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Tagi podanych wiadomości, posortowane po nazwie.
pub fn by_message(conn: &Connection, ids: &[i64]) -> Result<HashMap<i64, Vec<Tag>>> {
    let mut result: HashMap<i64, Vec<Tag>> = HashMap::new();
    for chunk in ids.chunks(500) {
        let sql = format!(
            "SELECT mt.message_id, t.id, t.name, t.color, t.description, t.system_key
             FROM message_tags mt JOIN tags t ON t.id = mt.tag_id
             WHERE mt.message_id IN ({}) ORDER BY lower(t.name)",
            placeholders(chunk.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk), |r| {
            Ok((r.get::<_, i64>(0)?, Tag { id: r.get(1)?, name: r.get(2)?, color: r.get(3)?, description: r.get(4)?, system: r.get(5)? }))
        })?;
        for row in rows {
            let (message_id, tag) = row?;
            result.entry(message_id).or_default().push(tag);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    fn by_key(db: &Connection, key: &str) -> Tag {
        list(db).unwrap().into_iter().find(|t| t.system.as_deref() == Some(key)).unwrap()
    }

    #[test]
    fn system_tags_exist_from_first_start() {
        let db = open_memory();
        let names: Vec<_> = list(&db).unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["Pilne", "Luźne", "Zachować"]);
        assert!(!by_key(&db, "urgent").description.is_empty());
    }

    #[test]
    fn ensure_adopts_existing_tag_by_name_and_is_idempotent() {
        let db = open_memory();
        db.execute("DELETE FROM tags", []).unwrap();
        db.execute("INSERT INTO tags (name, color, description) VALUES ('pilne', NULL, 'mój opis')", []).unwrap();
        ensure_system_tags(&db).unwrap();
        ensure_system_tags(&db).unwrap();
        let urgent = by_key(&db, "urgent");
        assert_eq!((urgent.name.as_str(), urgent.description.as_str()), ("pilne", "mój opis"));
        assert_eq!(urgent.color.as_deref(), Some("#ef4444"));
        assert_eq!(list(&db).unwrap().len(), 3);
    }

    #[test]
    fn system_tag_can_be_renamed_but_not_deleted() {
        let db = open_memory();
        let urgent = by_key(&db, "urgent");
        let renamed = update(&db, urgent.id, "ASAP", Some("#00ff00"), Some("opis")).unwrap();
        assert_eq!((renamed.name.as_str(), renamed.system.as_deref()), ("ASAP", Some("urgent")));
        assert!(delete(&db, urgent.id).is_err());
        assert_eq!(by_key(&db, "urgent").id, urgent.id);
    }

    #[test]
    fn create_validates_and_rejects_duplicates() {
        let db = open_memory();
        let tag = create(&db, "  Praca   dodatkowa ", Some("#ABCDEF"), "").unwrap();
        assert_eq!((tag.name.as_str(), tag.color.as_deref()), ("Praca dodatkowa", Some("#abcdef")));
        assert!(create(&db, "praca dodatkowa", None, "").is_err());
        assert!(create(&db, "  ", None, "").is_err());
        assert_eq!(create(&db, "x", Some("red"), "").unwrap().color, None);
        assert!(create(&db, &"a".repeat(31), None, "").is_err());
    }

    #[test]
    fn update_without_description_keeps_it() {
        let db = open_memory();
        let tag = create(&db, "A", None, "opis").unwrap();
        assert_eq!(update(&db, tag.id, "B", None, None).unwrap().description, "opis");
        assert!(update(&db, 9999, "C", None, None).is_err());
    }

    #[test]
    fn move_changes_order() {
        let db = open_memory();
        let first = list(&db).unwrap()[0].id;
        move_tag(&db, first, "down").unwrap();
        assert_eq!(list(&db).unwrap()[1].id, first);
        move_tag(&db, first, "up").unwrap();
        assert_eq!(list(&db).unwrap()[0].id, first);
        move_tag(&db, first, "up").unwrap();
        assert_eq!(list(&db).unwrap()[0].id, first);
    }
}
