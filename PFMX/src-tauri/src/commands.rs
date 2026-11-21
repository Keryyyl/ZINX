//! Komendy wywoływane z interfejsu. Nazwy i argumenty pasują do src/renderer/src/bridge.ts
//! (Tauri zamienia camelCase z interfejsu na snake_case z Rusta).

use crate::db::{budget, finance as fin, invest, receipts, recurring, Db};
use crate::finance::xtb;
use crate::error::{Error, Result};
use crate::finance::auto_import::{self, AutoImportReport};
use crate::finance::import::{import_millennium, ImportReport};
use crate::db::invest::{Holding, HistoryEvent, InvestImportReport};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::MutexGuard;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    pub db: Db,
}

impl AppState {
    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.db.lock()
    }
}

/// Folder eksportów z banku: Dokumenty/Millennium.
fn export_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().document_dir().map_err(|e| Error(e.to_string()))?.join(auto_import::FOLDER))
}

#[tauri::command]
pub fn finance_accounts_summary(state: State<AppState>) -> Result<Vec<fin::AccountSummary>> {
    fin::account_summaries(&*state.conn()?)
}

#[tauri::command]
pub fn finance_account_add(state: State<AppState>, name: String, kind: String, iban: Option<String>) -> Result<fin::FinAccount> {
    fin::add_account(&*state.conn()?, &name, &kind, iban.as_deref())
}

#[tauri::command]
pub fn finance_transactions_list(state: State<AppState>, account_id: Option<i64>, limit: Option<i64>) -> Result<Vec<fin::TransactionRow>> {
    fin::list_transactions(&*state.conn()?, account_id, limit.unwrap_or(200))
}

/// Plik wybiera użytkownik w oknie systemowym; interfejs nie podaje ścieżki.
#[tauri::command]
pub async fn finance_import_csv(app: AppHandle, state: State<'_, AppState>) -> Result<Option<ImportReport>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Wybierz eksport historii z Millennium")
        .add_filter("CSV", &["csv"])
        .blocking_pick_file();
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|e| Error(e.to_string()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let text = std::fs::read_to_string(&path).map_err(|e| Error(format!("Nie udało się odczytać pliku: {e}")))?;
    let report = import_millennium(&*state.conn()?, &text, &name)?;
    Ok(Some(report))
}

/// Odczyt eksportów z folderu Millennium; wywoływany też ręcznie („Odśwież dane”).
#[tauri::command]
pub async fn finance_auto_import_now(app: AppHandle, state: State<'_, AppState>) -> Result<AutoImportReport> {
    auto_import::run_on(state.db.clone(), export_dir(&app)?).await
}

#[tauri::command]
pub fn finance_budget_view(state: State<AppState>, month: String) -> Result<budget::BudgetView> {
    budget::view(&*state.conn()?, &month)
}

#[tauri::command]
pub fn finance_budget_set_planned(state: State<AppState>, envelope_id: i64, month: String, amount: i64) -> Result<()> {
    budget::set_planned(&*state.conn()?, envelope_id, &month, amount)
}

#[tauri::command]
pub fn finance_cushion_set_target(state: State<AppState>, amount: i64) -> Result<()> {
    budget::set_cushion_target(&*state.conn()?, amount)
}

/// Ręczne przypisanie. Zwraca, ile innych transakcji tego odbiorcy dostało tę kategorię przez zapamiętaną regułę.
#[tauri::command]
pub fn finance_tag_transaction(state: State<AppState>, transaction_id: i64, envelope_id: Option<i64>) -> Result<usize> {
    let conn = state.conn()?;
    budget::tag_transaction(&conn, transaction_id, envelope_id)?;
    match envelope_id {
        Some(envelope) => crate::db::categorize::learn(&conn, transaction_id, envelope),
        None => Ok(0),
    }
}

#[tauri::command]
pub async fn finance_ai_status() -> Result<crate::finance::ai::AiStatus> {
    Ok(crate::finance::ai::status().await)
}

/// Kategoryzuje wydatki lokalnym modelem (do 25 na raz). Zwraca, ile dostało kategorię i ile czeka.
#[tauri::command]
pub async fn finance_ai_run(state: State<'_, AppState>) -> Result<crate::finance::ai::PassReport> {
    crate::finance::ai::run_pass(state.db.clone(), 25).await
}

#[tauri::command]
pub fn finance_recurring_list(state: State<AppState>, month: String) -> Result<Vec<recurring::Recurring>> {
    recurring::list(&*state.conn()?, &month)
}

#[tauri::command]
pub fn finance_recurring_save(state: State<AppState>, input: recurring::RecurringInput) -> Result<i64> {
    recurring::save(&*state.conn()?, &input)
}

#[tauri::command]
pub fn finance_recurring_delete(state: State<AppState>, id: i64) -> Result<()> {
    recurring::delete(&*state.conn()?, id)
}

#[tauri::command]
pub fn finance_recurring_set_paid(state: State<AppState>, id: i64, month: String, paid: bool) -> Result<()> {
    recurring::set_paid(&*state.conn()?, id, &month, paid)
}

#[tauri::command]
pub fn finance_daily_flow(state: State<AppState>, since: String) -> Result<Vec<fin::DailyFlow>> {
    fin::daily_flow(&*state.conn()?, &since)
}

/// Zapisuje transakcje w układzie eksportu banku. Plik wybiera użytkownik; zwraca ścieżkę albo None przy anulowaniu.
/// Nazwa nie zaczyna się od „historia_”, więc automatyczny odczyt folderu Millennium go nie złapie.
#[tauri::command]
pub async fn finance_export_csv(app: AppHandle, state: State<'_, AppState>, account_id: Option<i64>) -> Result<Option<String>> {
    let text = crate::finance::millennium::export(&fin::export_rows(&*state.conn()?, account_id)?);
    let picked = app
        .dialog()
        .file()
        .set_title("Zapisz eksport transakcji")
        .set_file_name("eksport_pfmx.csv")
        .add_filter("CSV", &["csv"])
        .blocking_save_file();
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|e| Error(e.to_string()))?;
    std::fs::write(&path, text).map_err(|e| Error(format!("Nie udało się zapisać pliku: {e}")))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub fn finance_envelopes_list(state: State<AppState>) -> Result<Vec<budget::EnvelopeInfo>> {
    budget::envelopes(&*state.conn()?)
}

#[tauri::command]
pub fn finance_cash_state(state: State<AppState>) -> Result<crate::db::cash::CashState> {
    crate::db::cash::state(&*state.conn()?)
}

#[tauri::command]
pub fn finance_cash_add(state: State<AppState>, amount: i64, note: String) -> Result<i64> {
    crate::db::cash::add(&*state.conn()?, amount, &note)
}

/// Ustawia stan gotówki na wpisaną kwotę; różnicę zapisuje jako korektę. Zwraca kwotę korekty albo None.
#[tauri::command]
pub fn finance_cash_set_balance(state: State<AppState>, amount: i64, note: String) -> Result<Option<i64>> {
    crate::db::cash::set_balance(&*state.conn()?, amount, &note)
}

#[tauri::command]
pub fn finance_cash_update(state: State<AppState>, id: i64, amount: i64, note: String) -> Result<()> {
    crate::db::cash::update(&*state.conn()?, id, amount, &note)
}

#[tauri::command]
pub fn finance_cash_delete(state: State<AppState>, id: i64) -> Result<()> {
    crate::db::cash::delete(&*state.conn()?, id)
}

/// Ustawia rodzaj wpływu: „salary” (wypłata) albo „one_off” (jednorazowy).
#[tauri::command]
pub fn finance_income_set(state: State<AppState>, transaction_id: i64, kind: String) -> Result<()> {
    crate::db::income::set_kind(&*state.conn()?, transaction_id, &kind)
}

#[tauri::command]
pub fn finance_income_summary(state: State<AppState>, month: String) -> Result<crate::db::income::IncomeSummary> {
    crate::db::income::summary(&*state.conn()?, &month)
}

/// Wyszukiwanie transakcji po kryteriach z formularza (kategoria, rodzaj, data, wielkość).
#[tauri::command]
pub fn finance_transactions_search(state: State<AppState>, query: fin::TransactionQuery) -> Result<Vec<fin::TransactionRow>> {
    fin::search_transactions(&*state.conn()?, &query)
}

/// Eksport historii z XTB (xlsx); plik wybiera użytkownik. Ponowny import niczego nie dubluje.
#[tauri::command]
pub async fn finance_invest_import(app: AppHandle, state: State<'_, AppState>) -> Result<Option<InvestImportReport>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Wybierz eksport historii z XTB")
        .add_filter("Excel", &["xlsx"])
        .blocking_pick_file();
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|e| Error(e.to_string()))?;
    let parsed = xtb::parse_file(&path)?;
    let report = invest::import(&*state.conn()?, &parsed)?;
    Ok(Some(report))
}

#[tauri::command]
pub fn finance_invest_holdings(state: State<AppState>) -> Result<Vec<Holding>> {
    invest::holdings(&*state.conn()?)
}

#[tauri::command]
pub fn finance_invest_history(state: State<AppState>) -> Result<Vec<HistoryEvent>> {
    invest::history(&*state.conn()?)
}

/// Rozszerzenia, które przyjmujemy jako paragon
const RECEIPT_TYPES: &[&str] = &["jpg", "jpeg", "png", "heic", "webp", "pdf"];

fn receipts_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app.path().app_data_dir().map_err(|e| Error(e.to_string()))?.join("paragony"))
}

/// Dane paragonu przypiętego do transakcji (albo null, gdy go nie ma)
#[tauri::command]
pub fn finance_receipt_info(state: State<AppState>, transaction_id: i64) -> Result<Option<receipts::ReceiptInfo>> {
    receipts::get(&*state.conn()?, transaction_id)
}

/// Użytkownik wybiera zdjęcie paragonu w oknie systemowym; plik kopiujemy do danych aplikacji.
/// Nowy paragon zastępuje poprzedni dla tej samej transakcji.
#[tauri::command]
pub async fn finance_receipt_attach(
    app: AppHandle,
    state: State<'_, AppState>,
    transaction_id: i64,
) -> Result<Option<receipts::ReceiptInfo>> {
    {
        let conn = state.conn()?;
        let exists: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM fin_transactions WHERE id = ?1)", [transaction_id], |r| r.get(0))
            .map_err(|e| Error(e.to_string()))?;
        if !exists {
            return Err(Error("Nie ma takiej transakcji".into()));
        }
    }
    let picked = app
        .dialog()
        .file()
        .set_title("Wybierz zdjęcie paragonu")
        .add_filter("Paragon", RECEIPT_TYPES)
        .blocking_pick_file();
    let Some(file) = picked else { return Ok(None) };
    let src = file.into_path().map_err(|e| Error(e.to_string()))?;
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !RECEIPT_TYPES.contains(&ext.as_str()) {
        return Err(Error("Paragon musi być zdjęciem (JPG, PNG, HEIC, WEBP) albo PDF".into()));
    }
    let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let dir = receipts_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| Error(format!("Nie udało się utworzyć folderu paragonów: {e}")))?;
    let stored = format!("{transaction_id}-{now}.{ext}");
    std::fs::copy(&src, dir.join(&stored)).map_err(|e| Error(format!("Nie udało się skopiować paragonu: {e}")))?;

    let previous = receipts::put(&*state.conn()?, transaction_id, &name, &stored, now)?;
    if let Some(old) = previous {
        // poprzedni plik nie jest już potrzebny; błąd usuwania nie psuje zapisu
        let _ = std::fs::remove_file(dir.join(old));
    }
    receipts::get(&*state.conn()?, transaction_id)
}

/// Usuwa paragon z transakcji i jego kopię z folderu danych. Plik kasujemy po zapisie w bazie.
#[tauri::command]
pub fn finance_receipt_remove(app: AppHandle, state: State<AppState>, transaction_id: i64) -> Result<()> {
    let previous = receipts::remove(&*state.conn()?, transaction_id)?;
    if let Some(stored) = previous {
        // brak pliku nie jest błędem: wpis w bazie i tak zniknął
        let _ = std::fs::remove_file(receipts_dir(&app)?.join(stored));
    }
    Ok(())
}

/// Otwiera paragon w domyślnej aplikacji systemu (podgląd zdjęcia albo PDF)
#[tauri::command]
pub fn finance_receipt_open(app: AppHandle, state: State<AppState>, transaction_id: i64) -> Result<()> {
    let info = receipts::get(&*state.conn()?, transaction_id)?.ok_or("Ta transakcja nie ma paragonu")?;
    let path = receipts_dir(&app)?.join(&info.stored_as);
    if !path.exists() {
        return Err(Error("Plik paragonu zniknął z folderu danych aplikacji".into()));
    }
    std::process::Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(|e| Error(format!("Nie udało się otworzyć paragonu: {e}")))?;
    Ok(())
}
