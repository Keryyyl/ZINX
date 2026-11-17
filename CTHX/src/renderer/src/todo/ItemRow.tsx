import { useState } from 'react'
import type { Item, Recurrence } from '@shared/types'
import { durationLabel, isDone, timeLabel } from '../items'

export const REPEAT_LABEL: Record<Recurrence, string> = {
  none: 'Jednorazowe',
  daily: 'Codziennie',
  weekly: 'Co tydzień',
  monthly: 'Co miesiąc',
  yearly: 'Co rok'
}

/** Jeden wiersz listy: checkbox, nazwa, opis, godzina, Edytuj, Usuń (usunięcie wymaga drugiego kliknięcia) */
export default function ItemRow({
  item,
  day,
  doneSet,
  onToggle,
  onEdit,
  onDelete
}: {
  item: Item
  day: string
  doneSet: Set<string>
  onToggle: (item: Item, on: boolean, day: string) => void
  onEdit?: (item: Item) => void
  onDelete?: (item: Item) => void
}): JSX.Element {
  const checked = isDone(doneSet, item, day)
  const time = timeLabel(item)
  const duration = durationLabel(item)
  const [armed, setArmed] = useState(false)

  return (
    <div className={`cthx-item ${checked ? 'is-done' : ''}`}>
      <input
        type="checkbox"
        className="cthx-check"
        checked={checked}
        onChange={(e) => onToggle(item, e.target.checked, day)}
        aria-label={`Zrobione: ${item.title}`}
      />
      <div className="cthx-item-main">
        <div className="cthx-item-title">
          <span>{item.title}</span>
          <span className={`cthx-tag ${item.kind}`}>{item.kind === 'event' ? 'Wydarzenie' : 'Zadanie'}</span>
          {item.recurrence !== 'none' && <span className="cthx-tag">{REPEAT_LABEL[item.recurrence]}</span>}
        </div>
        {item.description && <div className="cthx-item-desc">{item.description}</div>}
      </div>
      {time && (
        <div className="cthx-item-time" title={duration ? `Czas trwania: ${duration}` : undefined}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" aria-hidden>
            <circle cx="12" cy="12" r="8.5" />
            <path d="M12 7.5V12l3 2" />
          </svg>
          <span>{time}</span>
          {duration && <span className="cthx-dur">{duration}</span>}
        </div>
      )}
      <div className="cthx-item-actions">
        {onEdit && (
          <button className="cthx-link" onClick={() => onEdit(item)}>
            Edytuj
          </button>
        )}
        {onDelete && (
          <button
            className={`cthx-link ${armed ? 'danger' : ''}`}
            onClick={() => (armed ? onDelete(item) : setArmed(true))}
            onBlur={() => setArmed(false)}
          >
            {armed ? 'Na pewno?' : 'Usuń'}
          </button>
        )}
      </div>
    </div>
  )
}
