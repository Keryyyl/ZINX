//! Serwer Ollamy: przy starcie aplikacji, gdy asystent jest włączony i serwer nie odpowiada, uruchamiamy
//! `ollama serve` jako proces potomny i zatrzymujemy go przy wyjściu. Cudzego serwera (aplikacja Ollama,
//! brew services) nie ruszamy.

use super::clients::{list_models, OLLAMA_URL};
use futures::future::BoxFuture;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::{Child, Command};

/// Aplikacja uruchamiana z Findera ma ubogi PATH, więc sprawdzamy też typowe miejsca instalacji.
const KNOWN_PATHS: [&str; 3] = ["/opt/homebrew/bin/ollama", "/usr/local/bin/ollama", "/Applications/Ollama.app/Contents/Resources/ollama"];

pub fn find_binary(path_var: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let from_path = std::env::split_paths(path_var).filter(|d| !d.as_os_str().is_empty()).flat_map(|d| [d.join("ollama"), d.join("ollama.exe")]);
    from_path.chain(KNOWN_PATHS.iter().map(PathBuf::from)).find(|p| exists(p))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerResult {
    Running,
    Started,
    Missing,
    Failed,
}

#[derive(Clone)]
pub struct ServerDeps {
    pub is_up: Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>,
    pub spawn: Arc<dyn Fn(&Path) -> std::io::Result<Child> + Send + Sync>,
    pub find: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    pub poll: Duration,
}

pub fn real_deps() -> ServerDeps {
    ServerDeps {
        is_up: Arc::new(|| Box::pin(async { list_models(OLLAMA_URL).await.is_some() })),
        spawn: Arc::new(|binary| {
            Command::new(binary)
                .arg("serve")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                // te same ustawienia, które przyspieszają model w testach: flash attention i skompresowany cache KV
                .env("OLLAMA_FLASH_ATTENTION", "1")
                .env("OLLAMA_KV_CACHE_TYPE", "q8_0")
                .kill_on_drop(true)
                .spawn()
        }),
        find: Arc::new(|| find_binary(&std::env::var("PATH").unwrap_or_default(), |p| p.is_file())),
        poll: Duration::from_millis(300),
    }
}

static CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// Upewnia się, że serwer Ollamy odpowiada. Jeśli już działa, nic nie robi i go nie zatrzyma; w przeciwnym razie
/// uruchamia własny proces i czeka, aż będzie gotowy.
pub async fn ensure_running(timeout: Duration, deps: &ServerDeps) -> ServerResult {
    if (deps.is_up)().await {
        return ServerResult::Running;
    }
    let Some(binary) = (deps.find)() else { return ServerResult::Missing };
    let Ok(child) = (deps.spawn)(&binary) else { return ServerResult::Failed };
    *CHILD.lock().unwrap() = Some(child);

    let mut waited = Duration::ZERO;
    while waited < timeout {
        tokio::time::sleep(deps.poll).await;
        waited += deps.poll;
        if (deps.is_up)().await {
            return ServerResult::Started;
        }
        let exited = CHILD.lock().unwrap().as_mut().is_none_or(|c| c.try_wait().map_or(true, |s| s.is_some()));
        if exited {
            break;
        }
    }
    stop();
    ServerResult::Failed
}

/// Zatrzymuje tylko serwer uruchomiony przez nas.
pub fn stop() {
    if let Some(mut child) = CHILD.lock().unwrap().take() {
        let _ = child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // stan procesu jest globalny, więc testy uruchamiamy jeden po drugim
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn deps(up_after: usize, binary: Option<&'static str>, spawn_ok: bool) -> (ServerDeps, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let (checks, spawns) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (c, s) = (checks.clone(), spawns.clone());
        let deps = ServerDeps {
            is_up: Arc::new(move || {
                let n = c.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move { n >= up_after })
            }),
            spawn: Arc::new(move |_| {
                s.fetch_add(1, Ordering::SeqCst);
                if spawn_ok {
                    Command::new("sleep").arg("30").kill_on_drop(true).spawn()
                } else {
                    Err(std::io::Error::other("brak uprawnień"))
                }
            }),
            find: Arc::new(move || binary.map(PathBuf::from)),
            poll: Duration::from_millis(5),
        };
        (deps, checks, spawns)
    }

    #[test]
    fn finds_binary_on_path_then_in_known_places() {
        let found = find_binary("/a:/b", |p| p == Path::new("/b/ollama"));
        assert_eq!(found, Some(PathBuf::from("/b/ollama")));
        assert_eq!(find_binary("/a", |p| p == Path::new("/opt/homebrew/bin/ollama")), Some(PathBuf::from("/opt/homebrew/bin/ollama")));
        assert_eq!(find_binary("", |_| false), None);
    }

    #[tokio::test]
    async fn leaves_a_running_server_alone() {
        let _g = SERIAL.lock().await;
        let (d, _, spawns) = deps(0, Some("/x/ollama"), true);
        assert_eq!(ensure_running(Duration::from_secs(1), &d).await, ServerResult::Running);
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
        stop();
    }

    #[tokio::test]
    async fn starts_its_own_server_and_stops_it() {
        let _g = SERIAL.lock().await;
        let (d, checks, spawns) = deps(3, Some("/x/ollama"), true);
        assert_eq!(ensure_running(Duration::from_secs(2), &d).await, ServerResult::Started);
        assert_eq!((spawns.load(Ordering::SeqCst), checks.load(Ordering::SeqCst) >= 4), (1, true));
        assert!(CHILD.lock().unwrap().is_some());
        stop();
        assert!(CHILD.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn reports_missing_binary_failed_spawn_and_timeout() {
        let _g = SERIAL.lock().await;
        let (none, _, spawns) = deps(usize::MAX, None, true);
        assert_eq!(ensure_running(Duration::from_secs(1), &none).await, ServerResult::Missing);
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
        let (broken, _, _) = deps(usize::MAX, Some("/x/ollama"), false);
        assert_eq!(ensure_running(Duration::from_secs(1), &broken).await, ServerResult::Failed);
        let (never, _, _) = deps(usize::MAX, Some("/x/ollama"), true);
        assert_eq!(ensure_running(Duration::from_millis(50), &never).await, ServerResult::Failed);
        assert!(CHILD.lock().unwrap().is_none(), "po nieudanym starcie nie zostaje żaden proces");
    }
}
