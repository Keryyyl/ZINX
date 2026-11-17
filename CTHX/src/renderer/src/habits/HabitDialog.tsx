import { useEffect, useState } from 'react'
import type { Habit } from '@shared/habits'
import { dayKey } from '@shared/recurrence'

/** Okno nowego i edytowanego nawyku, w tym samym stylu co okno zadań */
export default function HabitDialog({ habit, onClose, onSaved }: { habit?: Habit; onClose: () => void; onSaved: (id: number) => void }): JSX.Element {
  const [name, setName] = useState(habit?.name ?? '')
  const [description, setDescription] = useState(habit?.description ?? '')
  const [goal, setGoal] = useState(String(habit?.goal ?? 20))
  const [startDate, setStartDate] = useState(habit?.startDate ?? dayKey(new Date()))
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const submit = (e: React.FormEvent): void => {
    e.preventDefault()
    setError(null)
    setBusy(true)
    window.api
      .habitSave({ id: habit?.id, name, description, goal: Number(goal), startDate })
      .then(onSaved)
      .catch((err: unknown) => setError(String(err)))
      .finally(() => setBusy(false))
  }

  const heading = habit ? 'Edytuj nawyk' : 'Nowy nawyk'

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
            <span>Nazwa</span>
            <input value={name} onChange={(e) => setName(e.target.value)} maxLength={80} placeholder="Np. Bieganie" required autoFocus />
          </label>
          <label className="cthx-field">
            <span>Opis</span>
            <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={2} maxLength={500} placeholder="Dodaj opis…" />
          </label>
          <div className="cthx-fields">
            <label className="cthx-field">
              <span>Cel w miesiącu (razy)</span>
              <input type="number" min={1} max={31} value={goal} onChange={(e) => setGoal(e.target.value)} required />
            </label>
            <label className="cthx-field">
              <span>Start śledzenia</span>
              <input type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} required />
            </label>
          </div>
          {error && <div className="error">{error}</div>}
        </div>
        <footer className="cthx-dialog-foot">
          <button type="button" className="cthx-cancel" onClick={onClose}>
            Anuluj
          </button>
          <button type="submit" className="cthx-primary cthx-save" disabled={busy || !name.trim()}>
            Zapisz
          </button>
        </footer>
      </form>
    </div>
  )
}
