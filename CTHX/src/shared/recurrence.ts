import type { Item } from './types'

/** Data lokalna jako RRRR-MM-DD */
export function dayKey(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${d.getFullYear()}-${m}-${day}`
}

export function parseDay(key: string): Date {
  const [y, m, d] = key.split('-').map(Number)
  return new Date(y, m - 1, d)
}

export function addDays(key: string, delta: number): string {
  const d = parseDay(key)
  d.setDate(d.getDate() + delta)
  return dayKey(d)
}

/** Liczba dni od 1970-01-01 w kalendarzu (bez przesunięć strefy czasowej) */
function dayNumber(key: string): number {
  const [y, m, d] = key.split('-').map(Number)
  return Date.UTC(y, m - 1, d) / 86_400_000
}

/** Czy wpis wypada tego dnia (bez sprawdzania wykonania) */
export function occursOn(item: Pick<Item, 'date' | 'recurrence'>, day: string): boolean {
  if (day < item.date) return false
  switch (item.recurrence) {
    case 'none':
      return day === item.date
    case 'daily':
      return true
    case 'weekly':
      return (dayNumber(day) - dayNumber(item.date)) % 7 === 0
    case 'monthly':
      return parseDay(day).getDate() === parseDay(item.date).getDate()
    case 'yearly': {
      const d = parseDay(day)
      const start = parseDay(item.date)
      return d.getMonth() === start.getMonth() && d.getDate() === start.getDate()
    }
  }
}

/** Siatka miesiąca: 6 tygodni od poniedziałku, w tym dni sąsiednich miesięcy */
export function monthGrid(year: number, month: number): string[] {
  const first = new Date(year, month, 1)
  const offset = (first.getDay() + 6) % 7
  const start = dayKey(new Date(year, month, 1 - offset))
  return Array.from({ length: 42 }, (_, i) => addDays(start, i))
}

/** Godziny jako minuty od północy, do sortowania i liczenia długości */
export function minutesOf(time: string | null): number {
  if (!time) return 24 * 60
  const [h, m] = time.split(':').map(Number)
  return h * 60 + m
}
