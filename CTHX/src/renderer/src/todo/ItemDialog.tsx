import { useEffect, useState } from 'react'
import type { Item, ItemKind, Recurrence } from '@shared/types'
import { REPEAT_LABEL } from './ItemRow'

const RECURRENCES: Recurrence[] = ['none', 'daily', 'weekly', 'monthly', 'yearly']

/** Okno dodawania i edycji: zadanie (opcjonalna godzina) albo wydarzenie (od–do lub cały dzień) */
export default function ItemDialog({
  kind,
  item,
  defaultDay,
  onClose,
  onSaved
}: {
  kind: ItemKind
  item?: Item
  defaultDay: string
  onClose: () => void
  onSaved: () => void
}): JSX.Element {
  const isEvent = kind === 'event'
  const [title, setTitle] = useState(item?.title ?? '')
  const [description, setDescription] = useState(item?.description ?? '')
  const [date, setDate] = useState(item?.date ?? defaultDay)
  // wydarzenie bez godziny rozpoczęcia to wydarzenie całodniowe
  const [allDay, setAllDay] = useState(isEvent && !!item && !item.startTime)
  const [startTime, setStartTime] = useState(item?.startTime ?? (isEvent ? '09:00' : ''))
  const [endTime, setEndTime] = useState(item?.endTime ?? (isEvent ? '10:00' : ''))
  const [recurrence, setRecurrence] = useState<Recurrence>(item?.recurrence ?? 'none')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const heading = isEvent ? (item ? 'Edytuj wydarzenie' : 'Nowe wydarzenie') : item ? 'Edytuj zadanie' : 'Nowe zadanie do wykonania'
  const timed = isEvent && !allDay

  const submit = (e: React.FormEvent): void => {
    e.preventDefault()
    setError(null)
    setBusy(true)
    const start = isEvent && allDay ? null : startTime || null
    window.api
      .itemSave({
        id: item?.id,
        kind,
        title,
        description,
        date,
        startTime: start,
        endTime: isEvent && start ? endTime || null : null,
        recurrence
      })
      .then(onSaved)
      .catch((err: unknown) => setError(String(err)))
      .finally(() => setBusy(false))
  }

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
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              maxLength={120}
              placeholder={isEvent ? 'Nazwa wydarzenia' : 'Co masz do zrobienia?'}
              required
              autoFocus
            />
          </label>
          <label className="cthx-field">
            <span>Opis</span>
            <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={3} maxLength={2000} placeholder="Dodaj opis…" />
          </label>

          {isEvent && (
            <div className="cthx-allday">
              <span>Cały dzień</span>
              <button
                type="button"
                role="switch"
                aria-checked={allDay}
                aria-label="Cały dzień"
                className={`cthx-switch ${allDay ? 'is-on' : ''}`}
                onClick={() => setAllDay((v) => !v)}
              >
                <span className="cthx-switch-knob" />
              </button>
            </div>
          )}

          <div className={`cthx-fields ${timed ? 'with-times' : isEvent ? 'single' : ''}`}>
            <label className="cthx-field">
              <span>{recurrence === 'none' ? 'Data' : 'Pierwszy dzień'}</span>
              <input type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
            </label>
            {(!isEvent || timed) && (
              <label className="cthx-field">
                <span>{isEvent ? 'Od' : 'Godzina'}</span>
                <input type="time" value={startTime} onChange={(e) => setStartTime(e.target.value)} required={isEvent} />
              </label>
            )}
            {timed && (
              <label className="cthx-field">
                <span>Do</span>
                <input type="time" value={endTime} onChange={(e) => setEndTime(e.target.value)} required />
              </label>
            )}
          </div>

          <label className="cthx-field">
            <span>Powtarzanie</span>
            <select value={recurrence} onChange={(e) => setRecurrence(e.target.value as Recurrence)}>
              {RECURRENCES.map((r) => (
                <option key={r} value={r}>
                  {REPEAT_LABEL[r]}
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
