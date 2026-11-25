/** Kolumny kanbanu. Wartość zapisywana w bazie to „status”. */
export type KanbanStatus = 'todo' | 'doing' | 'done'

export const KANBAN_COLUMNS: { id: KanbanStatus; label: string }[] = [
  { id: 'todo', label: 'Do zrobienia' },
  { id: 'doing', label: 'W trakcie' },
  { id: 'done', label: 'Zrobione' }
]

/** Strona kanbanu (np. „Prezenty”). Zadania na każdej stronie są osobne. */
export interface KanbanBoard {
  id: number
  name: string
}

/** Zadanie na tablicy jednego miesiąca */
export interface KanbanTask {
  id: number
  /** RRRR-MM */
  month: string
  status: KanbanStatus
  title: string
  description: string
  boardId: number
  boardName: string
  /** zadanie z kalendarza, z którym karta jest połączona (tytuł i dzień bierze się z niego) */
  itemId: number | null
  /** dzień połączonego zadania (RRRR-MM-DD) */
  itemDate: string | null
}

export interface KanbanInput {
  /** brak = nowe zadanie */
  id?: number
  month: string
  status: KanbanStatus
  title: string
  description: string
  boardId: number
  /** zadanie z kalendarza do połączenia (przy tworzeniu) albo połączenie już istniejącej karty */
  itemId?: number | null
}
