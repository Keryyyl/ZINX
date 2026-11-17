//! Hasła i klucze API tylko w systemowym schowku (Keychain / Menedżer poświadczeń), nigdy w bazie ani w logach.
//!
//! Wszystkie sekrety leżą w JEDNYM wpisie schowka (mapa nazwa -> wartość jako JSON) i są czytane raz na uruchomienie.
//! Dzięki temu system pyta o dostęp najwyżej raz, a nie osobno dla każdego konta i klucza. Dawne, osobne wpisy
//! (po jednym na sekret) są przenoszone do wspólnego przy pierwszym odczycie.

use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::sync::Mutex;

const SERVICE: &str = "app.zim.mail";
/// Nazwa wspólnego wpisu; nie koliduje z kluczami sekretów (`account:N`, `ai:dostawca`)
const BLOB: &str = "zim-secrets";

/// Miejsce, w którym fizycznie leżą dane; w testach zastępuje je pamięć.
pub trait Backend {
    fn read_blob(&self) -> Result<Option<String>>;
    fn write_blob(&self, json: &str) -> Result<()>;
    /// Dawny, osobny wpis jednego sekretu
    fn read_legacy(&self, key: &str) -> Result<Option<String>>;
    fn delete_legacy(&self, key: &str) -> Result<()>;
}

pub struct Vault<B: Backend> {
    backend: B,
    cache: Mutex<Option<BTreeMap<String, String>>>,
}

impl<B: Backend> Vault<B> {
    pub const fn new(backend: B) -> Self {
        Vault { backend, cache: Mutex::new(None) }
    }

    fn with_map<T>(&self, f: impl FnOnce(&mut BTreeMap<String, String>) -> Result<(T, bool)>) -> Result<T> {
        let mut guard = self.cache.lock().map_err(|_| Error::from("Schowek jest niedostępny"))?;
        if guard.is_none() {
            let map = match self.backend.read_blob()? {
                Some(json) => serde_json::from_str(&json).map_err(|_| Error::from("Wpis w schowku jest uszkodzony"))?,
                None => BTreeMap::new(),
            };
            *guard = Some(map);
        }
        let map = guard.as_mut().expect("mapa wczytana wyżej");
        let (value, changed) = f(map)?;
        if changed {
            self.backend.write_blob(&serde_json::to_string(map)?)?;
        }
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let legacy = || self.backend.read_legacy(key);
        self.with_map(|map| {
            if let Some(v) = map.get(key) {
                return Ok((Some(v.clone()), false));
            }
            // dawny wpis: przenosimy do wspólnego i usuwamy osobny
            match legacy()? {
                Some(v) => {
                    map.insert(key.to_string(), v.clone());
                    Ok((Some(v), true))
                }
                None => Ok((None, false)),
            }
        })
        .inspect(|found| {
            if found.is_some() {
                let _ = self.backend.delete_legacy(key);
            }
        })
    }

    pub fn set(&self, key: &str, secret: &str) -> Result<()> {
        self.with_map(|map| {
            let changed = map.get(key).map(String::as_str) != Some(secret);
            map.insert(key.to_string(), secret.to_string());
            Ok(((), changed))
        })?;
        let _ = self.backend.delete_legacy(key);
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        self.with_map(|map| Ok(((), map.remove(key).is_some())))?;
        self.backend.delete_legacy(key)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod keychain {
    use super::*;

    fn entry(name: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, name).map_err(|e| Error(format!("Schowek systemowy: {e}")))
    }

    pub struct Keychain;

    impl Backend for Keychain {
        fn read_blob(&self) -> Result<Option<String>> {
            read(BLOB)
        }
        fn write_blob(&self, json: &str) -> Result<()> {
            entry(BLOB)?.set_password(json).map_err(|e| Error(format!("Nie udało się zapisać w schowku: {e}")))
        }
        fn read_legacy(&self, key: &str) -> Result<Option<String>> {
            read(key)
        }
        fn delete_legacy(&self, key: &str) -> Result<()> {
            match entry(key)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(Error(format!("Nie udało się usunąć ze schowka: {e}"))),
            }
        }
    }

    fn read(name: &str) -> Result<Option<String>> {
        match entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error(format!("Nie udało się odczytać ze schowka: {e}"))),
        }
    }
}

// iOS i Android: schowek dochodzi razem z wersją mobilną
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod keychain {
    use super::*;

    pub struct Keychain;

    impl Backend for Keychain {
        fn read_blob(&self) -> Result<Option<String>> {
            Ok(None)
        }
        fn write_blob(&self, _: &str) -> Result<()> {
            Err("Schowek systemowy nie jest jeszcze dostępny na tej platformie".into())
        }
        fn read_legacy(&self, _: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn delete_legacy(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }
}

static VAULT: Vault<keychain::Keychain> = Vault::new(keychain::Keychain);

pub fn set(key: &str, secret: &str) -> Result<()> {
    VAULT.set(key, secret)
}

pub fn get(key: &str) -> Result<Option<String>> {
    VAULT.get(key)
}

pub fn delete(key: &str) -> Result<()> {
    VAULT.delete(key)
}

pub fn account_key(id: i64) -> String {
    format!("account:{id}")
}

pub fn ai_key(provider: &str) -> String {
    format!("ai:{provider}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Fake {
        blob: Arc<Mutex<Option<String>>>,
        legacy: Arc<Mutex<HashMap<String, String>>>,
        blob_reads: Arc<Mutex<usize>>,
        legacy_reads: Arc<Mutex<usize>>,
        blob_writes: Arc<Mutex<usize>>,
    }

    impl Backend for Fake {
        fn read_blob(&self) -> Result<Option<String>> {
            *self.blob_reads.lock().unwrap() += 1;
            Ok(self.blob.lock().unwrap().clone())
        }
        fn write_blob(&self, json: &str) -> Result<()> {
            *self.blob_writes.lock().unwrap() += 1;
            *self.blob.lock().unwrap() = Some(json.to_string());
            Ok(())
        }
        fn read_legacy(&self, key: &str) -> Result<Option<String>> {
            *self.legacy_reads.lock().unwrap() += 1;
            Ok(self.legacy.lock().unwrap().get(key).cloned())
        }
        fn delete_legacy(&self, key: &str) -> Result<()> {
            self.legacy.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[test]
    fn keychain_is_read_once_per_run_no_matter_how_many_secrets() {
        let fake = Fake::default();
        *fake.blob.lock().unwrap() = Some(r#"{"account:1":"a","account:2":"b","ai:openai":"k"}"#.into());
        let vault = Vault::new(fake.clone());
        for key in ["account:1", "account:2", "ai:openai", "account:1", "account:2"] {
            assert!(vault.get(key).unwrap().is_some());
        }
        assert_eq!(*fake.blob_reads.lock().unwrap(), 1, "jedno odczytanie wpisu = jedno pytanie systemu");
        assert_eq!(*fake.legacy_reads.lock().unwrap(), 0);
    }

    #[test]
    fn set_get_delete_roundtrip_persists_in_the_single_entry() {
        let fake = Fake::default();
        let vault = Vault::new(fake.clone());
        vault.set("account:1", "hasło").unwrap();
        vault.set("ai:anthropic", "sk-ant-123").unwrap();
        assert_eq!(vault.get("account:1").unwrap().as_deref(), Some("hasło"));
        // nowy proces widzi to samo
        let next = Vault::new(fake.clone());
        assert_eq!(next.get("ai:anthropic").unwrap().as_deref(), Some("sk-ant-123"));
        next.delete("ai:anthropic").unwrap();
        assert_eq!(Vault::new(fake.clone()).get("ai:anthropic").unwrap(), None);
        assert_eq!(Vault::new(fake).get("account:1").unwrap().as_deref(), Some("hasło"));
    }

    #[test]
    fn setting_the_same_value_does_not_rewrite_the_entry() {
        let fake = Fake::default();
        let vault = Vault::new(fake.clone());
        vault.set("account:1", "x").unwrap();
        vault.set("account:1", "x").unwrap();
        assert_eq!(*fake.blob_writes.lock().unwrap(), 1);
        vault.set("account:1", "y").unwrap();
        assert_eq!(*fake.blob_writes.lock().unwrap(), 2);
    }

    #[test]
    fn legacy_entries_are_moved_into_the_shared_one_on_first_read() {
        let fake = Fake::default();
        fake.legacy.lock().unwrap().extend([("account:1".to_string(), "stare1".to_string()), ("account:2".to_string(), "stare2".to_string())]);
        let vault = Vault::new(fake.clone());
        assert_eq!(vault.get("account:1").unwrap().as_deref(), Some("stare1"));
        assert_eq!(fake.legacy.lock().unwrap().keys().collect::<Vec<_>>(), ["account:2"], "przeniesiony wpis znika z osobnych");
        // drugi odczyt już bez dawnego wpisu
        let before = *fake.legacy_reads.lock().unwrap();
        assert_eq!(vault.get("account:1").unwrap().as_deref(), Some("stare1"));
        assert_eq!(*fake.legacy_reads.lock().unwrap(), before);
        assert_eq!(Vault::new(fake.clone()).get("account:2").unwrap().as_deref(), Some("stare2"));
        assert_eq!(Vault::new(fake).get("account:9").unwrap(), None);
    }

    #[test]
    fn corrupted_entry_is_an_error_not_silent_data_loss() {
        let fake = Fake::default();
        *fake.blob.lock().unwrap() = Some("to nie json".into());
        let vault = Vault::new(fake.clone());
        assert!(vault.get("account:1").is_err());
        assert!(vault.set("account:1", "x").is_err());
        assert_eq!(fake.blob.lock().unwrap().as_deref(), Some("to nie json"), "uszkodzonego wpisu nie nadpisujemy");
    }
}
