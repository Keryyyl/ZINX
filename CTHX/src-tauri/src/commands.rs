//! Komendy wywoływane z interfejsu. Nazwy i kontrakt: src/shared/api.ts i src/renderer/src/bridge.ts.

use crate::birthdays::{self, SyncReport};
use crate::contacts;
use crate::db::{self, Db, Done, Item, ItemInput};
use crate::error::Error;
use crate::habits::{self, Habit, HabitDay, HabitInput};
use crate::kanban::{self, KanbanInput, KanbanTask};
use crate::error::Result;
use rusqlite::Connection;
use std::sync::MutexGuard;
use tauri::State;

pub struct AppState {
    pub db: Db,
}

impl AppState {
    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.db.lock()
    }
}

#[tauri::command]
pub fn cthx_items_list(state: State<AppState>) -> Result<Vec<Item>> {
    db::list(&*state.conn()?)
}

#[tauri::command]
pub fn cthx_done_list(state: State<AppState>) -> Result<Vec<Done>> {
    db::done_list(&*state.conn()?)
}

#[tauri::command]
pub fn cthx_item_save(state: State<AppState>, input: ItemInput) -> Result<i64> {
    db::save(&*state.conn()?, &input)
}

#[tauri::command]
pub fn cthx_item_delete(state: State<AppState>, id: i64) -> Result<()> {
    db::delete(&*state.conn()?, id)
}

#[tauri::command]
pub fn cthx_item_set_done(state: State<AppState>, item_id: i64, date: String, done: bool) -> Result<()> {
    db::set_done(&*state.conn()?, item_id, &date, done)
}

/// Czyta urodziny z kontaktów i zapisuje je jako coroczne wydarzenia. Pierwsze uruchomienie pyta o dostęp.
#[tauri::command]
pub async fn cthx_birthdays_sync(state: State<'_, AppState>) -> Result<SyncReport> {
    let people = tauri::async_runtime::spawn_blocking(contacts::birthdays)
        .await
        .map_err(|e| Error(e.to_string()))?
        .map_err(Error::from)?;
    let conn = state.conn()?;
    birthdays::sync(&conn, &people)
}

#[tauri::command]
pub fn cthx_habits_list(state: State<AppState>) -> Result<Vec<Habit>> {
    habits::list(&*state.conn()?)
}

#[tauri::command]
pub fn cthx_habit_save(state: State<AppState>, input: HabitInput) -> Result<i64> {
    habits::save(&*state.conn()?, &input)
}

#[tauri::command]
pub fn cthx_habit_delete(state: State<AppState>, id: i64) -> Result<()> {
    habits::delete(&*state.conn()?, id)
}

#[tauri::command]
pub fn cthx_habit_days(state: State<AppState>) -> Result<Vec<HabitDay>> {
    habits::days(&*state.conn()?)
}

#[tauri::command]
pub fn cthx_habit_set_day(state: State<AppState>, habit_id: i64, date: String, done: bool) -> Result<()> {
    habits::set_day(&*state.conn()?, habit_id, &date, done)
}

#[tauri::command]
pub fn cthx_kanban_list(state: State<AppState>, month: String, current_month: String) -> Result<Vec<KanbanTask>> {
    kanban::list(&*state.conn()?, &month, &current_month)
}

#[tauri::command]
pub fn cthx_kanban_save(state: State<AppState>, input: KanbanInput) -> Result<i64> {
    kanban::save(&*state.conn()?, &input)
}

#[tauri::command]
pub fn cthx_kanban_delete(state: State<AppState>, id: i64) -> Result<()> {
    kanban::delete(&*state.conn()?, id)
}
