import { Fragment, useCallback, useEffect, useRef, useState, type FormEvent, type PointerEvent as ReactPointerEvent } from 'react'
import { KANBAN_COLUMNS, type KanbanBoard, type KanbanStatus, type KanbanTask } from '@shared/kanban'
import { dayKey, parseDay } from '@shared/recurrence'
import KanbanDialog from './KanbanDialog'
import { DeleteBoardDialog, NewBoardDialog } from './BoardDialogs'

const pad = (n: number): string => String(n).padStart(2, '0')

/** Ruch wskaźnikiem krótszy niż tyle pikseli to jeszcze nie przeciąganie (zostaje zwykłe kliknięcie) */
const DRAG_THRESHOLD = 5

/** Wybrana strona: konkretna (id) albo „Wszystkie” */
type ActiveBoard = number | 'all'

/** Stan trwającego przeciągania: karta, pozycja wskaźnika i czy próg już przekroczono */
interface DragState {
  task: KanbanTask
  startX: number
  startY: number
  x: number
  y: number
  active: boolean
  over: KanbanStatus | null
  /** karta, przed którą zostanie wstawiona przeciągana (null = na koniec kolumny) */
  before: number | null
}

/**
 * Kanban: strony (np. „Prezenty”, „Programowanie”) i na każdej trzy kolumny dla przeglądanego miesiąca.
 * „Wszystkie” pokazuje zadania ze wszystkich stron. Kartę przeciąga się myszką do innej kolumny.
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
  const [boards, setBoards] = useState<KanbanBoard[]>([])
  const [active, setActive] = useState<ActiveBoard>('all')
  const [dialog, setDialog] = useState<{ task?: KanbanTask } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [drag, setDrag] = useState<DragState | null>(null)
  const dragRef = useRef<DragState | null>(null)

  // dodawanie strony i usuwanie strony (drugie kliknięcie potwierdza, z liczbą zadań do skasowania)
  const [boardDialog, setBoardDialog] = useState(false)
  const [deleteDialog, setDeleteDialog] = useState<{ id: number; name: string; count: number } | null>(null)
  const [boardMenu, setBoardMenu] = useState(false)
  const boardMenuRef = useRef<HTMLDivElement>(null)

  // menu stron zamyka się po kliknięciu poza nim i po Escape
  useEffect(() => {
    if (!boardMenu) return
    const onDown = (e: MouseEvent): void => {
      if (!boardMenuRef.current?.contains(e.target as Node)) setBoardMenu(false)
    }
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') setBoardMenu(false)
    }
    document.addEventListener('mousedown', onDown)
    window.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [boardMenu])

  const reload = useCallback(async (): Promise<void> => {
    try {
      const [list, allBoards] = await Promise.all([window.api.kanbanList(month, currentMonth), window.api.kanbanBoards()])
      setTasks(list)
      setBoards(allBoards)
      // usunięta strona nie może zostać wybrana
      setActive((cur) => (cur !== 'all' && !allBoards.some((b) => b.id === cur) ? 'all' : cur))
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

  // przeniesienie (albo przestawienie w kolumnie) przed wskazaną kartą; zmianę kolumny i kolejność robi rdzeń
  const moveTo = (task: KanbanTask, status: KanbanStatus, before: number | null): void => {
    if (before === task.id) return
    run(() => window.api.kanbanMove(task.id, status, before))
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
      const colEl = el?.closest<HTMLElement>('[data-status]')
      const col = colEl?.dataset.status as KanbanStatus | undefined
      // wstawienie przed pierwszą kartą kolumny, której środek jest poniżej wskaźnika (pomijamy przeciąganą kartę)
      let before: number | null = null
      if (colEl) {
        for (const card of colEl.querySelectorAll<HTMLElement>('[data-card-id]')) {
          const id = Number(card.dataset.cardId)
          if (id === d.task.id) continue
          const box = card.getBoundingClientRect()
          if (e.clientY < box.top + box.height / 2) {
            before = id
            break
          }
        }
      }
      dragRef.current = { ...d, active: true, x: e.clientX, y: e.clientY, over: col ?? null, before }
      setDrag(dragRef.current)
    }
    const onUp = (): void => {
      const d = dragRef.current
      dragRef.current = null
      setDrag(null)
      if (d?.active && d.over) moveRef.current(d.task, d.over, d.before)
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
    dragRef.current = { task, startX: e.clientX, startY: e.clientY, x: e.clientX, y: e.clientY, active: false, over: null, before: null }
  }

  const askDeleteBoard = (id: number): void => {
    const board = boards.find((b) => b.id === id)
    if (!board) return
    setError(null)
    window.api
      .kanbanBoardTaskCount(id)
      .then((count) => setDeleteDialog({ id, name: board.name, count }))
      .catch((err: unknown) => setError(String(err)))
  }

  const deleteBoard = (id: number): void => {
    setDeleteDialog(null)
    run(() => window.api.kanbanBoardDelete(id).then(() => setActive('all')))
  }

  // wybór z menu: „Stwórz Stronę” otwiera okno nowej strony, pozostałe opcje przełączają widok
  const pickBoard = (value: 'all' | number | 'create'): void => {
    setBoardMenu(false)
    if (value === 'create') {
      setBoardDialog(true)
      return
    }
    setActive(value)
  }
  const shown = (tasks ?? []).filter((t) => active === 'all' || t.boardId === active)
  const activeBoard = typeof active === 'number' ? boards.find((b) => b.id === active) ?? null : null
  // zadanie nowe trafia na przeglądaną stronę, a w „Wszystkich” na pierwszą
  const defaultBoardId = activeBoard?.id ?? boards[0]?.id ?? 0
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
            <div className="cthx-seg kb-board-seg" ref={boardMenuRef}>
              <button
                className="kb-board-pick"
                onClick={() => setBoardMenu((open) => !open)}
                aria-haspopup="listbox"
                aria-expanded={boardMenu}
                aria-label="Strona kanbanu"
              >
                <span>{activeBoard ? activeBoard.name : 'Wszystkie'}</span>
                <span className="kb-board-caret" aria-hidden>
                  ▾
                </span>
              </button>
              {boardMenu && (
                <ul className="kb-board-menu" role="listbox" aria-label="Strony">
                  <li role="option" aria-selected={active === 'all'}>
                    <button className={active === 'all' ? 'is-active' : ''} onClick={() => pickBoard('all')}>
                      Wszystkie
                    </button>
                  </li>
                  {boards.map((b) => (
                    <li key={b.id} role="option" aria-selected={active === b.id}>
                      <button className={active === b.id ? 'is-active' : ''} onClick={() => pickBoard(b.id)}>
                        {b.name}
                      </button>
                    </li>
                  ))}
                  <li role="option" aria-selected={false}>
                    <button onClick={() => pickBoard('create')}>Stwórz Stronę</button>
                  </li>
                </ul>
              )}
            </div>
            {activeBoard && (
              <button className="kb-board-delete" onClick={() => askDeleteBoard(activeBoard.id)}>
                Usuń stronę
              </button>
            )}
          </div>
          <div className="cthx-actions">
            <button
              className="kb-plus"
              onClick={() => setDialog({})}
              disabled={boards.length === 0}
              aria-label="Dodaj zadanie"
              title={boards.length === 0 ? 'Najpierw dodaj stronę' : 'Dodaj zadanie'}
            >
              +
            </button>
          </div>
        </div>
      </div>

      {error && <div className="error cthx-error">{error}</div>}

      {tasks === null ? (
        <div className="fin-empty">Ładowanie…</div>
      ) : boards.length === 0 ? (
        <div className="fin-empty">Nie ma jeszcze żadnej strony. Dodaj pierwszą, np. „Prezenty” albo „Programowanie”.</div>
      ) : (
        <div className="kb-board">
          {KANBAN_COLUMNS.map((col) => {
            const inColumn = shown.filter((t) => t.status === col.id)
            return (
              <section key={col.id} data-status={col.id} className={`kb-col ${overColumn === col.id ? 'is-over' : ''}`} aria-label={col.label}>
                <header className="kb-col-head">
                  <span>{col.label}</span>
                  <span className="kb-count">{inColumn.length}</span>
                </header>
                {inColumn.length === 0 && <p className="kb-empty">Brak zadań</p>}
                {inColumn.map((task) => (
                  <Fragment key={task.id}>
                    {overColumn === col.id && drag?.before === task.id && <div className="kb-drop-marker" aria-hidden />}
                    <TaskCard
                    key={task.id}
                    task={task}
                    showBoard={active === 'all'}
                    dragging={drag?.active === true && drag.task.id === task.id}
                    onPointerDown={(e) => startDrag(task, e)}
                    onEdit={() => setDialog({ task })}
                    onDelete={() => run(() => window.api.kanbanDelete(task.id))}
                  />
                  </Fragment>
                ))}
                {overColumn === col.id && drag?.before === null && <div className="kb-drop-marker" aria-hidden />}
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

      {boardDialog && (
        <NewBoardDialog
          onClose={() => setBoardDialog(false)}
          onCreated={(id) => {
            setBoardDialog(false)
            setActive(id)
            void reload()
          }}
        />
      )}

      {deleteDialog && (
        <DeleteBoardDialog
          name={deleteDialog.name}
          count={deleteDialog.count}
          onCancel={() => setDeleteDialog(null)}
          onConfirm={() => deleteBoard(deleteDialog.id)}
        />
      )}

      {dialog && boards.length > 0 && (
        <KanbanDialog
          task={dialog.task}
          month={month}
          boards={boards}
          defaultBoardId={defaultBoardId}
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
  showBoard,
  dragging,
  onPointerDown,
  onEdit,
  onDelete
}: {
  task: KanbanTask
  /** w widoku „Wszystkie” karta pokazuje, z której jest strony */
  showBoard: boolean
  dragging: boolean
  onPointerDown: (e: ReactPointerEvent) => void
  onEdit: () => void
  onDelete: () => void
}): JSX.Element {
  const [armed, setArmed] = useState(false)
  return (
    <article
      data-card-id={task.id}
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
      {showBoard && <span className="kb-board-tag">{task.boardName}</span>}
      {task.description && <p className="kb-desc">{task.description}</p>}
    </article>
  )
}
