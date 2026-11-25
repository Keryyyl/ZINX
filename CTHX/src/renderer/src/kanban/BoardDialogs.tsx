import { useEffect, useState, type FormEvent } from 'react'

/** Okno dodawania strony kanbanu (ten sam wygląd co okno nowego zadania) */
export function NewBoardDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (id: number) => void }): JSX.Element {
  const [name, setName] = useState('')
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
      .kanbanBoardCreate(name.trim())
      .then(onCreated)
      .catch((err: unknown) => setError(String(err)))
      .finally(() => setBusy(false))
  }

  return (
    <div className="modal-overlay cthx-overlay" onClick={onClose}>
      <form className="cthx-dialog" role="dialog" aria-label="Nowa strona" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <header className="cthx-dialog-head">
          <h2>Nowa strona</h2>
          <button type="button" className="cthx-close" onClick={onClose} aria-label="Zamknij">
            ×
          </button>
        </header>
        <div className="cthx-dialog-body">
          <label className="cthx-field">
            <span>Tytuł</span>
            <input value={name} onChange={(e) => setName(e.target.value)} maxLength={40} placeholder="Np. Prezenty" required autoFocus />
          </label>
          {error && <div className="error">{error}</div>}
        </div>
        <footer className="cthx-dialog-foot">
          <button type="button" className="cthx-cancel" onClick={onClose}>
            Anuluj
          </button>
          <button type="submit" className="cthx-primary cthx-save" disabled={busy || !name.trim()}>
            Dodaj
          </button>
        </footer>
      </form>
    </div>
  )
}

/** Potwierdzenie usunięcia strony: bez tytułu, sama informacja i dwa przyciski */
export function DeleteBoardDialog({
  name,
  count,
  onCancel,
  onConfirm
}: {
  name: string
  count: number
  onCancel: () => void
  onConfirm: () => void
}): JSX.Element {
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onCancel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onCancel])

  return (
    <div className="modal-overlay cthx-overlay" onClick={onCancel}>
      <div className="cthx-dialog kb-confirm" role="alertdialog" aria-label="Usuń stronę" onClick={(e) => e.stopPropagation()}>
        <div className="cthx-dialog-body">
          <p className="kb-confirm-text">
            Usunąć stronę „{name}”{count > 0 ? ` razem z ${count} zadaniami` : ''}?
          </p>
        </div>
        <footer className="cthx-dialog-foot">
          <button type="button" className="cthx-cancel" onClick={onCancel}>
            Nie
          </button>
          <button type="button" className="kb-confirm-yes" onClick={onConfirm} autoFocus>
            Tak, usuń
          </button>
        </footer>
      </div>
    </div>
  )
}
