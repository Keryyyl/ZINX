use crate::ai::{default_model, is_cloud, is_provider, CLOUD_PROVIDERS};
use crate::error::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
}

pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| r.get(0)).optional()?)
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Model zapamiętany osobno dla każdego dostawcy.
pub fn model_for(conn: &Connection, provider: &str) -> Result<String> {
    Ok(get(conn, &format!("ai.model.{provider}"))?.unwrap_or_else(|| default_model(provider).to_string()))
}

pub fn get_ai_config(conn: &Connection) -> Result<AiConfig> {
    let stored = get(conn, "ai.provider")?;
    let provider = stored.filter(|p| is_provider(p)).unwrap_or_else(|| "ollama".to_string());
    // wyłączone dopiero po jawnym wyborze „Brak”; domyślnie asystent działa lokalnie
    let enabled = get(conn, "ai.enabled")?.as_deref() != Some("0");
    let model = model_for(conn, &provider)?;
    Ok(AiConfig { enabled, provider, model })
}

pub fn get_consent(conn: &Connection, provider: &str) -> Result<bool> {
    Ok(get(conn, &format!("ai.consent.{provider}"))?.as_deref() == Some("1"))
}

pub fn consent_by_provider(conn: &Connection) -> Result<BTreeMap<String, bool>> {
    CLOUD_PROVIDERS.iter().map(|p| Ok((p.to_string(), get_consent(conn, p)?))).collect()
}

pub fn models_by_provider(conn: &Connection) -> Result<BTreeMap<String, String>> {
    ["ollama", "anthropic", "openai"].iter().map(|p| Ok((p.to_string(), model_for(conn, p)?))).collect()
}

#[derive(Debug, Deserialize)]
pub struct AiConfigInput {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub consent: Option<bool>,
}

fn valid_model(model: &str) -> bool {
    (1..=100).contains(&model.chars().count())
        && model.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ':' | '/'))
}

/// Zapis ustawień asystenta. Dostawca w chmurze wymaga zgody na wysyłanie treści maili poza komputer:
/// `consent: true` przychodzi z interfejsu dopiero po jej zaznaczeniu i jest zapamiętywana dla dostawcy.
pub fn set_ai_config(conn: &Connection, input: &AiConfigInput) -> Result<AiConfig> {
    if !is_provider(&input.provider) {
        return Err("Nieznany dostawca modelu".into());
    }
    if !valid_model(&input.model) {
        return Err("Nieprawidłowa nazwa modelu".into());
    }
    if input.enabled && is_cloud(&input.provider) && !get_consent(conn, &input.provider)? {
        if input.consent != Some(true) {
            return Err(Error::from("Najpierw potwierdź zgodę na wysyłanie treści maili do dostawcy"));
        }
        set(conn, &format!("ai.consent.{}", input.provider), "1")?;
    }
    set(conn, "ai.enabled", if input.enabled { "1" } else { "0" })?;
    set(conn, "ai.provider", &input.provider)?;
    set(conn, &format!("ai.model.{}", input.provider), &input.model)?;
    get_ai_config(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_memory;

    fn input(enabled: bool, provider: &str, consent: Option<bool>) -> AiConfigInput {
        AiConfigInput { enabled, provider: provider.into(), model: "m1".into(), consent }
    }

    #[test]
    fn defaults_to_local_ollama() {
        let db = open_memory();
        let cfg = get_ai_config(&db).unwrap();
        assert_eq!(cfg, AiConfig { enabled: true, provider: "ollama".into(), model: "gemma3:4b".into() });
    }

    #[test]
    fn cloud_requires_consent_once() {
        let db = open_memory();
        assert!(set_ai_config(&db, &input(true, "anthropic", None)).is_err());
        assert!(set_ai_config(&db, &input(true, "anthropic", Some(true))).is_ok());
        assert!(get_consent(&db, "anthropic").unwrap());
        assert!(set_ai_config(&db, &input(true, "anthropic", None)).is_ok());
        assert!(!get_consent(&db, "openai").unwrap());
    }

    #[test]
    fn rejects_bad_input() {
        let db = open_memory();
        assert!(set_ai_config(&db, &input(true, "gemini", None)).is_err());
        let mut bad = input(true, "ollama", None);
        bad.model = "zły model!".into();
        assert!(set_ai_config(&db, &bad).is_err());
    }

    #[test]
    fn model_is_remembered_per_provider() {
        let db = open_memory();
        set_ai_config(&db, &input(true, "ollama", None)).unwrap();
        assert_eq!(model_for(&db, "ollama").unwrap(), "m1");
        assert_eq!(model_for(&db, "openai").unwrap(), "gpt-4o-mini");
    }
}
