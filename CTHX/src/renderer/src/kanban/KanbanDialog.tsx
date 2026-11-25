import { useEffect, useState, type FormEvent } from 'react'
import { KANBAN_COLUMNS, type KanbanBoard, type KanbanStatus, type KanbanTask } from '@shared/kanban'
import type { Item } from '@shared/types'

/** Dzień RRRR-MM-DD jako „12.11.2026” */
function dayLabel(day: string): string {
  const [y, m, d] = day.split('-')
  return `${d}.${m}.${y}`
}

/** Okno nowego i edytowanego zadania kanbanu (ten sam styl co okno nawyków) */
export default function KanbanDialog({
  task,
  month,
  boards,
  defaultBoardId,
  onClose,
  onSaved
}: {
  task?: KanbanTask
  /** miesiąc, w którym powstaje nowe zadanie */
  month: string
  boards: KanbanBoard[]
  /** strona wybrana z góry (ta, na której jest użytkownik); dla edycji bierze się stronę zadania */
  defaultBoardId: number
  onClose: () => void
  onSaved: () => void
}): JSX.Element {
  const [title, setTitle] = useState(task?.title ?? '')
  const [description, setDescription] = useState(task?.description ?? '')
  const [status, setStatus] = useState<KanbanStatus>(task?.status ?? 'todo')
  const [boardId, setBoardId] = useState<number>(task?.boardId ?? defaultBoardId)
  // połączenie z zadaniem z kalendarza: przy edycji zostaje z karty, przy tworzeniu wybiera się z listy
  const [linkId, setLinkId] = useState<number | null>(task?.itemId ?? null)
  const [calendarTasks, setCalendarTasks] = useState<Item[]>([])
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  // tylko przy tworzeniu nowej karty można wybrać zadanie z kalendarza
  const canLink = !task
  useEffect(() => {
    if (!canLink) return
    window.api
      .itemsList()
      .then((items) =>
        setCalendarTasks(
          items
            .filter((item) => item.kind === 'task')
            .sort((a, b) => a.date.localeCompare(b.date) || a.title.localeCompare(b.title, 'pl'))
        )
      )
      .catch((err: unknown) => setError(String(err)))
  }, [canLink])

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  // połączone zadanie: tytuł i dzień są z kalendarza, opis wczytuje się z zadania (dalej można go zmienić)
  const linkedItem = linkId !== null ? calendarTasks.find((item) => item.id === linkId) ?? null : null
  const linked = linkId !== null
  const linkedTitle = linked ? (task ? task.title : linkedItem?.title ?? '') : ''
  const linkedDate = linked ? (task ? task.itemDate : linkedItem?.date ?? null) : null
  const effectiveTitle = linked ? linkedTitle : title

  const chooseLink = (value: string): void => {
    const id = value === '' ? null : Number(value)
    setLinkId(id)
    const item = id !== null ? calendarTasks.find((i) => i.id === id) : undefined
    if (item) setDescription(item.description)
  }

  const submit = (e: FormEvent): void => {
    e.preventDefault()
    setError(null)
    setBusy(true)
    window.api
      .kanbanSave({
        id: task?.id,
        month: task?.month ?? month,
        status,
        title: effectiveTitle,
        description,
        boardId,
        itemId: linkId
      })
      .then(onSaved)
      .catch((err: unknown) => setError(String(err)))
      .finally(() => setBusy(false))
  }

  const heading = task ? 'Edytuj zadanie' : 'Nowe zadanie'

  return (
    <div className="modal-overlay cthx-overlay" onClick={onClose}>
      <form className="cthx-dialog" role="dialog" aria-label={heading} onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <header className="cthx-dialog-head">
          <h2>{heading}</h2>
          <button type="button" className="cthx-close" onClick={onClose} aria-label="Zamknij">
            ×
          </button>
        </header>
        <div className="cthx-dialog-body">
          {canLink && (
            <label className="cthx-field">
              <span>Połącz z zadaniem</span>
              <select value={linkId ?? ''} onChange={(e) => chooseLink(e.target.value)}>
                <option value="">Bez połączenia</option>
                {calendarTasks.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.title} ({dayLabel(item.date)})
                  </option>
                ))}
              </select>
            </label>
          )}
          {linked ? (
            <>
              <div className="cthx-field">
                <span>Tytuł (z zadania)</span>
                <input value={linkedTitle} readOnly aria-readonly="true" />
              </div>
              <div className="cthx-field">
                <span>Data (z zadania)</span>
                <input value={linkedDate ? dayLabel(linkedDate) : 'brak daty'} readOnly aria-readonly="true" />
              </div>
            </>
          ) : (
            <label className="cthx-field">
              <span>Tytuł</span>
              <input value={title} onChange={(e) => setTitle(e.target.value)} maxLength={120} placeholder="Np. Przygotować prezentację" required autoFocus />
            </label>
          )}
          <label className="cthx-field">
            <span>Opis</span>
            <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={3} maxLength={2000} placeholder="Dodaj opis…" />
          </label>
          <label className="cthx-field">
            <span>Strona</span>
            <select value={boardId} onChange={(e) => setBoardId(Number(e.target.value))}>
              {boards.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.name}
                </option>
              ))}
            </select>
          </label>
          <label className="cthx-field">
            <span>Kolumna</span>
            <select value={status} onChange={(e) => setStatus(e.target.value as KanbanStatus)}>
              {KANBAN_COLUMNS.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
          {error && <div className="error">{error}</div>}
        </div>
        <footer className="cthx-dialog-foot">
          <button type="button" className="cthx-cancel" onClick={onClose}>
            Anuluj
          </button>
          <button type="submit" className="cthx-primary cthx-save" disabled={busy || !effectiveTitle.trim() || boards.length === 0}>
            Zapisz
          </button>
        </footer>
      </form>
    </div>
  )
}
