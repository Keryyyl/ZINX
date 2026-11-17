import type { DailyFlow } from '@shared/types'

/** Kwoty przychodzą w groszach (liczba całkowita); zamiana na tekst dopiero tutaj. */
export function formatMoney(grosze: number, currency = 'PLN', signed = false): string {
  const text = new Intl.NumberFormat('pl-PL', { style: 'currency', currency }).format(Math.abs(grosze) / 100)
  if (grosze < 0) return `−${text}`
  return signed && grosze > 0 ? `+${text}` : text
}

/** 2026-11-04 -> 04.11.2026 */
export function formatDate(iso: string): string {
  const [y, m, d] = iso.split('-')
  return d && m && y ? `${d}.${m}.${y}` : iso
}

/** Pole kwoty w zł („1234,5”) -> grosze; null gdy pusto albo to nie liczba */
export function parseMoney(text: string): number | null {
  const clean = text.trim().replace(/\s/g, '').replace(',', '.')
  if (clean === '') return null
  const value = Number(clean)
  return Number.isFinite(value) ? Math.round(value * 100) : null
}

/** Grosze -> tekst do pola kwoty, bez symbolu waluty */
export function moneyInput(grosze: number): string {
  return (grosze / 100).toFixed(2).replace('.', ',')
}

/** Bieżący miesiąc w formacie RRRR-MM */
export function currentMonth(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`
}

export function shiftMonth(month: string, delta: number): string {
  const [y, m] = month.split('-').map(Number)
  const index = y * 12 + (m - 1) + delta
  return `${Math.floor(index / 12)}-${String((index % 12) + 1).padStart(2, '0')}`
}

/** RRRR-MM -> „listopad 2026” */
export function monthLabel(month: string): string {
  const [y, m] = month.split('-').map(Number)
  return new Date(y, m - 1, 1).toLocaleDateString('pl-PL', { month: 'long', year: 'numeric' })
}

/** Procent z dokładnością do setnych: 0,05% dla 1 zł z 2000 zł, 100% dla celu osiągniętego */
export function formatPercent(value: number): string {
  return `${value.toLocaleString('pl-PL', { maximumFractionDigits: 2 })}%`
}

/** Data lokalna w formacie RRRR-MM-DD */
export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

/** Skumulowany przepływ netto dla ostatnich `days` dni (łącznie z dzisiaj), po jednym punkcie na dzień */
export function cumulativeSeries(flow: DailyFlow[], days = 30): number[] {
  const byDate = new Map(flow.map((f) => [f.date, f.net]))
  const day = new Date()
  day.setDate(day.getDate() - (days - 1))
  const out: number[] = []
  let sum = 0
  for (let i = 0; i < days; i++) {
    sum += byDate.get(isoDate(day)) ?? 0
    out.push(sum)
    day.setDate(day.getDate() + 1)
  }
  return out
}

/** Początek okna 30 dni w formacie RRRR-MM-DD */
export function windowStart(days = 30): string {
  const day = new Date()
  day.setDate(day.getDate() - (days - 1))
  return isoDate(day)
}
