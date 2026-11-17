import type { Alert } from './alertRules'

/** Małe powiadomienia w rogu; zamknięte nie wracają (zapis w tej przeglądarce). */
export function Alerts({ items, onDismiss }: { items: Alert[]; onDismiss: (key: string) => void }): JSX.Element | null {
  if (items.length === 0) return null
  return (
    <div className="fin-alerts" role="status" aria-live="polite">
      {items.slice(0, 3).map((a) => (
        <div className="fin-alert-card" key={a.key}>
          <div>
            <h4>{a.title}</h4>
            <p>{a.text}</p>
          </div>
          <button className="fin-alert-close" onClick={() => onDismiss(a.key)} aria-label="Zamknij powiadomienie">
            ×
          </button>
        </div>
      ))}
      {items.length > 3 && <div className="fin-alerts-more">i jeszcze {items.length - 3}</div>}
    </div>
  )
}
