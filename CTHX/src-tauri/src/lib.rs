// CTHX: kalendarz, lista zadań i habit tracker. Komendy mają nazwy cthx_* i kontrakt w src/shared/api.ts.

mod birthdays;
mod commands;
mod contacts;
mod db;
mod error;
mod habits;
mod kanban;

use commands::AppState;
use db::Db;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Okno główne: na macOS bez paska tytułowego, przeciąganie przez .titlebar-drag (data-tauri-drag-region)
fn create_window(app: &tauri::App) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("CTHX")
        .inner_size(1200.0, 800.0)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .build()?;
    // po starcie okno ma być aktywne od razu (inaczej pierwsze kliknięcie bywa połknięte)
    window.set_focus()?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("cthx.sqlite")).map_err(|e| e.to_string())?;
            app.manage(AppState { db: Db::new(conn) });
            create_window(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::cthx_items_list,
            commands::cthx_done_list,
            commands::cthx_item_save,
            commands::cthx_item_delete,
            commands::cthx_item_set_done,
            commands::cthx_birthdays_sync,
            commands::cthx_habits_list,
            commands::cthx_habit_save,
            commands::cthx_habit_delete,
            commands::cthx_habit_days,
            commands::cthx_habit_set_day,
            commands::cthx_kanban_list,
            commands::cthx_kanban_save,
            commands::cthx_kanban_delete,
        ])
        .run(tauri::generate_context!())
        .expect("nie udało się uruchomić CTHX");
}
