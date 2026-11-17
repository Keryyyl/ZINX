import { useEffect, useMemo, useRef, useState } from 'react'
import type { Item } from '@shared/types'
import { dayKey } from '@shared/recurrence'
import { layoutDay, type Placed } from '@shared/timeline'
import { dayTitle, itemsOn, isDone, timeLabel } from '../items'

/** Wysokość jednej godziny na osi, w pikselach */
export const HOUR_PX = 56
const HOURS = Array.from({ length: 24 }, (_, h) => h)
const pad = (n: number): string => String(n).padStart(2, '0')

/** Pojedynczy checkbox z nazwą wpisu nad osią czasu */
function Chip({
  item,
  day,
  doneSet,
  onToggle,
  onEdit,
  withTime
}: {
  item: Item
  day: string
  doneSet: Set<string>
  onToggle: (item: Item, on: boolean, day: string) => void
  onEdit: (item: Item) => void
  withTime?: boolean
}): JSX.Element {
  const checked = isDone(doneSet, item, day)
  return (
    <div className={`cthx-plan-chip ${checked ? 'is-done' : ''}`}>
      <input
        type="checkbox"
        className="cthx-check"
        checked={checked}
        onChange={(e) => onToggle(item, e.target.checked, day)}
        aria-label={`Zrobione: ${item.title}`}
      />
      <button className="cthx-plan-title" onClick={() => onEdit(item)}>
        {item.title}
      </button>
      {withTime && timeLabel(item) && <span className="cthx-plan-time">{timeLabel(item)}</span>}
    </div>
  )
}

/** Plan dnia na 24 godziny: zadania jako pojedyncze wiersze z checkboxem, wydarzenia jako bloki czasu */
export default function DayTimeline({
  items,
  doneSet,
  day,
  onToggle,
  onEdit
}: {
  items: Item[]
  doneSet: Set<string>
  day: string
  onToggle: (item: Item, on: boolean, day: string) => void
  onEdit: (item: Item) => void
}): JSX.Element {
  const dayItems = useMemo(() => itemsOn(items, day), [items, day])
  const layout = useMemo(() => layoutDay(dayItems), [dayItems])
  // checkboxy wydarzeń dnia nad osią (żeby dało się je odznaczyć); zadania bez godziny osobno
  const eventChips = dayItems.filter((item) => item.kind === 'event')
  const untimedTasks = layout.untimed.filter((item) => item.kind === 'task')
  const scroller = useRef<HTMLDivElement>(null)
  const isToday = day === dayKey(new Date())
  const now = useNowMinutes()

  // przy zmianie dnia pokazujemy poranek, a nie północ
  useEffect(() => {
    scroller.current?.scrollTo({ top: 6 * HOUR_PX })
  }, [day])

  return (
    <aside className="cthx-plan" aria-label="Plan dnia">
      <div className="cthx-plan-head">{dayTitle(day)}</div>
      {(eventChips.length > 0 || untimedTasks.length > 0) && (
        <div className="cthx-plan-strip">
          {eventChips.length > 0 && (
            <div className="cthx-plan-group">
              <span className="cthx-plan-label">Wydarzenia</span>
              {eventChips.map((item) => (
                <Chip key={item.id} item={item} day={day} doneSet={doneSet} onToggle={onToggle} onEdit={onEdit} withTime />
              ))}
            </div>
          )}
          {untimedTasks.length > 0 && (
            <div className="cthx-plan-group">
              <span className="cthx-plan-label">Bez godziny</span>
              {untimedTasks.map((item) => (
                <Chip key={item.id} item={item} day={day} doneSet={doneSet} onToggle={onToggle} onEdit={onEdit} />
              ))}
            </div>
          )}
        </div>
      )}

      <div className="cthx-plan-scroll" ref={scroller}>
        <div className="cthx-plan-canvas" style={{ height: 24 * HOUR_PX }}>
          {HOURS.map((h) => (
            <div key={h} className="cthx-hour" style={{ top: h * HOUR_PX }}>
              <span>{pad(h)}:00</span>
            </div>
          ))}

          {layout.timed.map((p) => (
            <PlacedItem key={p.item.id} placed={p} day={day} doneSet={doneSet} onToggle={onToggle} onEdit={onEdit} />
          ))}

          {isToday && <div className="cthx-now" style={{ top: (now / 60) * HOUR_PX }} aria-hidden />}
        </div>
      </div>
    </aside>
  )
}

function PlacedItem({
  placed,
  day,
  doneSet,
  onToggle,
  onEdit
}: {
  placed: Placed
  day: string
  doneSet: Set<string>
  onToggle: (item: Item, on: boolean, day: string) => void
  onEdit: (item: Item) => void
}): JSX.Element {
  const { item, top, duration, column, columns } = placed
  const geometry = {
    top: (top / 60) * HOUR_PX,
    left: `calc(${(column / columns) * 100}% + 4px)`,
    width: `calc(${100 / columns}% - 8px)`
  }
  const checked = isDone(doneSet, item, day)

  if (item.kind === 'task') {
    return (
      <div className={`cthx-plan-task ${checked ? 'is-done' : ''}`} style={geometry}>
        <input
          type="checkbox"
          className="cthx-check"
          checked={checked}
          onChange={(e) => onToggle(item, e.target.checked, day)}
          aria-label={`Zrobione: ${item.title}`}
        />
        <button className="cthx-plan-title" onClick={() => onEdit(item)}>
          {item.title}
        </button>
        <span className="cthx-plan-time">{timeLabel(item)}</span>
      </div>
    )
  }

  return (
    <button className="cthx-plan-event" style={{ ...geometry, height: Math.max(22, (duration / 60) * HOUR_PX - 2) }} onClick={() => onEdit(item)}>
      <span className="cthx-plan-title">{item.title}</span>
      <span className="cthx-plan-time">{timeLabel(item)}</span>
      {item.description && <span className="cthx-plan-desc">{item.description}</span>}
    </button>
  )
}

/** Minuty od północy, odświeżane co minutę (linia „teraz”) */
function useNowMinutes(): number {
  const [now, setNow] = useState(() => minutesNow())
  useEffect(() => {
    const id = window.setInterval(() => setNow(minutesNow()), 60_000)
    return () => window.clearInterval(id)
  }, [])
  return now
}

function minutesNow(): number {
  const d = new Date()
  return d.getHours() * 60 + d.getMinutes()
}
