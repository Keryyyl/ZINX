// Rdzeń ZIM. Komendy mają te same nazwy co dawne kanały IPC z Electrona ('messages:list' -> messages_list),
// a ich kontrakt opisuje src/shared/api.ts.

mod address;
mod ai;
mod classifier;
mod commands;
mod db;
mod tray;
mod error;
mod mail;
mod priority;
mod secrets;
mod smtp;

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

use commands::AppState;
use classifier::llm::classify_with;
use classifier::ollama_server;
use classifier::providers::{create_chat, Endpoints};
use classifier::queue::{Classifier, ClassifierDeps};
use classifier::summary::{summarize_with, Summarizer};
use mail::attachment_cache::{AttachmentCache, CACHE_LIMIT};
use mail::imap_real::RealSession;
use mail::outbox::{real_sender, Outbox};
use mail::manager::{Connector, Emit, SyncManager};
use mail::session::ImapSession;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager};

/// Jak często odpytujemy serwery o nowe wiadomości
const POLL: Duration = Duration::from_secs(60);
/// Jak często sprawdzamy, czy nadszedł czas wysyłki zaplanowanych maili
const OUTBOX_POLL: Duration = Duration::from_secs(15);

/// Okno główne tworzymy w kodzie, bo linki z treści maila (target=_blank w czytniku) mają otwierać się w
/// przeglądarce, nigdy w aplikacji.
fn create_window(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default()).title("ZIM");
    #[cfg(desktop)]
    let builder = builder
        .inner_size(1200.0, 800.0)
        // macOS: bez paska tytułowego, przyciski okna wtapiają się w pasek boczny (przeciąganie: .titlebar-drag)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .on_new_window(|url, _| {
            if matches!(url.scheme(), "http" | "https" | "mailto") {
                let _ = opener::open(url.as_str());
            }
            tauri::webview::NewWindowResponse::Deny
        });
    builder.build()?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // dwie biblioteki TLS w drzewie zależności: wybieramy dostawcę kryptografii jawnie
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("zim.sqlite")).map_err(|e| e.to_string())?;
            let db = db::Db::new(conn);
            let handle = app.handle().clone();
            let emit: Emit = Arc::new(move |status| {
                let _ = handle.emit("sync:status", status);
            });
            let connect: Connector = Arc::new(|account| {
                Box::pin(async move {
                    let password = secrets::get(&secrets::account_key(account.id))?.ok_or("Brak hasła do konta w schowku")?;
                    let session = RealSession::connect(&account.imap_host, account.imap_port, &account.username, &password).await?;
                    Ok(Box::new(session) as Box<dyn ImapSession>)
                })
            });
            // sync i kolejka AI wołają się nawzajem (po synchronizacji tagowanie, po archiwizacji wypchnięcie zmian)
            let classifier_cell: Arc<OnceLock<Classifier>> = Arc::new(OnceLock::new());
            let sync_cell: Arc<OnceLock<SyncManager>> = Arc::new(OnceLock::new());

            let cell = classifier_cell.clone();
            let sync = SyncManager::new(
                db.clone(),
                connect.clone(),
                emit,
                Arc::new(move || {
                    if let Some(c) = cell.get() {
                        c.kick(); // nowe maile mogły właśnie wylądować w bazie
                    }
                }),
            );
            let _ = sync_cell.set(sync.clone());

            let key_reader: Arc<dyn Fn(&str) -> Option<String> + Send + Sync> =
                Arc::new(|provider| secrets::get(&secrets::ai_key(provider)).ok().flatten());
            let chat = create_chat(key_reader, Endpoints::default());
            // tagi od AI pojawiają się pojedynczo; interfejs odświeżamy najwyżej co chwilę
            let handle = app.handle().clone();
            let tagged_pending = Arc::new(AtomicBool::new(false));
            let on_tagged: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if tagged_pending.swap(true, Ordering::SeqCst) {
                    return;
                }
                let (handle, flag) = (handle.clone(), tagged_pending.clone());
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    flag.store(false, Ordering::SeqCst);
                    let _ = handle.emit("ai:tagged", ());
                });
            });
            let archived_cell = sync_cell.clone();
            let classifier = Classifier::new(ClassifierDeps {
                db: db.clone(),
                classify: classify_with(chat.clone()),
                on_tagged,
                on_archived: Arc::new(move || {
                    if let Some(s) = archived_cell.get() {
                        s.schedule_push();
                    }
                }),
                now: Arc::new(now_ms),
            });
            let _ = classifier_cell.set(classifier.clone());
            let summarizer = Summarizer::new(db.clone(), summarize_with(chat));

            // lokalny model: serwer Ollamy startuje razem z aplikacją; model w chmurze nie potrzebuje niczego
            let (ai_db, ai_classifier) = (db.clone(), classifier.clone());
            let start_ai: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let config = ai_db.with(db::settings::get_ai_config);
                let classifier = ai_classifier.clone();
                match config {
                    Ok(c) if c.enabled && !ai::is_cloud(&c.provider) => {
                        tauri::async_runtime::spawn(async move {
                            ollama_server::ensure_running(Duration::from_secs(15), &ollama_server::real_deps()).await;
                            classifier.kick();
                        });
                    }
                    _ => classifier.kick(),
                }
            });

            let handle = app.handle().clone();
            let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = handle.emit("outbox:changed", ());
            });
            let outbox = Outbox::new(db.clone(), real_sender(db.clone(), connect.clone()), notify);
            db.with(db::outbox::recover_interrupted).map_err(|e| e.to_string())?;
            let cache = AttachmentCache::new(dir.join("attachments"), CACHE_LIMIT);
            let pruning = dir.join("attachments");
            std::thread::spawn(move || mail::attachment_cache::prune_cache(&pruning, CACHE_LIMIT));
            app.manage(AppState {
                db,
                sync: sync.clone(),
                outbox: outbox.clone(),
                cache,
                connect,
                classifier,
                summarizer,
                start_ai: start_ai.clone(),
                picked: Default::default(),
            });
            create_window(app)?;
            // zasobnik systemowy: aplikacja żyje w tle, a okno wraca z menu albo kliknięciem ikony
            #[cfg(desktop)]
            tray::install(app.handle())?;
            // serwer Ollamy startuje razem z aplikacją (tylko gdy asystent jest włączony); dopiero potem tagowanie
            start_ai();


            outbox.kick();
            let timer = outbox.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(OUTBOX_POLL);
                loop {
                    tick.tick().await;
                    timer.process(now_ms()).await;
                }
            });

            sync.sync_all();
            let poll = sync.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(POLL);
                tick.tick().await;
                loop {
                    tick.tick().await;
                    poll.sync_all();
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // zamknięcie okna chowa je do zasobnika; sync i odczyt CSV dalej działają
            #[cfg(desktop)]
            {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            if let tauri::WindowEvent::Focused(true) = event {
                if let Some(state) = window.try_state::<AppState>() {
                    state.sync.sync_all();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::messages_list,
            commands::messages_search,
            commands::messages_get,
            commands::messages_read,
            commands::messages_star,
            commands::messages_move,
            commands::messages_spam,
            commands::messages_not_spam,
            commands::messages_delete_forever,
            commands::messages_tag,
            commands::messages_summary,
            commands::messages_reclassify,
            commands::tags_list,
            commands::tags_create,
            commands::tags_update,
            commands::tags_move,
            commands::tags_delete,
            commands::contacts_list,
            commands::contacts_save,
            commands::contacts_mail,
            commands::contacts_delete,
            commands::contacts_pick_photo,
            commands::accounts_list,
            commands::accounts_add,
            commands::accounts_set_alias,
            commands::accounts_remove,
            commands::ai_status,
            commands::ai_set_config,
            commands::ai_set_key,
            commands::ai_reprocess,
            commands::attachments_open,
            commands::attachments_save,
            commands::files_save_text,
            commands::compose_pick_files,
            commands::compose_send,
            commands::compose_schedule,
            commands::outbox_list,
            commands::outbox_cancel,
            commands::outbox_retry,
        ])
        .build(tauri::generate_context!())
        .expect("nie udało się uruchomić ZIM")
        .run(|_, event| {
            // zamykamy tylko serwer, który uruchomiliśmy sami; cudzy (aplikacja Ollama, brew services) zostaje
            if let tauri::RunEvent::Exit = event {
                ollama_server::stop();
            }
        });
}
