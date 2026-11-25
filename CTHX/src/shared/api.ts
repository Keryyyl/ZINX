import type { BirthdaySyncReport, Done, HabitDay, HabitInput, Item, ItemInput } from './types'
import type { Habit } from './habits'
import type { KanbanBoard, KanbanInput, KanbanStatus, KanbanTask } from './kanban'

/** Kontrakt między interfejsem a rdzeniem (Rust). Komendy są w src-tauri/src/commands.rs. */
export interface Api {
  itemsList(): Promise<Item[]>
  doneList(): Promise<Done[]>
  /** dodaje albo zmienia wpis (gdy jest id); zwraca id */
  itemSave(input: ItemInput): Promise<number>
  itemDelete(id: number): Promise<void>
  /** oznacza jedno wystąpienie (dzień) jako zrobione albo cofa */
  itemSetDone(itemId: number, date: string, done: boolean): Promise<void>
  /** czyta urodziny z kontaktów (pierwszy raz pyta o dostęp) i zapisuje je jako coroczne wydarzenia */
  birthdaysSync(): Promise<BirthdaySyncReport>
  habitsList(): Promise<Habit[]>
  /** dodaje albo zmienia nawyk (gdy jest id); zwraca id */
  habitSave(input: HabitInput): Promise<number>
  habitDelete(id: number): Promise<void>
  habitDays(): Promise<HabitDay[]>
  habitSetDay(habitId: number, date: string, done: boolean): Promise<void>
  /** zadania kanbanu miesiąca RRRR-MM; currentMonth (RRRR-MM) przenosi zaległe „Do zrobienia” */
  kanbanList(month: string, currentMonth: string): Promise<KanbanTask[]>
  /** dodaje albo zmienia zadanie (gdy jest id); zwraca id */
  kanbanSave(input: KanbanInput): Promise<number>
  kanbanDelete(id: number): Promise<void>
  /** przenosi kartę do kolumny i przed kartę `beforeId` (null = na koniec kolumny) */
  kanbanMove(id: number, status: KanbanStatus, beforeId: number | null): Promise<void>
  /** strony kanbanu w kolejności dodania */
  kanbanBoards(): Promise<KanbanBoard[]>
  kanbanBoardCreate(name: string): Promise<number>
  /** ile zadań ma strona (ze wszystkich miesięcy) */
  kanbanBoardTaskCount(id: number): Promise<number>
  /** usuwa stronę razem z jej zadaniami */
  kanbanBoardDelete(id: number): Promise<void>
}
