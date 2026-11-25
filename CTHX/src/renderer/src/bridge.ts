import { invoke } from '@tauri-apps/api/core'
import type { Api } from '@shared/api'

/**
 * Most do rdzenia w Rust. Nazwa komendy to nazwa funkcji `cthx_*` w commands.rs,
 * a argumenty idą pod nazwami parametrów (camelCase, Tauri sam zmienia je na snake_case).
 */
export const api: Api = {
  itemsList: () => invoke('cthx_items_list'),
  doneList: () => invoke('cthx_done_list'),
  itemSave: (input) => invoke('cthx_item_save', { input }),
  itemDelete: (id) => invoke('cthx_item_delete', { id }),
  itemSetDone: (itemId, date, done) => invoke('cthx_item_set_done', { itemId, date, done }),
  birthdaysSync: () => invoke('cthx_birthdays_sync'),
  habitsList: () => invoke('cthx_habits_list'),
  habitSave: (input) => invoke('cthx_habit_save', { input }),
  habitDelete: (id) => invoke('cthx_habit_delete', { id }),
  habitDays: () => invoke('cthx_habit_days'),
  habitSetDay: (habitId, date, done) => invoke('cthx_habit_set_day', { habitId, date, done }),
  kanbanList: (month, currentMonth) => invoke('cthx_kanban_list', { month, currentMonth }),
  kanbanSave: (input) => invoke('cthx_kanban_save', { input }),
  kanbanDelete: (id) => invoke('cthx_kanban_delete', { id }),
  kanbanMove: (id, status, beforeId) => invoke('cthx_kanban_move', { id, status, beforeId }),
  kanbanBoards: () => invoke('cthx_kanban_boards'),
  kanbanBoardCreate: (name) => invoke('cthx_kanban_board_create', { name }),
  kanbanBoardTaskCount: (id) => invoke('cthx_kanban_board_task_count', { id }),
  kanbanBoardDelete: (id) => invoke('cthx_kanban_board_delete', { id })
}
