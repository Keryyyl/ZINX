/** Data lokalna jako RRRR-MM-DD */
export function dayKey(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${d.getFullYear()}-${m}-${day}`
}

/** Miesiąc lokalny jako RRRR-MM */
export function monthKey(d: Date): string {
  return dayKey(d).slice(0, 7)
}

const money = new Intl.NumberFormat('pl-PL', { style: 'currency', currency: 'PLN' })

/** Grosze na złote, np. 123456 → „1 234,56 zł” */
export function zl(grosze: number): string {
  return money.format(grosze / 100)
}

/** „10:00–12:00”, „10:00” albo nic */
export function timeRange(start: string | null, end: string | null): string {
  if (!start) return ''
  return end ? `${start}–${end}` : start
}

export function plural(n: number, one: string, few: string, many: string): string {
  if (n === 1) return one
  const last = n % 10
  const lastTwo = n % 100
  return last >= 2 && last <= 4 && (lastTwo < 12 || lastTwo > 14) ? few : many
}

/** Dzień przesunięty o `delta` dni, RRRR-MM-DD */
export function shiftDay(key: string, delta: number): string {
  const [y, m, d] = key.split('-').map(Number)
  return dayKey(new Date(y, m - 1, d + delta))
}

/** Tytuł dnia, np. „sobota, 8 listopada 2026” */
export function dayTitle(key: string): string {
  const [y, m, d] = key.split('-').map(Number)
  return new Date(y, m - 1, d).toLocaleDateString('pl-PL', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })
}
