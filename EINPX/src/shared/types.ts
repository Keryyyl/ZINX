/** Rodzaj wpisu: zadanie do wykonania albo wydarzenie z godzinami */
export type ItemKind = 'task' | 'event'

/** Powtarzalność; „none” = jednorazowe, „yearly” = co rok (urodziny) */
export type Recurrence = 'none' | 'daily' | 'weekly' | 'monthly' | 'yearly'

/** Zadanie albo wydarzenie. Daty w formacie RRRR-MM-DD, godziny GG:MM. */
export interface Item {
  id: number
  kind: ItemKind
  title: string
  description: string
  /** pierwszy dzień wystąpienia */
  date: string
  startTime: string | null
  /** tylko wydarzenia */
  endTime: string | null
  recurrence: Recurrence
}

export interface ItemInput {
  /** brak = nowy wpis */
  id?: number
  kind: ItemKind
  title: string
  description: string
  date: string
  startTime: string | null
  endTime: string | null
  recurrence: Recurrence
}

/** Wykonane wystąpienie: id wpisu i dzień, którego dotyczy */
export interface Done {
  itemId: number
  date: string
}

/** Wynik synchronizacji urodzin z kontaktów */
export interface BirthdaySyncReport {
  added: number
  updated: number
  /** wpisy kontaktów, których już nie ma w książce */
  removed: number
  total: number
}

/** Dane do zapisu nawyku (bez id przy nowym) */
export interface HabitInput {
  id?: number
  name: string
  description: string
  goal: number
  startDate: string
}

/** Odhaczony dzień nawyku */
export interface HabitDay {
  habitId: number
  date: string
}
