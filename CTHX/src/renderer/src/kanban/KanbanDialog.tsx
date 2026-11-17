import { useEffect, useState, type FormEvent } from 'react'
import { KANBAN_COLUMNS, type KanbanStatus, type KanbanTask } from '@shared/kanban'

/** Okno nowego i edytowanego zadania kanbanu (ten sam styl co okno nawyków) */
export default function KanbanDialog({
  task,
  month,
  onClose,
  onSaved
}: {
  task?: KanbanTask
  /** miesiąc, w którym powstaje nowe zadanie */
  month: string
  onClose: () => void
  onSaved: () => void
}): JSX.Element {
  const [title, setTitle] = useState(task?.title ?? '')
  const [description, setDescription] = useState(task?.description ?? '')
  const [status, setStatus] = useState<KanbanStatus>(task?.status ?? 'todo')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const submit = (e: FormEvent): void => {
    e.preventDefault()
    setError(null)
    setBusy(true)
    window.api
      .kanbanSave({ id: task?.id, month: task?.month ?? month, status, title, description })
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
          <label className="cthx-field">
            <span>Tytuł</span>
            <input value={title} onChange={(e) => setTitle(e.target.value)} maxLength={120} placeholder="Np. Przygotować prezentację" required autoFocus />
          </label>
          <label className="cthx-field">
            <span>Opis</span>
            <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={3} maxLength={2000} placeholder="Dodaj opis…" />
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
          <button type="submit" className="cthx-primary cthx-save" disabled={busy || !title.trim()}>
            Zapisz
          </button>
        </footer>
      </form>
    </div>
  )
}
