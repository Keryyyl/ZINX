/** Kolumny kanbanu. Wartość zapisywana w bazie to „status”. */
export type KanbanStatus = 'todo' | 'doing' | 'done'

export const KANBAN_COLUMNS: { id: KanbanStatus; label: string }[] = [
  { id: 'todo', label: 'Do zrobienia' },
  { id: 'doing', label: 'W trakcie' },
  { id: 'done', label: 'Zrobione' }
]

/** Zadanie na tablicy jednego miesiąca */
export interface KanbanTask {
  id: number
  /** RRRR-MM */
  month: string
  status: KanbanStatus
  title: string
  description: string
}

export interface KanbanInput {
  /** brak = nowe zadanie */
  id?: number
  month: string
  status: KanbanStatus
  title: string
  description: string
}
