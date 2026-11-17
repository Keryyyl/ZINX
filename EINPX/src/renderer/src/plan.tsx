import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type MouseEvent } from 'react'
import type { ItemKind, NewItem, PlanItem } from '@shared/api'
import type { Item } from '@shared/types'
import { layoutDay, type Placed } from '@shared/timeline'
import { dayKey, dayTitle, timeRange } from './format'
import { AppTag, Card } from './cards'

/** Wysokość jednej godziny na osi, w pikselach (jak w CTHX, odchudzone) */
const HOUR_PX = 40
/** Oś zawsze obejmuje całą dobę; na ekranie widać tyle godzin, a resztę przewija się */
const DAY_START = 0
const DAY_END = 24
/** Ile godzin mieści się w widoku bez przewijania */
const VIEW_HOURS = 10
/** Od której godziny widok startuje po otwarciu */
const VIEW_START = 7
const pad = (n: number): string => String(n).padStart(2, '0')

export interface PlanCardProps {
  /** zmienia się przy każdym odświeżeniu, żeby plan przeładował dane */
  tick: number
  onChange: (fn: () => Promise<void>) => Promise<void>
}

/** Plan dzisiejszego dnia jak w CTHX: wpisy bez godziny na górze, poniżej oś 00–24. Dodawanie przez „+”. */
export default function PlanCard({ tick, onChange }: PlanCardProps): JSX.Element {
  const today = dayKey(new Date())
  const [plan, setPlan] = useState<PlanItem[]>([])
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)

  const load = useCallback(async (): Promise<void> => {
    try {
      setPlan(await window.api.dayPlan(today))
      setError(null)
    } catch (e: unknown) {
      setError(String(e))
    }
  }, [today])

  useEffect(() => {
    void load()
  }, [load, tick])

  // zadania i wydarzenia w formie wspólnej z CTHX (układ osi liczy src/shared/timeline.ts)
  const items: Item[] = useMemo(
    () => plan.map((p) => ({ id: p.id, kind: p.kind, title: p.title, description: p.description, date: today, startTime: p.startTime, endTime: p.endTime, recurrence: 'none' })),
    [plan, today]
  )
  const layout = useMemo(() => layoutDay(items), [items])
  const startHour = DAY_START
  const endHour = DAY_END
  const hours = useMemo(() => Array.from({ length: endHour - startHour }, (_, i) => startHour + i), [startHour, endHour])
  const scroller = useRef<HTMLDivElement>(null)

  // po otwarciu widok pokazuje poranek, a reszta dnia czeka w przewijaniu
  useEffect(() => {
    scroller.current?.scrollTo({ top: (VIEW_START - startHour) * HOUR_PX })
  }, [startHour])
  const doneIds = useMemo(() => new Set(plan.filter((p) => p.done).map((p) => p.id)), [plan])
  // wydarzenia z godziną są już na osi; na górze zostają tylko całodniowe
  const events = plan.filter((p) => p.kind === 'event' && !p.startTime)
  const untimedTasks = layout.untimed.filter((i) => i.kind === 'task')

  const toggle = (id: number, done: boolean): void => {
    void onChange(() => window.api.setItemDone(id, today, done))
  }

  return (
    <Card
      title="Plan dnia"
      subtitle={dayTitle(today)}
      className="einpx-plan-card"
      action={
        <>
          <AppTag app="CTHX" />
        <button className="einpx-btn plus-btn" onClick={() => setAdding(true)} aria-label="Dodaj zadanie lub wydarzenie" title="Dodaj">
          +
        </button>
        </>
      }
    >
      {error && <p className="einpx-section-error">{error}</p>}

      <aside className="einpx-plan">
        {(events.length > 0 || untimedTasks.length > 0) && (
          <div className="einpx-plan-strip">
            {events.length > 0 && (
              <div className="einpx-plan-group">
                <span className="einpx-plan-label">Cały dzień</span>
                {events.map((p) => (
                  <Chip key={p.id} done={doneIds.has(p.id)} title={p.title} time={timeRange(p.startTime, p.endTime)} onToggle={(d) => toggle(p.id, d)} />
                ))}
              </div>
            )}
            {untimedTasks.length > 0 && (
              <div className="einpx-plan-group">
                <span className="einpx-plan-label">Bez godziny</span>
                {untimedTasks.map((i) => (
                  <Chip key={i.id} done={doneIds.has(i.id)} title={i.title} onToggle={(d) => toggle(i.id, d)} />
                ))}
              </div>
            )}
          </div>
        )}

        <div className="einpx-plan-scroll" ref={scroller} style={{ height: VIEW_HOURS * HOUR_PX }}>
          <div className="einpx-plan-canvas" style={{ height: hours.length * HOUR_PX }}>
            {hours.map((h) => (
              <div key={h} className="einpx-hour" style={{ top: (h - startHour) * HOUR_PX }}>
                <span>{pad(h)}:00</span>
              </div>
            ))}

            {layout.timed.map((p) => (
              <PlacedItem key={p.item.id} placed={p} offset={startHour * 60} done={doneIds.has(p.item.id)} onToggle={(d) => toggle(p.item.id, d)} />
            ))}

            <NowLine startHour={startHour} endHour={endHour} />
          </div>
        </div>
      </aside>

      {adding && (
        <AddItemDialog
          today={today}
          onClose={() => setAdding(false)}
          onAdd={async (input) => {
            await onChange(() => window.api.addItem(input))
            setAdding(false)
          }}
        />
      )}
    </Card>
  )
}

/** Kliknięcie w cały wpis odznacza/zaznacza go; checkbox sam obsługuje swoje kliknięcie */
function clickToggle(e: MouseEvent, toggle: () => void): void {
  if ((e.target as HTMLElement).tagName === 'INPUT') return
  toggle()
}

function Chip({ done, title, time, onToggle }: { done: boolean; title: string; time?: string; onToggle: (done: boolean) => void }): JSX.Element {
  return (
    <div className={`einpx-plan-chip ${done ? 'is-done' : ''}`} onClick={(e) => clickToggle(e, () => onToggle(!done))}>
      <input type="checkbox" className="einpx-check" checked={done} onChange={(e) => onToggle(e.target.checked)} aria-label={`Zrobione: ${title}`} />
      <span className="einpx-plan-title">{title}</span>
      {time && <span className="einpx-plan-time">{time}</span>}
    </div>
  )
}

function PlacedItem({ placed, offset, done, onToggle }: { placed: Placed; offset: number; done: boolean; onToggle: (done: boolean) => void }): JSX.Element {
  const { item, top, duration, column, columns } = placed
  const geometry = {
    top: ((top - offset) / 60) * HOUR_PX,
    left: `calc(${(column / columns) * 100}% + 4px)`,
    width: `calc(${100 / columns}% - 8px)`
  }

  if (item.kind === 'task') {
    return (
      <div className={`einpx-plan-task ${done ? 'is-done' : ''}`} style={geometry} onClick={(e) => clickToggle(e, () => onToggle(!done))}>
        <input type="checkbox" className="einpx-check" checked={done} onChange={(e) => onToggle(e.target.checked)} aria-label={`Zrobione: ${item.title}`} />
        <span className="einpx-plan-title">{item.title}</span>
      </div>
    )
  }

  return (
    <div
      className={`einpx-plan-event ${done ? 'is-done' : ''}`}
      style={{ ...geometry, height: Math.max(22, (duration / 60) * HOUR_PX - 2) }}
      title={item.description || item.title}
      onClick={(e) => clickToggle(e, () => onToggle(!done))}
    >
      <span className="einpx-plan-title">{item.title}</span>
      <span className="einpx-plan-time">{timeRange(item.startTime, item.endTime)}</span>
      {item.description && <span className="einpx-plan-desc">{item.description}</span>}
    </div>
  )
}

function NowLine({ startHour, endHour }: { startHour: number; endHour: number }): JSX.Element | null {
  const [now, setNow] = useState(minutesNow)
  useEffect(() => {
    const id = window.setInterval(() => setNow(minutesNow()), 60_000)
    return () => window.clearInterval(id)
  }, [])
  if (now < startHour * 60 || now > endHour * 60) return null
  return <div className="einpx-now" style={{ top: ((now - startHour * 60) / 60) * HOUR_PX }} aria-hidden />
}

function minutesNow(): number {
  const d = new Date()
  return d.getHours() * 60 + d.getMinutes()
}

/** Okno dodawania: nakładka z formularzem; Esc albo kliknięcie tła zamyka */
function AddItemDialog({ today, onClose, onAdd }: { today: string; onClose: () => void; onAdd: (input: NewItem) => Promise<void> }): JSX.Element {
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div className="einpx-modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="einpx-modal" role="dialog" aria-modal="true" aria-label="Dodaj wpis">
        <div className="einpx-modal-head">
          <h3>Dodaj na dziś</h3>
          <button className="einpx-btn small-btn" onClick={onClose} aria-label="Zamknij">
            ×
          </button>
        </div>
        <AddItemForm today={today} onAdd={onAdd} />
      </div>
    </div>
  )
}

function AddItemForm({ today, onAdd }: { today: string; onAdd: (input: NewItem) => Promise<void> }): JSX.Element {
  const [kind, setKind] = useState<ItemKind>('task')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [startTime, setStartTime] = useState('')
  const [endTime, setEndTime] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async (e: FormEvent): Promise<void> => {
    e.preventDefault()
    if (!title.trim()) return
    setPending(true)
    setError(null)
    try {
      await onAdd({
        kind,
        title,
        description,
        date: today,
        startTime: kind === 'event' && startTime ? startTime : null,
        endTime: kind === 'event' && endTime ? endTime : null
      })
    } catch (err: unknown) {
      setError(String(err))
    } finally {
      setPending(false)
    }
  }

  return (
    <form className="einpx-add" onSubmit={(e) => void submit(e)}>
      <div className="einpx-seg" role="tablist" aria-label="Rodzaj wpisu">
        <button type="button" className={kind === 'task' ? 'is-on' : ''} onClick={() => setKind('task')}>
          Zadanie
        </button>
        <button type="button" className={kind === 'event' ? 'is-on' : ''} onClick={() => setKind('event')}>
          Wydarzenie
        </button>
      </div>
      <input placeholder="Tytuł" value={title} maxLength={120} onChange={(e) => setTitle(e.target.value)} required autoFocus />
      {kind === 'event' && (
        <div className="einpx-add-row">
          <input type="time" value={startTime} onChange={(e) => setStartTime(e.target.value)} aria-label="Od" />
          <input type="time" value={endTime} onChange={(e) => setEndTime(e.target.value)} aria-label="Do" />
        </div>
      )}
      <input placeholder="Opis (opcjonalnie)" value={description} maxLength={2000} onChange={(e) => setDescription(e.target.value)} />
      {error && <p className="einpx-section-error">{error}</p>}
      <button className="einpx-btn primary" disabled={pending || !title.trim()}>
        {kind === 'event' ? 'Dodaj wydarzenie' : 'Dodaj zadanie'}
      </button>
    </form>
  )
}
