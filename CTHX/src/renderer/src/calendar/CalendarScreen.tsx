import { useEffect, useMemo, useState } from 'react'
import type { Item, ItemKind } from '@shared/types'
import { dayKey, monthGrid, parseDay } from '@shared/recurrence'
import { dayTitle, itemsOn } from '../items'
import DayTimeline from './DayTimeline'

const WEEKDAYS = ['Pn', 'Wt', 'Śr', 'Cz', 'Pt', 'So', 'Nd']

/** Wąski ekran (telefon): najpierw miesiąc, dzień otwiera się osobno */
const NARROW = '(max-width: 700px)'

function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW).matches)
  useEffect(() => {
    const mq = window.matchMedia(NARROW)
    const onChange = (): void => setNarrow(mq.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])
  return narrow
}

/** Kalendarz miesiąca z wydarzeniami i agenda wybranego dnia */
export default function CalendarScreen({
  items,
  doneSet,
  day,
  onDay,
  onToggle,
  onEdit,
  onAdd,
  onSyncBirthdays,
  syncing
}: {
  items: Item[]
  doneSet: Set<string>
  day: string
  onDay: (day: string) => void
  onToggle: (item: Item, on: boolean, day: string) => void
  onEdit: (item: Item) => void
  /** Dodawanie z kalendarza: okno dostaje wybrany dzień */
  onAdd: (kind: ItemKind) => void
  onSyncBirthdays: () => void
  syncing: boolean
}): JSX.Element {
  const selected = parseDay(day)
  const narrow = useNarrow()
  const [dayOpen, setDayOpen] = useState(false)
  const [view, setView] = useState({ year: selected.getFullYear(), month: selected.getMonth() })
  const today = dayKey(new Date())
  const cells = useMemo(() => monthGrid(view.year, view.month), [view])

  const shift = (delta: number): void => {
    const d = new Date(view.year, view.month + delta, 1)
    setView({ year: d.getFullYear(), month: d.getMonth() })
  }

  // telefon: widok dnia zamiast miesiąca, z przyciskiem X powrotu
  if (narrow && dayOpen) {
    return (
      <div className="cthx-day-view">
        <div className="cthx-day-view-head">
          <h2>{dayTitle(day)}</h2>
          <div className="cthx-actions">
            <button className="cthx-ghost" onClick={() => onAdd('event')}>
              + Wydarzenie
            </button>
            <button className="cthx-primary" onClick={() => onAdd('task')}>
              + Zadanie
            </button>
          </div>
          <button className="btn cthx-day-close" onClick={() => setDayOpen(false)} aria-label="Wróć do kalendarza">
            ×
          </button>
        </div>
        <DayTimeline items={items} doneSet={doneSet} day={day} onToggle={onToggle} onEdit={onEdit} />
      </div>
    )
  }

  return (
    <div className="cthx-calendar">
      <div className="list-header">
        <div className="cthx-toolbar">
          <h2 className="cthx-month">
            {new Date(view.year, view.month, 1).toLocaleDateString('pl-PL', { month: 'long', year: 'numeric' })}
          </h2>
          <div className="cthx-actions">
            <button className="cthx-ghost" onClick={() => onAdd('event')} title="Dodaj wydarzenie na wybrany dzień">
              + Wydarzenie
            </button>
            <button className="cthx-primary" onClick={() => onAdd('task')} title="Dodaj zadanie na wybrany dzień">
              + Zadanie
            </button>
          </div>
          <div className="cthx-nav">
            <button className="btn" onClick={() => shift(-1)} aria-label="Poprzedni miesiąc">
              ‹
            </button>
            <button
              className="btn"
              onClick={() => {
                setView({ year: new Date().getFullYear(), month: new Date().getMonth() })
                onDay(today)
              }}
            >
              Dziś
            </button>
            <button className="btn" onClick={() => shift(1)} aria-label="Następny miesiąc">
              ›
            </button>
            <button className="btn cthx-sync" onClick={onSyncBirthdays} disabled={syncing}>
              {syncing ? 'Synchronizuję…' : 'Urodziny z kontaktów'}
            </button>
          </div>
        </div>
      </div>

      <div className="cthx-calendar-body">
        <div className="cthx-grid" role="grid" aria-label="Kalendarz">
          {WEEKDAYS.map((w) => (
            <div className="cthx-weekday" key={w}>
              {w}
            </div>
          ))}
          {cells.map((cell) => {
            const inMonth = parseDay(cell).getMonth() === view.month
            const dayItems = itemsOn(items, cell)
            const classes = ['cthx-cell', inMonth ? '' : 'other', cell === day ? 'selected' : '', cell === today ? 'today' : '']
            return (
              <button
                key={cell}
                className={classes.join(' ')}
                onClick={() => {
                  onDay(cell)
                  if (narrow) setDayOpen(true)
                }}
                aria-label={dayTitle(cell)}
              >
                <span className="cthx-cell-num">{parseDay(cell).getDate()}</span>
                {dayItems.slice(0, 2).map((item) => (
                  <span key={item.id} className={`cthx-cell-item ${item.kind}`}>
                    {item.title}
                  </span>
                ))}
                {dayItems.length > 2 && <span className="cthx-cell-more">+{dayItems.length - 2}</span>}
              </button>
            )
          })}
        </div>

        {!narrow && <DayTimeline items={items} doneSet={doneSet} day={day} onToggle={onToggle} onEdit={onEdit} />}
      </div>
    </div>
  )
}
