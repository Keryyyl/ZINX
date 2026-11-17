import { useEffect, useState } from 'react'
import { parseMoney } from './format'

/** Filtry transakcji (bez kategorii i konta, te są na pasku). Kwoty w groszach, daty RRRR-MM-DD. */
export interface Filters {
  kind: 'income' | 'spending' | null
  dateFrom: string | null
  dateTo: string | null
  amountMin: number | null
  amountMax: number | null
}

export const NO_FILTERS: Filters = { kind: null, dateFrom: null, dateTo: null, amountMin: null, amountMax: null }

export function countFilters(f: Filters | null): number {
  if (!f) return 0
  return [f.kind, f.dateFrom, f.dateTo, f.amountMin, f.amountMax].filter((v) => v !== null).length
}

/** Małe okno z filtrami. Zamyka się kliknięciem obok, przyciskiem X albo Escape. */
export function FilterPopup({
  initial,
  onApply,
  onReset,
  onClose
}: {
  initial: Filters | null
  onApply: (filters: Filters) => void
  onReset: () => void
  onClose: () => void
}): JSX.Element {
  const [kind, setKind] = useState<string>(initial?.kind ?? '')
  const [dateFrom, setDateFrom] = useState(initial?.dateFrom ?? '')
  const [dateTo, setDateTo] = useState(initial?.dateTo ?? '')
  const [amountFrom, setAmountFrom] = useState(initial?.amountMin !== null && initial?.amountMin !== undefined ? String(initial.amountMin / 100) : '')
  const [amountTo, setAmountTo] = useState(initial?.amountMax !== null && initial?.amountMax !== undefined ? String(initial.amountMax / 100) : '')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const apply = (e: React.FormEvent): void => {
    e.preventDefault()
    setError(null)
    const min = amountFrom.trim() === '' ? null : parseMoney(amountFrom)
    const max = amountTo.trim() === '' ? null : parseMoney(amountTo)
    if ((amountFrom.trim() !== '' && min === null) || (amountTo.trim() !== '' && max === null)) {
      setError('Kwota: podaj liczbę, np. 1200 albo 1200,50')
      return
    }
    if (min !== null && max !== null && min > max) {
      setError('Kwota „od” nie może być większa niż „do”')
      return
    }
    if (dateFrom && dateTo && dateFrom > dateTo) {
      setError('Data „od” nie może być późniejsza niż „do”')
      return
    }
    onApply({
      kind: kind === '' ? null : (kind as 'income' | 'spending'),
      dateFrom: dateFrom || null,
      dateTo: dateTo || null,
      amountMin: min,
      amountMax: max
    })
  }

  const reset = (): void => {
    setKind('')
    setDateFrom('')
    setDateTo('')
    setAmountFrom('')
    setAmountTo('')
    setError(null)
    onReset()
  }

  return (
    <div className="fin-popup-overlay" onClick={onClose}>
      <form className="fin-popup" role="dialog" aria-label="Filtry transakcji" onClick={(e) => e.stopPropagation()} onSubmit={apply}>
        <header className="fin-popup-head">
          <h3>Filtry</h3>
          <button type="button" className="fin-popup-close" onClick={onClose} aria-label="Zamknij">
            ×
          </button>
        </header>
        <div className="fin-popup-body">
          <label>
            Rodzaj
            <select value={kind} onChange={(e) => setKind(e.target.value)}>
              <option value="">Wszystkie</option>
              <option value="income">Wpływ</option>
              <option value="spending">Płatność</option>
            </select>
          </label>
          <div className="fin-popup-row">
            <label>
              Data od
              <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} />
            </label>
            <label>
              Data do
              <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} />
            </label>
          </div>
          <div className="fin-popup-row">
            <label>
              Wielkość od (zł)
              <input value={amountFrom} onChange={(e) => setAmountFrom(e.target.value)} inputMode="decimal" placeholder="np. 1000" />
            </label>
            <label>
              Wielkość do (zł)
              <input value={amountTo} onChange={(e) => setAmountTo(e.target.value)} inputMode="decimal" placeholder="np. 1500" />
            </label>
          </div>
          {error && <div className="error">{error}</div>}
        </div>
        <footer className="fin-popup-foot">
          <button type="button" className="fin-ghost fin-ghost-inline" onClick={reset}>
            Zrestartuj Filtry
          </button>
          <button type="submit" className="fin-ghost fin-ghost-inline is-on">
            Wyszukaj
          </button>
        </footer>
      </form>
    </div>
  )
}
