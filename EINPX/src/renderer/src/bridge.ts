import { invoke } from '@tauri-apps/api/core'
import type { Api, Note, PlanItem, Snapshot, SystemInfo } from '@shared/api'

/** Jedyne miejsce, które zna nazwy komend Rust */
export const api: Api = {
  snapshot: (today, month) => invoke<Snapshot>('einpx_snapshot', { today, month }),
  system: () => invoke<SystemInfo>('einpx_system'),
  dayPlan: (date) => invoke<PlanItem[]>('einpx_day_plan', { date }),
  addItem: (input) => invoke<void>('einpx_add_item', { input }),
  notesList: () => invoke<Note[]>('einpx_notes_list'),
  noteAdd: (text) => invoke<void>('einpx_note_add', { text }),
  noteDelete: (id) => invoke<void>('einpx_note_delete', { id }),
  openApp: (app) => invoke<void>('einpx_open_app', { app }),
  setItemDone: (itemId, date, done) => invoke<void>('einpx_set_item_done', { itemId, date, done }),
  setHabitDay: (habitId, date, done) => invoke<void>('einpx_set_habit_day', { habitId, date, done })
}
