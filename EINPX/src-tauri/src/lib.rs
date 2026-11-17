// EINPX: jedno okno z kalendarzem (CTHX), pocztą (ZIM), finansami (PFMX) i nawykami (CTHX).
// Odczyt idzie z baz pozostałych aplikacji, zapis tylko do CTHX. Komendy mają nazwy einpx_* i kontrakt w src/shared/api.ts.

mod cthx;
mod dates;
mod finance;
mod mail;
mod notes;
mod paths;
mod system;

use cthx::{HabitLine, NewItem, PlanItem};
use dates::{now_ms, valid_date, valid_month};
use finance::FinanceSummary;
use mail::MailSummary;
use notes::Note;
use system::SystemInfo;
use serde::Serialize;

/// Wynik jednej sekcji: albo dane, albo komunikat. Błąd jednej aplikacji nie blokuje reszty.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section<T> {
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> Section<T> {
    fn from_result(result: Result<T, String>) -> Self {
        match result {
            Ok(data) => Section { data: Some(data), error: None },
            Err(error) => Section { data: None, error: Some(error) },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub mail: Section<MailSummary>,
    pub finance: Section<FinanceSummary>,
    pub habits: Section<Vec<HabitLine>>,
    pub fetched_at: i64,
}

/// `today` = RRRR-MM-DD, `month` = RRRR-MM; przychodzą z frontendu, bo tam znamy lokalną strefę
#[tauri::command]
fn einpx_snapshot(today: String, month: String) -> Result<Snapshot, String> {
    if !valid_date(&today) || !valid_month(&month) {
        return Err("Nieprawidłowa data".into());
    }
    Ok(Snapshot {
        mail: Section::from_result(mail::summary()),
        finance: Section::from_result(finance::summary()),
        habits: Section::from_result(cthx::habits(&today, &month)),
        fetched_at: now_ms(),
    })
}

/// Bateria i zajętość dysków (nie zależy od baz aplikacji)
#[tauri::command]
fn einpx_system() -> Result<SystemInfo, String> {
    system::info()
}

/// Plan jednego dnia (zadania i wydarzenia, także powtarzalne)
#[tauri::command]
fn einpx_day_plan(date: String) -> Result<Vec<PlanItem>, String> {
    if !valid_date(&date) {
        return Err("Nieprawidłowa data".into());
    }
    cthx::plan(&date)
}

#[tauri::command]
fn einpx_notes_list() -> Result<Vec<Note>, String> {
    notes::list()
}

#[tauri::command]
fn einpx_note_add(text: String) -> Result<(), String> {
    notes::add(&text)
}

#[tauri::command]
fn einpx_note_delete(id: i64) -> Result<(), String> {
    notes::delete(id)
}

/// Aplikacje ZINX, które można otworzyć z tagów (tylko z tej listy)
const OPENABLE_APPS: [&str; 4] = ["ZIM", "PFMX", "CTHX", "EINPX"];

/// Otwiera aplikację ZINX przez `open -a` (macOS). Działa dla aplikacji w /Applications.
#[tauri::command]
fn einpx_open_app(app: String) -> Result<(), String> {
    if !OPENABLE_APPS.contains(&app.as_str()) {
        return Err("Nieznana aplikacja".into());
    }
    let status = std::process::Command::new("open").args(["-a", &app]).status().map_err(|e| format!("{app}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Nie udało się otworzyć {app}. Sprawdź, czy jest w /Applications."))
    }
}

#[tauri::command]
fn einpx_add_item(input: NewItem) -> Result<(), String> {
    cthx::add_item(&input)
}

#[tauri::command]
fn einpx_set_item_done(item_id: i64, date: String, done: bool) -> Result<(), String> {
    cthx::set_item_done(item_id, &date, done)
}

#[tauri::command]
fn einpx_set_habit_day(habit_id: i64, date: String, done: bool) -> Result<(), String> {
    cthx::set_habit_day(habit_id, &date, done)
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            einpx_snapshot,
            einpx_system,
            einpx_day_plan,
            einpx_add_item,
            einpx_set_item_done,
            einpx_set_habit_day,
            einpx_notes_list,
            einpx_note_add,
            einpx_note_delete,
            einpx_open_app
        ])
        .run(tauri::generate_context!())
        .expect("nie udało się uruchomić EINPX");
}

#[cfg(test)]
mod tests {
    /// Odczyt prawdziwych baz (tylko do odczytu). Uruchom: cargo test -- --ignored --nocapture
    #[test]
    #[ignore]
    fn snapshot_from_real_databases() {
        let snap = super::einpx_snapshot("2026-11-08".into(), "2026-11".into()).expect("snapshot");
        println!("{}", serde_json::to_string_pretty(&snap).unwrap());
    }
}
