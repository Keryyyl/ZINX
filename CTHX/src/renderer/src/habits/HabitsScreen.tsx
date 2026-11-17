import { useCallback, useEffect, useMemo, useState } from 'react'
import type { Habit } from '@shared/habits'
import { habitStats } from '@shared/habits'
import { dayKey, parseDay } from '@shared/recurrence'
import type { HabitDay } from '@shared/types'
import HabitDetail from './HabitDetail'
import HabitDialog from './HabitDialog'

/** Habit tracker: wybór „Wszystkie” albo jednego nawyku; szczegóły w dwóch kolumnach */
export default function HabitsScreen(): JSX.Element {
  const [habits, setHabits] = useState<Habit[] | null>(null)
  const [days, setDays] = useState<HabitDay[]>([])
  const [selected, setSelected] = useState<'all' | number>('all')
  const [dialog, setDialog] = useState<{ habit?: Habit } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [armed, setArmed] = useState(false)
  const today = dayKey(new Date())
  const now = parseDay(today)
  // przeglądany miesiąc: domyślnie bieżący, strzałkami można przejść do poprzednich i przyszłych
  const [view, setView] = useState({ year: now.getFullYear(), month: now.getMonth() })
  const monthName = new Date(view.year, view.month, 1).toLocaleDateString('pl-PL', { month: 'long', year: 'numeric' })
  const isCurrentMonth = view.year === now.getFullYear() && view.month === now.getMonth()
  const shiftMonth = (delta: number): void => {
    const d = new Date(view.year, view.month + delta, 1)
    setView({ year: d.getFullYear(), month: d.getMonth() })
  }

  const reload = useCallback(async (): Promise<void> => {
    const [list, daysList] = await Promise.all([window.api.habitsList(), window.api.habitDays()])
    setHabits(list)
    setDays(daysList)
  }, [])

  useEffect(() => {
    reload().catch((e: unknown) => setError(String(e)))
  }, [reload])

  // wybrany nawyk mógł zostać usunięty
  useEffect(() => {
    if (habits && typeof selected === 'number' && !habits.some((h) => h.id === selected)) setSelected('all')
  }, [habits, selected])

  const doneByHabit = useMemo(() => {
    const map = new Map<number, Set<string>>()
    for (const d of days) {
      if (!map.has(d.habitId)) map.set(d.habitId, new Set())
      map.get(d.habitId)!.add(d.date)
    }
    return map
  }, [days])

  const current = typeof selected === 'number' && habits ? habits.find((h) => h.id === selected) : undefined

  const toggle = (habitId: number, day: string, on: boolean): void => {
    setError(null)
    window.api
      .habitSetDay(habitId, day, on)
      .then(reload)
      .catch((e: unknown) => setError(String(e)))
  }

  const remove = (habit: Habit): void => {
    setError(null)
    window.api
      .habitDelete(habit.id)
      .then(() => {
        setSelected('all')
        return reload()
      })
      .catch((e: unknown) => setError(String(e)))
  }

  return (
    <div className="cthx-todo">
      <div className="list-header">
        <div className="cthx-toolbar">
          <div className="hb-title">
            <h2>Habit tracker</h2>
            <div className="cthx-seg" role="group" aria-label="Wybór miesiąca">
              <button className="cthx-seg-btn" onClick={() => shiftMonth(-1)} aria-label="Poprzedni miesiąc">
                ‹
              </button>
              <span className="cthx-seg-title">{monthName}</span>
              <button className="cthx-seg-btn" onClick={() => shiftMonth(1)} aria-label="Następny miesiąc">
                ›
              </button>
              {!isCurrentMonth && (
                <button
                  className="cthx-seg-btn cthx-seg-today"
                  onClick={() => setView({ year: now.getFullYear(), month: now.getMonth() })}
                >
                  Dziś
                </button>
              )}
            </div>
          </div>
          <div className="cthx-actions">
            <select
              className="hb-select"
              value={selected === 'all' ? 'all' : String(selected)}
              onChange={(e) => setSelected(e.target.value === 'all' ? 'all' : Number(e.target.value))}
              aria-label="Wybierz nawyk"
              disabled={!habits}
            >
              <option value="all">Wszystkie</option>
              {(habits ?? []).map((h) => (
                <option key={h.id} value={h.id}>
                  {h.name}
                </option>
              ))}
            </select>
            {current && (
              <>
                <button className="cthx-link" onClick={() => setDialog({ habit: current })}>
                  Edytuj
                </button>
                <button
                  className={`cthx-link ${armed ? 'danger' : ''}`}
                  onClick={() => (armed ? remove(current) : setArmed(true))}
                  onBlur={() => setArmed(false)}
                >
                  {armed ? 'Na pewno?' : 'Usuń'}
                </button>
              </>
            )}
            <button className="cthx-primary" onClick={() => setDialog({})}>
              Dodaj nawyk
            </button>
          </div>
        </div>
      </div>

      {error && <div className="error cthx-error">{error}</div>}

      {habits === null ? (
        <div className="fin-empty">Ładowanie…</div>
      ) : habits.length === 0 ? (
        <div className="hb-empty">
          <p>Nie masz jeszcze nawyków.</p>
          <span>Dodaj pierwszy, np. „Bieganie” z celem 12 razy w miesiącu.</span>
        </div>
      ) : current ? (
        <HabitDetail
          habit={current}
          done={doneByHabit.get(current.id) ?? new Set()}
          today={today}
          year={view.year}
          month={view.month}
          onToggle={(day, on) => toggle(current.id, day, on)}
        />
      ) : (
        <div className="hb-list">
          {habits.map((h) => {
            const done = doneByHabit.get(h.id) ?? new Set<string>()
            const s = habitStats(h, done, today, view.year, view.month)
            return <HabitRow key={h.id} habit={h} stats={s} onOpen={() => setSelected(h.id)} />
          })}
        </div>
      )}

      {dialog && (
        <HabitDialog
          habit={dialog.habit}
          onClose={() => setDialog(null)}
          onSaved={(id) => {
            setDialog(null)
            setSelected(id)
            reload().catch((e: unknown) => setError(String(e)))
          }}
        />
      )}
    </div>
  )
}

/** Wiersz na liście „Wszystkie”: trzy kolory na pasku, od lewej zielony, na szaro dni otwarte, od prawej czerwony */
function HabitRow({
  habit,
  stats,
  onOpen
}: {
  habit: Habit
  stats: ReturnType<typeof habitStats>
  onOpen: () => void
}): JSX.Element {
  const pct = (n: number): string => `${(n / stats.days) * 100}%`
  return (
    <button className="hb-row" onClick={onOpen}>
      <div className="hb-row-head">
        <div className="hb-row-name">
          <span>{habit.name}</span>
          {habit.description && <span className="hb-row-desc">{habit.description}</span>}
        </div>
        <div className="hb-row-meta">
          <span>
            <b>{stats.done}</b> / {habit.goal}×
          </span>
          <span className="hb-row-missed">{stats.missed} pominiętych</span>
        </div>
      </div>
      <div className="hb-bar" role="img" aria-label={`${stats.done} zrobione, ${stats.open} otwarte, ${stats.missed} pominięte`}>
        <span className="hb-seg done" style={{ width: pct(stats.done) }} />
        <span className="hb-seg open" style={{ width: pct(stats.open) }} />
        <span className="hb-seg missed" style={{ width: pct(stats.missed) }} />
      </div>
    </button>
  )
}
