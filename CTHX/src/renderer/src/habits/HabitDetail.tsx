import { useMemo } from 'react'
import type { Habit } from '@shared/habits'
import { dayState, habitStats, monthDays } from '@shared/habits'
import { dayKey, parseDay } from '@shared/recurrence'

const WEEKDAYS = ['Pn', 'Wt', 'Śr', 'Cz', 'Pt', 'So', 'Nd']
const RED = '#f87171'

/** Widok jednego nawyku: po lewej statystyki i wykres, po prawej kalendarz miesiąca do odhaczania */
export default function HabitDetail({
  habit,
  done,
  today,
  year,
  month,
  onToggle
}: {
  habit: Habit
  done: Set<string>
  today: string
  /** miesiąc, który jest przeglądany (może być inny niż bieżący) */
  year: number
  month: number
  onToggle: (day: string, on: boolean) => void
}): JSX.Element {
  const stats = useMemo(() => habitStats(habit, done, today, year, month), [habit, done, today, year, month])
  const cells = useMemo(() => {
    const days = monthDays(year, month)
    const offset = (new Date(year, month, 1).getDay() + 6) % 7
    return { days, offset }
  }, [year, month])

  const monthName = new Date(year, month, 1).toLocaleDateString('pl-PL', { month: 'long', year: 'numeric' })

  return (
    <div className="cthx-split hb-split">
      <section className="cthx-panel" aria-label="Statystyki nawyku">
        <header className="cthx-panel-head">
          <h3>{habit.name}</h3>
          <span className="cthx-count">{monthName}</span>
        </header>
        <div className="hb-panel-body">
          <ul className="hb-facts">
            <li>
              <span>Cel w miesiącu</span>
              <b>{habit.goal}×</b>
            </li>
            <li>
              <span>Zrobione</span>
              <b>{stats.done}</b>
            </li>
            <li>
              <span>Pominięte</span>
              <b>{stats.missed}</b>
            </li>
            <li>
              <span>Brakuje do celu</span>
              <b>{stats.goalLeft === 0 ? 'cel osiągnięty' : stats.goalLeft}</b>
            </li>
            <li>
              <span>Dni śledzone do dziś</span>
              <b>{stats.tracked}</b>
            </li>
          </ul>
          <Donut done={stats.done} missed={stats.missed} rate={stats.rate} />
        </div>
      </section>

      <section className="cthx-panel" aria-label="Kalendarz nawyku">
        <header className="cthx-panel-head">
          <h3>Kalendarz</h3>
          <span className="cthx-count">kliknij dzień, żeby odhaczyć</span>
        </header>
        <div className="hb-cal">
          {WEEKDAYS.map((w) => (
            <div className="hb-weekday" key={w}>
              {w}
            </div>
          ))}
          {Array.from({ length: cells.offset }, (_, i) => (
            <span key={`pad-${i}`} />
          ))}
          {cells.days.map((day) => {
            const state = dayState(day, done, habit, today)
            const isToday = day === today
            const selectable = day <= today && day >= habit.startDate
            const classes = ['hb-day', `is-${state}`, isToday ? 'is-today' : ''].join(' ')
            return (
              <button
                key={day}
                className={classes}
                disabled={!selectable}
                onClick={() => onToggle(day, state !== 'done')}
                aria-label={`${parseDay(day).getDate()} — ${state === 'done' ? 'zrobione' : state === 'missed' ? 'pominięte' : 'otwarte'}`}
                aria-pressed={state === 'done'}
              >
                {parseDay(day).getDate()}
              </button>
            )
          })}
        </div>
        <div className="hb-legend">
          <span>
            <i className="hb-dot done" /> zrobione
          </span>
          <span>
            <i className="hb-dot missed" /> pominięte
          </span>
        </div>
      </section>
    </div>
  )
}

/** Pierścień: zrobione (turkus) kontra pominięte (czerwień) spośród dni, które już minęły */
function Donut({ done, missed, rate }: { done: number; missed: number; rate: number }): JSX.Element {
  const r = 46
  const c = 2 * Math.PI * r
  const total = done + missed
  const doneLen = total > 0 ? (done / total) * c : 0
  const missedLen = total > 0 ? (missed / total) * c : 0
  return (
    <div className="hb-donut" role="img" aria-label={`Zrobione ${done}, pominięte ${missed}`}>
      <svg viewBox="0 0 120 120" width="150" height="150">
        <circle cx="60" cy="60" r={r} fill="none" stroke="color-mix(in srgb, var(--text) 8%, transparent)" strokeWidth="10" />
        {total > 0 && (
          <>
            <circle
              cx="60"
              cy="60"
              r={r}
              fill="none"
              stroke="var(--accent)"
              strokeWidth="10"
              strokeDasharray={`${doneLen} ${c}`}
              transform="rotate(-90 60 60)"
              strokeLinecap="round"
            />
            <circle
              cx="60"
              cy="60"
              r={r}
              fill="none"
              stroke={RED}
              strokeWidth="10"
              strokeDasharray={`${missedLen} ${c}`}
              strokeDashoffset={-doneLen}
              transform="rotate(-90 60 60)"
              strokeLinecap="round"
            />
          </>
        )}
      </svg>
      <div className="hb-donut-center">
        <b>{total > 0 ? `${rate}%` : '—'}</b>
        <span>skuteczność</span>
      </div>
    </div>
  )
}
