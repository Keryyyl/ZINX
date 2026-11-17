//! Cache pobranych załączników. To kopia robocza, którą zawsze da się odtworzyć z serwera,
//! więc starsze pliki (wg ostatniego użycia) są usuwane po przekroczeniu limitu.

use super::attachments::safe_filename;
use super::session::ImapSession;
use crate::db::attachments::AttachmentLocation;
use crate::error::{Error, Result};
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub const CACHE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

pub fn cache_path(root: &Path, a: &AttachmentLocation) -> PathBuf {
    root.join(a.account_id.to_string()).join(a.message_id.to_string()).join(format!("{}-{}", a.id, safe_filename(&a.filename)))
}

#[derive(Clone)]
pub struct AttachmentCache {
    root: PathBuf,
    limit: u64,
    // dwa kliknięcia w ten sam załącznik czekają na jedno pobranie
    locks: Arc<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>>,
}

impl AttachmentCache {
    pub fn new(root: PathBuf, limit: u64) -> Self {
        AttachmentCache { root, limit, locks: Arc::default() }
    }

    /// Zwraca ścieżkę pliku w cache; jeśli go nie ma, pobiera z serwera tylko tę jedną część MIME.
    /// `connect` wołamy dopiero wtedy, gdy pobieranie jest potrzebne.
    pub async fn get(
        &self,
        att: &AttachmentLocation,
        connect: impl FnOnce() -> BoxFuture<'static, Result<Box<dyn ImapSession>>>,
    ) -> Result<PathBuf> {
        let lock = self.locks.lock().unwrap().entry(att.id).or_default().clone();
        let _guard = lock.lock().await;
        let target = cache_path(&self.root, att);
        if target.is_file() {
            touch(&target); // ostatnie użycie, dla usuwania najstarszych
            return Ok(target);
        }
        // położenie jest nieznane, gdy sync właśnie szuka przeniesionej wiadomości
        let (Some(mailbox), Some(uid)) = (&att.mailbox, att.server_uid) else {
            return Err("Wiadomość jest właśnie synchronizowana, spróbuj za chwilę".into());
        };
        let dir = target.parent().ok_or_else(|| Error::from("Nieprawidłowa ścieżka"))?;
        tokio::fs::create_dir_all(dir).await.map_err(|e| Error(format!("Nie udało się utworzyć folderu cache: {e}")))?;

        let mut session = connect().await?;
        session.select(mailbox).await?;
        let content = session.fetch_part(uid, &att.part).await;
        let _ = session.logout().await;
        let content = content?;

        let temp = target.with_extension("part");
        let written = tokio::fs::write(&temp, &content).await;
        if let Err(e) = written {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(Error(format!("Nie udało się zapisać załącznika: {e}")));
        }
        tokio::fs::rename(&temp, &target).await.map_err(|e| Error(format!("Nie udało się zapisać załącznika: {e}")))?;
        let (root, limit) = (self.root.clone(), self.limit);
        let _ = tokio::task::spawn_blocking(move || prune_cache(&root, limit)).await;
        Ok(target)
    }
}

fn touch(path: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Usuwa najdawniej używane pliki, aż cache zmieści się w 80% limitu.
pub fn prune_cache(root: &Path, limit: u64) {
    let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    fn walk(dir: &Path, files: &mut Vec<(PathBuf, u64, SystemTime)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(m) if m.is_dir() => walk(&path, files),
                Ok(m) if m.is_file() => files.push((path, m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH))),
                _ => {}
            }
        }
    }
    walk(root, &mut files);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= limit {
        return;
    }
    files.sort_by_key(|f| f.2);
    for (path, size, _) in files {
        if total as f64 <= limit as f64 * 0.8 {
            break;
        }
        let _ = std::fs::remove_file(&path);
        total -= size;
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir(dir); // tylko jeśli pusty
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::mock::MockServer;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn temp_root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zim-test-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn location(mailbox: Option<&str>, uid: Option<u32>) -> AttachmentLocation {
        AttachmentLocation { id: 7, message_id: 3, account_id: 1, part: "2".into(), filename: "../CV żółte.pdf".into(), mailbox: mailbox.map(str::to_string), server_uid: uid }
    }

    fn connector(server: &MockServer, counter: Arc<AtomicUsize>) -> impl FnOnce() -> BoxFuture<'static, Result<Box<dyn ImapSession>>> {
        let server = server.clone();
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(Box::new(server.session()) as Box<dyn ImapSession>) })
        }
    }

    #[tokio::test]
    async fn downloads_once_and_reuses_the_file() {
        let root = temp_root("cache");
        let server = MockServer::new();
        server.add("INBOX", "treść załącznika", false);
        server.0.lock().unwrap().parts.insert(("INBOX".into(), 1, "2".into()), b"PDF-DATA".to_vec());
        let cache = AttachmentCache::new(root.clone(), CACHE_LIMIT);
        let att = location(Some("INBOX"), Some(1));
        let connects = Arc::new(AtomicUsize::new(0));

        let path = cache.get(&att, connector(&server, connects.clone())).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"PDF-DATA");
        assert!(path.starts_with(&root) && path.file_name().unwrap().to_string_lossy().starts_with("7-"), "{path:?}");
        assert!(!path.to_string_lossy().contains(".."), "nazwa z maila nie może wyjść poza cache");
        let again = cache.get(&att, connector(&server, connects.clone())).await.unwrap();
        assert_eq!((again, connects.load(Ordering::SeqCst), server.0.lock().unwrap().part_fetches), (path, 1, 1));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_download() {
        let root = temp_root("shared");
        let server = MockServer::new();
        server.add("INBOX", "x", false);
        let cache = AttachmentCache::new(root.clone(), CACHE_LIMIT);
        let att = location(Some("INBOX"), Some(1));
        let connects = Arc::new(AtomicUsize::new(0));
        let (a, b) = tokio::join!(cache.get(&att, connector(&server, connects.clone())), cache.get(&att, connector(&server, connects.clone())));
        assert_eq!(a.unwrap(), b.unwrap());
        assert_eq!(server.0.lock().unwrap().part_fetches, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn unknown_location_and_server_errors_are_reported() {
        let root = temp_root("errors");
        let server = MockServer::new();
        let cache = AttachmentCache::new(root.clone(), CACHE_LIMIT);
        let connects = Arc::new(AtomicUsize::new(0));
        let err = cache.get(&location(None, None), connector(&server, connects.clone())).await.unwrap_err();
        assert!(err.0.contains("synchronizowana"));
        assert_eq!(connects.load(Ordering::SeqCst), 0, "bez znanego położenia nie łączymy się z serwerem");
        let err = cache.get(&location(Some("INBOX"), Some(99)), connector(&server, connects.clone())).await.unwrap_err();
        assert!(err.0.contains("Nie ma takiej wiadomości"));
        let leftovers: Vec<_> = walk_files(&root);
        assert!(leftovers.is_empty(), "nieudane pobranie nie zostawia plików: {leftovers:?}");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn walk_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                out.extend(walk_files(&e.path()));
            } else {
                out.push(e.path());
            }
        }
        out
    }

    #[test]
    fn prune_removes_least_recently_used_first() {
        let root = temp_root("prune");
        let dir = root.join("1").join("10");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, age_days) in [("stary", 3u64), ("sredni", 2), ("nowy", 1)] {
            let path = dir.join(name);
            std::fs::write(&path, vec![0u8; 100]).unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(age_days * 86_400)).unwrap();
        }
        // 300 B, limit 250 B -> cel 200 B: znika najstarszy
        prune_cache(&root, 250);
        let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, ["nowy", "sredni"]);
        prune_cache(&root, 1000);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
}
