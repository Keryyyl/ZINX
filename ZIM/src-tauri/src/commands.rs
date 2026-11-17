//! Komendy wywoływane z interfejsu (`src/shared/api.ts`). Nazwa = dawny kanał IPC z Electrona z `_` zamiast `:`.
//! Dane z interfejsu są niezaufane: walidacja jest w modułach `db`, tu tylko spinamy je ze stanem aplikacji.

use crate::ai::{is_cloud, is_provider, CLOUD_PROVIDERS};
use crate::db::{self, Db, accounts, contacts, messages, settings, tags};
use crate::error::{Error, Result};
use crate::db::{attachments as attachment_db, outbox as outbox_db};
use crate::mail::attachment_cache::AttachmentCache;
use crate::mail::attachments::{is_risky, safe_filename};
use crate::classifier::clients::{list_models, OLLAMA_URL};
use crate::classifier::queue::{Classifier, QueueStatus};
use crate::classifier::summary::Summarizer;
use crate::mail::manager::{Connector, SyncManager};
use crate::mail::outbox::Outbox;
use crate::mail::send::{validate_mail, MailAttachment, OutgoingMail};
use crate::secrets;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri::State;

pub struct AppState {
    pub db: Db,
    pub sync: SyncManager,
    pub outbox: Outbox,
    pub cache: AttachmentCache,
    pub connect: Connector,
    pub classifier: Classifier,
    pub summarizer: Summarizer,
    /// Uruchamia serwer Ollamy (gdy trzeba) i kolejkę tagowania; woła się po zmianie ustawień asystenta
    pub start_ai: std::sync::Arc<dyn Fn() + Send + Sync>,
    /// Pliki wybrane w oknie systemowym; interfejs dostaje tylko token, nigdy ścieżkę
    pub picked: Mutex<HashMap<String, PathBuf>>,
}

impl AppState {
    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.db.lock()
    }
}


use crate::now_ms;

// --- wiadomości ---

#[tauri::command]
pub fn messages_list(state: State<AppState>, query: messages::MessageQuery) -> Result<Vec<messages::MessageRow>> {
    messages::list(&*state.conn()?, &query)
}

#[tauri::command]
pub fn messages_search(state: State<AppState>, query: messages::SearchQuery) -> Result<Vec<messages::MessageRow>> {
    messages::search(&*state.conn()?, &query)
}

#[tauri::command]
pub fn messages_get(state: State<AppState>, id: i64) -> Result<Option<messages::MessageDetail>> {
    messages::get(&*state.conn()?, id)
}

#[tauri::command]
pub fn messages_read(state: State<AppState>, ids: Vec<i64>, read: bool) -> Result<()> {
    let result = messages::set_read(&*state.conn()?, &db::sanitize_ids(&ids), read);
    state.sync.schedule_push();
    result
}

#[tauri::command]
pub fn messages_star(state: State<AppState>, ids: Vec<i64>, starred: bool) -> Result<()> {
    let result = messages::set_starred(&*state.conn()?, &db::sanitize_ids(&ids), starred);
    state.sync.schedule_push();
    result
}

#[tauri::command]
pub fn messages_move(state: State<AppState>, ids: Vec<i64>, folder: String) -> Result<()> {
    let result = messages::move_to_folder(&*state.conn()?, &db::sanitize_ids(&ids), &folder);
    state.sync.schedule_push();
    result
}

#[tauri::command]
pub fn messages_spam(state: State<AppState>, ids: Vec<i64>) -> Result<usize> {
    let result = messages::mark_spam(&*state.conn()?, &db::sanitize_ids(&ids), now_ms());
    state.sync.schedule_push();
    result
}

#[tauri::command]
pub fn messages_not_spam(state: State<AppState>, ids: Vec<i64>) -> Result<()> {
    let result = messages::mark_not_spam(&*state.conn()?, &db::sanitize_ids(&ids));
    state.sync.schedule_push();
    result
}

#[tauri::command]
pub fn messages_delete_forever(state: State<AppState>, ids: Vec<i64>) -> Result<()> {
    let result = messages::delete_forever(&*state.conn()?, &db::sanitize_ids(&ids));
    state.sync.schedule_push();
    result
}

#[tauri::command]
pub fn messages_tag(state: State<AppState>, ids: Vec<i64>, tag_id: i64, on: bool) -> Result<()> {
    tags::set_message_tag(&*state.conn()?, &db::sanitize_ids(&ids), tag_id, on)
}

#[tauri::command]
pub async fn messages_summary(state: State<'_, AppState>, id: i64, force: Option<bool>) -> Result<Option<String>> {
    state.summarizer.get(id, force.unwrap_or(false)).await
}

#[tauri::command]
pub async fn messages_reclassify(state: State<'_, AppState>, id: i64) -> Result<Option<messages::MessageDetail>> {
    state.classifier.classify_now(id).await?;
    state.sync.schedule_push();
    messages::get(&*state.conn()?, id)
}

// --- tagi ---

#[tauri::command]
pub fn tags_list(state: State<AppState>) -> Result<Vec<tags::Tag>> {
    tags::list(&*state.conn()?)
}

#[tauri::command]
pub fn tags_create(state: State<AppState>, name: String, color: Option<String>, description: Option<String>) -> Result<tags::Tag> {
    tags::create(&*state.conn()?, &name, color.as_deref(), description.as_deref().unwrap_or(""))
}

#[tauri::command]
pub fn tags_update(state: State<AppState>, id: i64, name: String, color: Option<String>, description: Option<String>) -> Result<tags::Tag> {
    tags::update(&*state.conn()?, id, &name, color.as_deref(), description.as_deref())
}

#[tauri::command]
pub fn tags_move(state: State<AppState>, id: i64, direction: String) -> Result<()> {
    tags::move_tag(&*state.conn()?, id, &direction)
}

#[tauri::command]
pub fn tags_delete(state: State<AppState>, id: i64) -> Result<()> {
    tags::delete(&*state.conn()?, id)
}

// --- kontakty ---

#[tauri::command]
pub fn contacts_list(state: State<AppState>) -> Result<Vec<contacts::Contact>> {
    contacts::list(&*state.conn()?)
}

#[tauri::command]
pub fn contacts_save(state: State<AppState>, contact: contacts::Contact) -> Result<contacts::Contact> {
    contacts::save(&*state.conn()?, contact)
}

#[tauri::command]
pub fn contacts_mail(state: State<AppState>, id: i64) -> Result<Vec<contacts::ContactMail>> {
    contacts::mail(&*state.conn()?, id, 3)
}

#[tauri::command]
pub fn contacts_delete(state: State<AppState>, id: i64) -> Result<()> {
    contacts::delete(&*state.conn()?, id)
}

#[tauri::command]
pub async fn contacts_pick_photo(app: AppHandle) -> Result<Option<String>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Wybierz zdjęcie")
        .add_filter("Obrazy", &["png", "jpg", "jpeg", "webp", "gif"])
        .blocking_pick_file();
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|e| Error(e.to_string()))?;
    let size = tokio::fs::metadata(&path).await.map_err(|e| Error(e.to_string()))?.len();
    if size > 1_000_000 {
        return Err("Zdjęcie jest za duże (najwyżej 1 MB)".into());
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let mime = if ext == "jpg" { "jpeg".to_string() } else { ext };
    let bytes = tokio::fs::read(&path).await.map_err(|e| Error(e.to_string()))?;
    use base64::Engine;
    Ok(Some(format!("data:image/{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

// --- konta ---

#[derive(Deserialize)]
pub struct NewAccountInput {
    #[serde(flatten)]
    fields: accounts::NewAccount,
    password: String,
}

#[tauri::command]
pub fn accounts_list(state: State<AppState>) -> Result<Vec<accounts::Account>> {
    accounts::list(&*state.conn()?)
}

#[tauri::command]
pub fn accounts_add(state: State<AppState>, input: NewAccountInput) -> Result<accounts::Account> {
    if input.password.is_empty() {
        return Err("Podaj hasło".into());
    }
    let conn = state.conn()?;
    let account = accounts::add(&conn, &input.fields)?;
    if let Err(e) = secrets::set(&secrets::account_key(account.id), &input.password) {
        accounts::remove(&conn, account.id)?;
        return Err(e);
    }
    drop(conn);
    state.sync.start(account.clone());
    Ok(account)
}

#[tauri::command]
pub fn accounts_set_alias(state: State<AppState>, id: i64, alias: Option<String>) -> Result<Option<String>> {
    accounts::set_alias(&*state.conn()?, id, alias.as_deref())
}

#[tauri::command]
pub fn accounts_remove(state: State<AppState>, id: i64) -> Result<()> {
    state.sync.abort(id);
    accounts::remove(&*state.conn()?, id)?;
    secrets::delete(&secrets::account_key(id))
}

// --- asystent AI ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    #[serde(flatten)]
    queue: QueueStatus,
    /// Modele zainstalowane w Ollamie; None = Ollama nie odpowiada (albo wybrano innego dostawcę)
    models: Option<Vec<String>>,
    /// Czy zapisano klucz API danego dostawcy (samego klucza interfejs nigdy nie dostaje)
    keys: BTreeMap<String, bool>,
    consent: BTreeMap<String, bool>,
    models_by_provider: BTreeMap<String, String>,
}

#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>) -> Result<AiStatus> {
    let queue = state.classifier.status();
    let models = if queue.config.provider == "ollama" { list_models(OLLAMA_URL).await } else { None };
    let keys = CLOUD_PROVIDERS
        .iter()
        .map(|p| (p.to_string(), secrets::get(&secrets::ai_key(p)).ok().flatten().is_some()))
        .collect();
    let (consent, models_by_provider) = {
        let conn = state.conn()?;
        (settings::consent_by_provider(&conn)?, settings::models_by_provider(&conn)?)
    };
    Ok(AiStatus { queue, models, keys, consent, models_by_provider })
}

#[tauri::command]
pub fn ai_set_config(state: State<AppState>, config: settings::AiConfigInput) -> Result<()> {
    settings::set_ai_config(&*state.conn()?, &config)?;
    (state.start_ai)();
    Ok(())
}

/// Klucz API: tylko drukowalne znaki ASCII bez spacji. `None` usuwa klucz; z powrotem go nie odczytamy.
/// Klucz API: tylko drukowalne znaki ASCII bez spacji. `None` usuwa klucz; z powrotem go nie odczytamy.
#[tauri::command]
pub fn ai_set_key(state: State<AppState>, provider: String, key: Option<String>) -> Result<()> {
    if !is_provider(&provider) || !is_cloud(&provider) {
        return Err("Nieznany dostawca".into());
    }
    match key.as_deref().map(str::trim) {
        None => secrets::delete(&secrets::ai_key(&provider))?,
        Some(k) => {
            if !(8..=400).contains(&k.len()) || !k.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
                return Err("Klucz API wygląda nieprawidłowo (bez spacji, 8-400 znaków)".into());
            }
            secrets::set(&secrets::ai_key(&provider), k)?;
        }
    }
    state.classifier.kick();
    Ok(())
}

/// Po dodaniu tagów albo zmianie modelu: ostatnie maile tagowane od nowa.
#[tauri::command]
pub fn ai_reprocess(state: State<AppState>) -> Result<()> {
    state.classifier.reprocess();
    Ok(())
}

// --- pliki, wysyłka, zaplanowane ---

/// Interfejs podaje tylko id załącznika; konto, folder i UID bierzemy z bazy.
fn attachment_by_id(state: &AppState, id: i64) -> Result<attachment_db::AttachmentLocation> {
    state.db.with(|c| attachment_db::get(c, id))?.ok_or_else(|| Error::from("Nie ma takiego załącznika"))
}

async fn cache_attachment(state: &AppState, att: &attachment_db::AttachmentLocation) -> Result<PathBuf> {
    let account = state
        .db
        .with(accounts::list)?
        .into_iter()
        .find(|a| a.id == att.account_id)
        .ok_or_else(|| Error::from("Nie ma konta tej wiadomości"))?;
    let connect = state.connect.clone();
    state.cache.get(att, move || connect(account)).await
}

#[tauri::command]
pub async fn attachments_open(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<()> {
    let att = attachment_by_id(&state, id)?;
    if is_risky(&att.filename) {
        // pobrane przez ZIM pliki nie mają znacznika kwarantanny, więc system ich nie sprawdzi
        let confirmed = app
            .dialog()
            .message(format!("„{}” może uruchomić program na tym komputerze.\n\nPliki tego typu w wiadomościach bywają złośliwe. Otwieraj je tylko od zaufanych nadawców.", att.filename))
            .title("Uwaga")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("Otwórz mimo to".into(), "Anuluj".into()))
            .blocking_show();
        if !confirmed {
            return Ok(());
        }
    }
    let path = cache_attachment(&state, &att).await?;
    opener::open(&path).map_err(|e| Error(format!("Nie udało się otworzyć pliku: {e}")))
}

#[tauri::command]
pub async fn attachments_save(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<bool> {
    let att = attachment_by_id(&state, id)?;
    let mut dialog = app.dialog().file().set_title("Zapisz załącznik").set_file_name(safe_filename(&att.filename));
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let Some(target) = dialog.blocking_save_file() else { return Ok(false) };
    let target = target.into_path().map_err(|e| Error(e.to_string()))?;
    let source = cache_attachment(&state, &att).await?;
    tokio::fs::copy(&source, &target).await.map_err(|e| Error(format!("Nie udało się zapisać pliku: {e}")))?;
    Ok(true)
}

/// Zapis małego pliku tekstowego z natywnym oknem „Zapisz jako” (np. szablon własnego motywu).
#[tauri::command]
pub async fn files_save_text(app: AppHandle, filename: String, content: String) -> Result<bool> {
    if content.chars().count() > 100_000 {
        return Err("Nieprawidłowy plik".into());
    }
    let mut dialog = app.dialog().file().set_title("Zapisz plik").set_file_name(safe_filename(&filename)).add_filter("Plik JSON", &["json"]);
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let Some(target) = dialog.blocking_save_file() else { return Ok(false) };
    let target = target.into_path().map_err(|e| Error(e.to_string()))?;
    tokio::fs::write(&target, content).await.map_err(|e| Error(format!("Nie udało się zapisać pliku: {e}")))?;
    Ok(true)
}

#[derive(Serialize)]
pub struct PickedFile {
    token: String,
    name: String,
    size: u64,
}

#[tauri::command]
pub async fn compose_pick_files(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<PickedFile>> {
    let Some(files) = app.dialog().file().set_title("Dodaj załączniki").blocking_pick_files() else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for file in files {
        let path = file.into_path().map_err(|e| Error(e.to_string()))?;
        let size = tokio::fs::metadata(&path).await.map_err(|e| Error(e.to_string()))?.len();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let token = uuid::Uuid::new_v4().to_string();
        state.picked.lock().map_err(|_| Error::from("Stan aplikacji jest niedostępny"))?.insert(token.clone(), path);
        out.push(PickedFile { token, name, size });
    }
    Ok(out)
}

/// Szkic z okna „Nowa wiadomość”
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingDraft {
    account_id: i64,
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    text: String,
    /// Tokeny plików wybranych w oknie systemowym (ścieżek interfejs nie widzi)
    #[serde(default)]
    attachments: Vec<String>,
}

/// Szkic -> gotowa wiadomość z treścią załączników.
async fn draft_to_mail(state: &AppState, draft: &OutgoingDraft) -> Result<OutgoingMail> {
    let mut attachments = Vec::new();
    for token in &draft.attachments {
        let path = state.picked.lock().map_err(|_| Error::from("Stan aplikacji jest niedostępny"))?.get(token).cloned();
        let path = path.ok_or_else(|| Error::from("Załącznik jest niedostępny, dodaj go ponownie"))?;
        let content = tokio::fs::read(&path).await.map_err(|e| Error(format!("Nie udało się odczytać załącznika: {e}")))?;
        let filename = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "załącznik".into());
        attachments.push(MailAttachment { filename, content });
    }
    let mail = OutgoingMail {
        to: draft.to.iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(),
        subject: draft.subject.clone(),
        text: draft.text.clone(),
        attachments,
    };
    validate_mail(&mail)?;
    Ok(mail)
}

fn release_files(state: &AppState, draft: &OutgoingDraft) {
    if let Ok(mut picked) = state.picked.lock() {
        draft.attachments.iter().for_each(|t| {
            picked.remove(t);
        });
    }
}

#[tauri::command]
pub async fn compose_send(state: State<'_, AppState>, draft: OutgoingDraft) -> Result<()> {
    let mail = draft_to_mail(&state, &draft).await?;
    state.outbox.send_now(draft.account_id, mail).await?;
    release_files(&state, &draft);
    Ok(())
}

#[tauri::command]
pub async fn compose_schedule(state: State<'_, AppState>, draft: OutgoingDraft, send_at: i64) -> Result<i64> {
    let now = now_ms();
    if send_at < now - 60_000 {
        return Err("Ta godzina już minęła".into());
    }
    let mail = draft_to_mail(&state, &draft).await?;
    let id = state.db.with(|c| {
        if !accounts::list(c)?.iter().any(|a| a.id == draft.account_id) {
            return Err(Error::from("Wybierz konto, z którego chcesz wysłać wiadomość"));
        }
        outbox_db::schedule(c, draft.account_id, &mail, send_at, now)
    })?;
    release_files(&state, &draft);
    state.outbox.notify();
    Ok(id)
}

#[tauri::command]
pub fn outbox_list(state: State<AppState>, account_id: Option<i64>) -> Result<Vec<outbox_db::ScheduledMail>> {
    outbox_db::list(&*state.conn()?, account_id)
}

#[tauri::command]
pub fn outbox_cancel(state: State<AppState>, id: i64) -> Result<()> {
    outbox_db::cancel(&*state.conn()?, id)?;
    state.outbox.notify();
    Ok(())
}

#[tauri::command]
pub fn outbox_retry(state: State<AppState>, id: i64) -> Result<()> {
    outbox_db::retry(&*state.conn()?, id, now_ms())?;
    state.outbox.notify();
    state.outbox.kick();
    Ok(())
}

// --- finanse osobiste ---
