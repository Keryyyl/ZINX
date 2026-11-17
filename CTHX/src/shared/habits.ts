import { addDays, dayKey } from './recurrence'

export interface Habit {
  id: number
  name: string
  description: string
  /** ile razy w miesiącu */
  goal: number
  /** pierwszy dzień śledzenia, RRRR-MM-DD */
  startDate: string
}

/** Stan dnia nawyku w danym miesiącu */
export type DayState = 'done' | 'missed' | 'pending' | 'untracked'

/** Wszystkie dni miesiąca jako RRRR-MM-DD */
export function monthDays(year: number, month: number): string[] {
  const count = new Date(year, month + 1, 0).getDate()
  const first = dayKey(new Date(year, month, 1))
  return Array.from({ length: count }, (_, i) => addDays(first, i))
}

/** Stan dnia: zrobiony, pominięty (minął, bez odhaczenia), jeszcze otwarty (dziś/przyszły) albo sprzed startu */
export function dayState(day: string, done: Set<string>, habit: Pick<Habit, 'startDate'>, today: string): DayState {
  if (done.has(day)) return 'done'
  if (day < habit.startDate) return 'untracked'
  if (day >= today) return 'pending'
  return 'missed'
}

export interface HabitStats {
  /** dni w miesiącu */
  days: number
  done: number
  missed: number
  /** pozostałe dni: jeszcze nie nastały albo nie są śledzone */
  open: number
  goal: number
  /** ile brakuje do celu (0 = cel osiągnięty) */
  goalLeft: number
  /** dni śledzone do dziś włącznie (od startu nawyku lub początku miesiąca) */
  tracked: number
  /** odsetek zrobionych spośród dni śledzonych do dziś */
  rate: number
}

export function habitStats(habit: Habit, done: Set<string>, today: string, year: number, month: number): HabitStats {
  const days = monthDays(year, month)
  let doneCount = 0
  let missed = 0
  let tracked = 0
  for (const day of days) {
    const state = dayState(day, done, habit, today)
    if (state === 'done') doneCount++
    if (state === 'missed') missed++
    if (state === 'done' || state === 'missed') tracked++
  }
  const open = days.length - doneCount - missed
  return {
    days: days.length,
    done: doneCount,
    missed,
    open,
    goal: habit.goal,
    goalLeft: Math.max(0, habit.goal - doneCount),
    tracked,
    rate: tracked > 0 ? Math.round((doneCount / tracked) * 100) : 0
  }
}
