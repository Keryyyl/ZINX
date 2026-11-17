import type { Item, ItemKind } from '@shared/types'
import { addDays, dayKey } from '@shared/recurrence'
import { dayProgress, dayTitle, isDone, itemsOn } from '../items'
import DayTimeline from '../calendar/DayTimeline'
import ItemRow from './ItemRow'

/** Dzień w dwóch kolumnach: zadania z checkboxami i harmonogram wydarzeń z osią czasu */
export default function TodoScreen({
  items,
  doneSet,
  day,
  onDay,
  onToggle,
  onEdit,
  onDelete,
  onAdd
}: {
  items: Item[]
  doneSet: Set<string>
  day: string
  onDay: (day: string) => void
  onToggle: (item: Item, on: boolean, day: string) => void
  onEdit: (item: Item) => void
  onDelete: (item: Item) => void
  onAdd: (kind: ItemKind) => void
}): JSX.Element {
  const list = itemsOn(items, day)
  const events = list.filter((item) => item.kind === 'event')
  const tasks = list.filter((item) => item.kind === 'task')
  const isToday = day === dayKey(new Date())
  const { done, total } = dayProgress(list, doneSet, day)
  const percent = total > 0 ? Math.round((done / total) * 100) : 0
  const tasksDone = tasks.filter((item) => isDone(doneSet, item, day)).length
  const eventsDone = events.filter((item) => isDone(doneSet, item, day)).length

  return (
    <div className="cthx-todo">
      <div className="list-header">
        <div className="cthx-toolbar">
          <div className="cthx-seg" role="group" aria-label="Wybór dnia">
            <button className="cthx-seg-btn" onClick={() => onDay(addDays(day, -1))} aria-label="Dzień wcześniej">
              ‹
            </button>
            <h2 className="cthx-seg-title">{dayTitle(day)}</h2>
            <button className="cthx-seg-btn" onClick={() => onDay(addDays(day, 1))} aria-label="Dzień później">
              ›
            </button>
            {!isToday && (
              <button className="cthx-seg-btn cthx-seg-today" onClick={() => onDay(dayKey(new Date()))}>
                Dziś
              </button>
            )}
          </div>
          <div className="cthx-actions">
            <button className="cthx-ghost" onClick={() => onAdd('event')}>
              Dodaj wydarzenie
            </button>
            <button className="cthx-primary" onClick={() => onAdd('task')}>
              Dodaj zadanie do wykonania
            </button>
          </div>
        </div>
        <div className="cthx-progress" aria-label="Postęp dnia">
          <div className="cthx-progress-meta">
            <span>Postęp dnia</span>
            <span>{total === 0 ? 'Brak wpisów' : `${done} z ${total} zrobione`}</span>
          </div>
          <div className="cthx-progress-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
            <span style={{ width: `${percent}%` }} />
          </div>
        </div>
      </div>

      <div className="cthx-split">
        <section className="cthx-panel cthx-panel-tasks" aria-label="Zadania">
          <header className="cthx-panel-head">
            <h3>Zadania</h3>
            <span className="cthx-count">
              <b>{tasksDone}</b> / {tasks.length}
            </span>
          </header>
          <div className="cthx-panel-body">
            {tasks.length === 0 ? (
              <div className="cthx-empty-line">Brak zadań na ten dzień.</div>
            ) : (
              tasks.map((item) => (
                <ItemRow key={`${item.id}-${day}`} item={item} day={day} doneSet={doneSet} onToggle={onToggle} onEdit={onEdit} onDelete={onDelete} />
              ))
            )}
          </div>
        </section>

        <section className="cthx-panel cthx-panel-plan" aria-label="Wydarzenia">
          <header className="cthx-panel-head">
            <h3>Wydarzenia</h3>
            <span className="cthx-count">
              <b>{eventsDone}</b> / {events.length}
            </span>
          </header>
          <div className="cthx-panel-body cthx-panel-body-plan">
            <DayTimeline items={events} doneSet={doneSet} day={day} onToggle={onToggle} onEdit={onEdit} />
          </div>
        </section>
      </div>
    </div>
  )
}
