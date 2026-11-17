import type { Item } from './types'
import { minutesOf } from './recurrence'

/** Zadanie bez godziny końca zajmuje na osi tyle minut (do ustawiania kolumn i wysokości). */
export const TASK_MINUTES = 30

export interface Placed {
  item: Item
  /** minuty od północy */
  top: number
  duration: number
  /** kolumna w obrębie grupy nakładających się wpisów */
  column: number
  /** ile kolumn ma ta grupa; szerokość wpisu = 1 / columns */
  columns: number
}

export interface DayLayout {
  timed: Placed[]
  /** zadania bez godziny: idą osobno, na górze dnia */
  untimed: Item[]
}

function span(item: Item): { start: number; end: number } {
  const start = minutesOf(item.startTime)
  const endFromItem = item.kind === 'event' && item.endTime ? minutesOf(item.endTime) : start + TASK_MINUTES
  return { start, end: Math.min(Math.max(endFromItem, start + 1), 24 * 60) }
}

/**
 * Układa wpisy dnia na osi: wpisy, które się nakładają, dzielą szerokość na kolumny.
 * Grupa to łańcuch nakładających się wpisów; wszystkie w grupie dostają tę samą liczbę kolumn.
 */
export function layoutDay(items: Item[]): DayLayout {
  const untimed = items.filter((i) => !i.startTime)
  const timedItems = items
    .filter((i) => i.startTime)
    .map((item) => ({ item, ...span(item) }))
    .sort((a, b) => a.start - b.start || b.end - a.end || a.item.id - b.item.id)

  const timed: Placed[] = []
  let cluster: typeof timedItems = []
  let clusterEnd = -1

  const flush = (): void => {
    // przypisanie kolumn: pierwsza kolumna, w której poprzedni wpis już się skończył
    const columnEnds: number[] = []
    const placed: Placed[] = cluster.map((entry) => {
      let column = columnEnds.findIndex((end) => end <= entry.start)
      if (column === -1) {
        column = columnEnds.length
        columnEnds.push(entry.end)
      } else {
        columnEnds[column] = entry.end
      }
      return { item: entry.item, top: entry.start, duration: entry.end - entry.start, column, columns: 0 }
    })
    for (const p of placed) p.columns = columnEnds.length
    timed.push(...placed)
    cluster = []
  }

  for (const entry of timedItems) {
    if (cluster.length > 0 && entry.start >= clusterEnd) flush()
    cluster.push(entry)
    clusterEnd = Math.max(clusterEnd, entry.end)
  }
  if (cluster.length > 0) flush()

  return { timed, untimed }
}
