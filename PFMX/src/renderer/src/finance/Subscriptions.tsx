import { useCallback, useEffect, useState } from 'react'
import type { Recurring } from '@shared/types'
import { currentMonth, formatDate, formatMoney } from './format'
import { RecurringForm, RecurringRow } from './Recurring'

/** Podstrona subskrypcji: koszt miesięczny i roczny, następna płatność, zmiana ceny z banku. */
export default function Subscriptions(): JSX.Element {
  const [month] = useState(currentMonth)
  const [items, setItems] = useState<Recurring[]>([])
  const [adding, setAdding] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(async (): Promise<void> => {
    const all = await window.api.financeRecurring(month)
    setItems(all.filter((r) => r.kind === 'subscription' && !r.ended))
  }, [month])

  useEffect(() => {
    reload().catch((e: unknown) => setError(String(e)))
  }, [reload])

  const monthly = items.reduce((sum, r) => sum + r.amount, 0)
  const nextDue = items.map((r) => r.nextDueOn).filter((d): d is string => d !== null).sort()[0]

  // przyjęcie nowej kwoty z banku: zapisujemy ją jako kwotę płatności, rat nie ruszamy
  const acceptPrice = (r: Recurring): void => {
    if (r.observedAmount === null) return
    window.api
      .financeSaveRecurring({ id: r.id, name: r.name, kind: r.kind, amount: r.observedAmount, installmentsLeft: r.installmentsLeft })
      .then(() => reload())
      .catch((e: unknown) => setError(String(e)))
  }

  return (
    <div className="subs">
      <div className="subs-metrics">
        <div className="subs-metric subs-metric-main">
          <div className="subs-metric-label">Miesięcznie</div>
          <div className="subs-metric-value">{formatMoney(monthly)}</div>
        </div>
        <div className="subs-metric">
          <div className="subs-metric-label">Rocznie</div>
          <div className="subs-metric-value">{formatMoney(monthly * 12)}</div>
        </div>
        <div className="subs-metric">
          <div className="subs-metric-label">Następna płatność</div>
          <div className="subs-metric-value">{nextDue ? formatDate(nextDue) : '—'}</div>
        </div>
        <div className="subs-metric">
          <div className="subs-metric-label">Subskrypcje</div>
          <div className="subs-metric-value">{items.length}</div>
        </div>
      </div>

      <section className="subs-list">
        <header className="subs-list-head">
          <h3>Subskrypcje</h3>
          {!adding && (
            <button className="fin-link" onClick={() => setAdding(true)}>
              Dodaj
            </button>
          )}
        </header>
        {adding && (
          <RecurringForm
            defaultKind="subscription"
            onSaved={() => {
              setAdding(false)
              void reload()
            }}
            onCancel={() => setAdding(false)}
          />
        )}
        {items.length === 0 && !adding && (
          <div className="fin-empty">Brak subskrypcji. Dodaj je ręcznie albo poczekaj na wykrycie z historii.</div>
        )}
        {items.map((item) => (
          <RecurringRow
            key={item.id}
            item={item}
            showPaid={false}
            month={month}
            onChanged={() => void reload()}
            onError={setError}
            leading={<span className="subs-logo" aria-hidden>{item.name.trim().charAt(0).toUpperCase() || '•'}</span>}
            tag={<span className="subs-cycle">miesięcznie</span>}
            notice={
              item.observedAmount !== null ? (
                <div className="fin-price-change">
                  Zmiana ceny: <strong>{formatMoney(item.amount)} → {formatMoney(item.observedAmount)}</strong>
                  <button className="fin-link" onClick={() => acceptPrice(item)}>
                    Przyjmij nową kwotę
                  </button>
                </div>
              ) : undefined
            }
          />
        ))}
      </section>

      {error && <div className="error">{error}</div>}
    </div>
  )
}
