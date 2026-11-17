import type { Item } from '@shared/types'
import { minutesOf, occursOn, parseDay } from '@shared/recurrence'

/** Wpisy, które wypadają danego dnia, posortowane po godzinie (bez godziny na końcu) */
export function itemsOn(items: Item[], day: string): Item[] {
  return items
    .filter((item) => occursOn(item, day))
    .sort((a, b) => minutesOf(a.startTime) - minutesOf(b.startTime) || a.title.localeCompare(b.title, 'pl'))
}

/** „09:00 – 10:30”, „09:00” albo nic */
export function timeLabel(item: Item): string {
  if (!item.startTime) return ''
  if (item.kind === 'event' && item.endTime) return `${item.startTime} – ${item.endTime}`
  return item.startTime
}

export function dayTitle(day: string): string {
  return parseDay(day).toLocaleDateString('pl-PL', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })
}

export function isDone(doneSet: Set<string>, item: Item, day: string): boolean {
  return doneSet.has(`${item.id}|${day}`)
}

/** Długość wydarzenia z godzinami: „1 h 30 min”, „45 min” albo pusty napis */
export function durationLabel(item: Item): string {
  if (item.kind !== 'event' || !item.startTime || !item.endTime) return ''
  const minutes = minutesOf(item.endTime) - minutesOf(item.startTime)
  if (minutes <= 0) return ''
  const h = Math.floor(minutes / 60)
  const m = minutes % 60
  if (h === 0) return `${m} min`
  return m === 0 ? `${h} h` : `${h} h ${m} min`
}

/** Ile wpisów dnia jest zrobionych (zadania i wydarzenia mają checkbox) */
export function dayProgress(list: Item[], doneSet: Set<string>, day: string): { done: number; total: number } {
  const done = list.filter((item) => isDone(doneSet, item, day)).length
  return { done, total: list.length }
}
