// PFMX: finanse osobiste. Komendy mają nazwy finance_* i kontrakt w src/shared/api.ts.

mod commands;
mod db;
mod error;
mod finance;
#[cfg(desktop)]
mod tray;

use commands::AppState;
use db::Db;
use std::time::Duration;
use tauri::{Manager, WindowEvent};

/// Jak często model dostaje kolejną porcję wydatków do kategoryzacji.
const AI_POLL: Duration = Duration::from_secs(120);
/// Jak często sprawdzamy folder eksportów z banku (plus przy starcie).
const FINANCE_POLL: Duration = Duration::from_secs(600);

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn create_window(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("PFMX")
        .inner_size(1100.0, 760.0)
        // macOS: bez paska tytułowego, przyciski okna wtapiają się w pasek boczny (przeciąganie: .titlebar-drag)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .build()?;
    // po uruchomieniu okno ma być aktywne od razu; inaczej pierwsze kliknięcie w PFMX bywa połknięte
    // (macOS: trzeba kliknąć inną aplikację i wrócić)
    window.set_focus()?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("pfmx.sqlite")).map_err(|e| e.to_string())?;
            let db = Db::new(conn);
            // wykrywanie płatności cyklicznych także dla danych sprzed tej wersji (nie tylko po imporcie)
            let _ = db.with(|c| finance::refresh_derived(c));
            app.manage(AppState { db: db.clone() });
            create_window(app)?;
            // aplikacja żyje w zasobniku; okno wraca z menu albo kliknięciem ikony
            #[cfg(desktop)]
            tray::install(app.handle())?;
            // odczyt eksportów z banku: przy starcie i co jakiś czas, także gdy okno jest ukryte
            // kategorie wydatków: lokalny model w tle, po chwili od startu i potem co 2 minuty
            let ai_db = db.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(AI_POLL);
                loop {
                    tick.tick().await;
                    let _ = finance::ai::run_pass(ai_db.clone(), 15).await;
                }
            });
            if let Ok(documents) = app.path().document_dir() {
                let dir = documents.join(finance::auto_import::FOLDER);
                tauri::async_runtime::spawn(async move {
                    let mut tick = tokio::time::interval(FINANCE_POLL);
                    loop {
                        tick.tick().await;
                        let _ = finance::auto_import::run_on(db.clone(), dir.clone()).await;
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // zamknięcie okna chowa je do zasobnika; odczyt CSV dalej działa
            #[cfg(desktop)]
            {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::finance_accounts_summary,
            commands::finance_daily_flow,
            commands::finance_account_add,
            commands::finance_transactions_list,
            commands::finance_import_csv,
            commands::finance_export_csv,
            commands::finance_ai_status,
            commands::finance_ai_run,
            commands::finance_income_set,
            commands::finance_income_summary,
            commands::finance_transactions_search,
            commands::finance_envelopes_list,
            commands::finance_cash_state,
            commands::finance_cash_add,
            commands::finance_cash_set_balance,
            commands::finance_cash_update,
            commands::finance_cash_delete,
            commands::finance_auto_import_now,
            commands::finance_budget_view,
            commands::finance_budget_set_planned,
            commands::finance_cushion_set_target,
            commands::finance_tag_transaction,
            commands::finance_recurring_list,
            commands::finance_recurring_save,
            commands::finance_recurring_delete,
            commands::finance_recurring_set_paid,
            commands::finance_invest_import,
            commands::finance_invest_holdings,
            commands::finance_invest_history,
            commands::finance_receipt_info,
            commands::finance_receipt_attach,
            commands::finance_receipt_open,
        ])
        .run(tauri::generate_context!())
        .expect("nie udało się uruchomić PFMX");
}
