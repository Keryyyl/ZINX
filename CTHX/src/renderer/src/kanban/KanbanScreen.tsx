import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { KANBAN_COLUMNS, type KanbanStatus, type KanbanTask } from '@shared/kanban'
import { dayKey, parseDay } from '@shared/recurrence'
import KanbanDialog from './KanbanDialog'

const pad = (n: number): string => String(n).padStart(2, '0')

/** Ruch wskaźnikiem krótszy niż tyle pikseli to jeszcze nie przeciąganie (zostaje zwykłe kliknięcie) */
const DRAG_THRESHOLD = 5

/** Stan trwającego przeciągania: karta, pozycja wskaźnika i czy próg już przekroczono */
interface DragState {
  task: KanbanTask
  startX: number
  startY: number
  x: number
  y: number
  active: boolean
  over: KanbanStatus | null
}

/**
 * Kanban: trzy kolumny dla przeglądanego miesiąca. Kartę przeciąga się myszką do innej kolumny.
 * Zadania „Do zrobienia” z poprzednich miesięcy przechodzą do bieżącego.
 */
export default function KanbanScreen(): JSX.Element {
  const now = parseDay(dayKey(new Date()))
  const currentMonth = `${now.getFullYear()}-${pad(now.getMonth() + 1)}`
  const [view, setView] = useState({ year: now.getFullYear(), month: now.getMonth() })
  const month = `${view.year}-${pad(view.month + 1)}`
  const monthName = new Date(view.year, view.month, 1).toLocaleDateString('pl-PL', { month: 'long', year: 'numeric' })
  const isCurrentMonth = month === currentMonth
  const shiftMonth = (delta: number): void => {
    const d = new Date(view.year, view.month + delta, 1)
    setView({ year: d.getFullYear(), month: d.getMonth() })
  }

  const [tasks, setTasks] = useState<KanbanTask[] | null>(null)
  const [dialog, setDialog] = useState<{ task?: KanbanTask } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [drag, setDrag] = useState<DragState | null>(null)
  const dragRef = useRef<DragState | null>(null)

  const reload = useCallback(async (): Promise<void> => {
    try {
      setTasks(await window.api.kanbanList(month, currentMonth))
      setError(null)
    } catch (e: unknown) {
      setError(String(e))
    }
  }, [month, currentMonth])

  useEffect(() => {
    void reload()
  }, [reload])

  const run = (fn: () => Promise<unknown>): void => {
    setError(null)
    fn()
      .then(reload)
      .catch((e: unknown) => setError(String(e)))
  }

  const moveTo = (task: KanbanTask, status: KanbanStatus): void => {
    if (task.status === status) return
    run(() => window.api.kanbanSave({ id: task.id, month: task.month, status, title: task.title, description: task.description }))
  }
  // aktualna wersja funkcji dla obsługi zdarzeń wskaźnika (ta rejestrowana raz nie widzi nowych stanów)
  const moveRef = useRef(moveTo)
  moveRef.current = moveTo

  /* Przeciąganie pointerem: działa niezależnie od natywnego drag and drop, który okno Tauri ogranicza */
  useEffect(() => {
    const onMove = (e: PointerEvent): void => {
      const d = dragRef.current
      if (!d) return
      if (!d.active && Math.hypot(e.clientX - d.startX, e.clientY - d.startY) < DRAG_THRESHOLD) return
      const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null
      const col = el?.closest<HTMLElement>('[data-status]')?.dataset.status as KanbanStatus | undefined
      dragRef.current = { ...d, active: true, x: e.clientX, y: e.clientY, over: col ?? null }
      setDrag(dragRef.current)
    }
    const onUp = (): void => {
      const d = dragRef.current
      dragRef.current = null
      setDrag(null)
      if (d?.active && d.over) moveRef.current(d.task, d.over)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onUp)
    return () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onUp)
    }
  }, [])

  // podczas przeciągania kursor i zaznaczanie są ustalone na całym oknie
  useEffect(() => {
    document.body.classList.toggle('kb-dragging', drag?.active === true)
  }, [drag?.active])

  const startDrag = (task: KanbanTask, e: ReactPointerEvent): void => {
    // przyciski na karcie (edycja, usuwanie) działają jak zwykle
    if (e.button !== 0 || (e.target as HTMLElement).closest('button')) return
    dragRef.current = { task, startX: e.clientX, startY: e.clientY, x: e.clientX, y: e.clientY, active: false, over: null }
  }

  const overColumn = drag?.active ? drag.over : null

  return (
    <div className="cthx-todo kb">
      <div className="list-header">
        <div className="cthx-toolbar">
          <div className="hb-title">
            <h2>Kanban</h2>
            <div className="cthx-seg" role="group" aria-label="Wybór miesiąca">
              <button className="cthx-seg-btn" onClick={() => shiftMonth(-1)} aria-label="Poprzedni miesiąc">
                ‹
              </button>
              <span className="cthx-seg-title">{monthName}</span>
              <button className="cthx-seg-btn" onClick={() => shiftMonth(1)} aria-label="Następny miesiąc">
                ›
              </button>
              {!isCurrentMonth && (
                <button className="cthx-seg-btn cthx-seg-today" onClick={() => setView({ year: now.getFullYear(), month: now.getMonth() })}>
                  Dziś
                </button>
              )}
            </div>
          </div>
          <div className="cthx-actions">
            <button className="kb-plus" onClick={() => setDialog({})} aria-label="Dodaj zadanie" title="Dodaj zadanie">
              +
            </button>
          </div>
        </div>
      </div>

      {error && <div className="error cthx-error">{error}</div>}

      {tasks === null ? (
        <div className="fin-empty">Ładowanie…</div>
      ) : (
        <div className="kb-board">
          {KANBAN_COLUMNS.map((col) => {
            const inColumn = tasks.filter((t) => t.status === col.id)
            return (
              <section key={col.id} data-status={col.id} className={`kb-col ${overColumn === col.id ? 'is-over' : ''}`} aria-label={col.label}>
                <header className="kb-col-head">
                  <span>{col.label}</span>
                  <span className="kb-count">{inColumn.length}</span>
                </header>
                {inColumn.length === 0 && <p className="kb-empty">Brak zadań</p>}
                {inColumn.map((task) => (
                  <TaskCard
                    key={task.id}
                    task={task}
                    dragging={drag?.active === true && drag.task.id === task.id}
                    onPointerDown={(e) => startDrag(task, e)}
                    onEdit={() => setDialog({ task })}
                    onDelete={() => run(() => window.api.kanbanDelete(task.id))}
                  />
                ))}
              </section>
            )
          })}
        </div>
      )}

      {drag?.active && (
        <div className="kb-ghost" style={{ left: drag.x + 14, top: drag.y + 10 }} aria-hidden>
          {drag.task.title}
        </div>
      )}

      {dialog && (
        <KanbanDialog
          task={dialog.task}
          month={month}
          onClose={() => setDialog(null)}
          onSaved={() => {
            setDialog(null)
            void reload()
          }}
        />
      )}
    </div>
  )
}

/** Karta zadania: tytuł i opis, zębatka do edycji, × do usunięcia (drugie kliknięcie potwierdza) */
function TaskCard({
  task,
  dragging,
  onPointerDown,
  onEdit,
  onDelete
}: {
  task: KanbanTask
  dragging: boolean
  onPointerDown: (e: ReactPointerEvent) => void
  onEdit: () => void
  onDelete: () => void
}): JSX.Element {
  const [armed, setArmed] = useState(false)
  return (
    <article
      className={`kb-card ${dragging ? 'is-dragging' : ''}`}
      onPointerDown={onPointerDown}
      // przytrzymanie karty nie zaznacza jej tekstu (zaznaczanie zaczyna się od mousedown)
      onMouseDown={(e) => {
        if (!(e.target as HTMLElement).closest('button')) e.preventDefault()
      }}
      onDragStart={(e) => e.preventDefault()}
      onPointerLeave={() => setArmed(false)}
    >
      <div className="kb-card-head">
        <h3>{task.title}</h3>
        <div className="kb-card-actions">
          <button className="kb-icon" onClick={onEdit} aria-label={`Edytuj: ${task.title}`} title="Edytuj">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden>
              <circle cx="12" cy="12" r="3" />
              <circle cx="12" cy="12" r="8" strokeDasharray="2.4 2.4" strokeWidth="3.4" />
            </svg>
          </button>
          <button
            className={`kb-icon kb-x ${armed ? 'is-armed' : ''}`}
            onClick={() => (armed ? onDelete() : setArmed(true))}
            onBlur={() => setArmed(false)}
            aria-label={armed ? `Na pewno usunąć: ${task.title}` : `Usuń: ${task.title}`}
            title={armed ? 'Kliknij ponownie, aby usunąć' : 'Usuń'}
          >
            {armed ? '?' : '×'}
          </button>
        </div>
      </div>
      {task.description && <p className="kb-desc">{task.description}</p>}
    </article>
  )
}
