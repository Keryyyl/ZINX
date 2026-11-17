import { useCallback, useEffect, useState } from 'react'
import type { BudgetEnvelope, BudgetView, IncomeSummary, Recurring } from '@shared/types'
import { currentMonth, formatMoney, formatPercent, monthLabel, moneyInput, parseMoney, shiftMonth } from './format'
import { RecurringForm, RecurringRow } from './Recurring'

/** Ekran budżetu: pasek miesiąca, poduszka, koperty z planem i rolowaniem, stałe płatności do odhaczania. */
export default function Budget({ onOpenSubscriptions }: { onOpenSubscriptions: () => void }): JSX.Element {
  const [month, setMonth] = useState(currentMonth)
  const [view, setView] = useState<BudgetView | null>(null)
  const [fixed, setFixed] = useState<Recurring[]>([])
  const [subscriptionsTotal, setSubscriptionsTotal] = useState(0)
  const [income, setIncome] = useState<IncomeSummary | null>(null)
  const [adding, setAdding] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(async (): Promise<void> => {
    const [budget, recurring, summary] = await Promise.all([
      window.api.financeBudget(month),
      window.api.financeRecurring(month),
      window.api.financeIncomeSummary(month)
    ])
    setView(budget)
    setIncome(summary)
    // zakończona rata zostaje na liście, jeśli opłacona w tym miesiącu; inaczej znika
    setFixed(recurring.filter((r) => r.kind === 'other' && (!r.ended || r.paidThisMonth)))
    setSubscriptionsTotal(recurring.filter((r) => r.kind === 'subscription' && !r.ended).reduce((sum, r) => sum + r.amount, 0))
  }, [month])

  useEffect(() => {
    setError(null)
    reload().catch((e: unknown) => setError(String(e)))
  }, [reload])

  const run = (action: () => Promise<unknown>): void => {
    setError(null)
    action()
      .then(reload)
      .catch((e: unknown) => setError(String(e)))
  }

  const unpaid = fixed.filter((r) => !r.paidThisMonth).reduce((sum, r) => sum + r.amount, 0)
  const spareTotal = view ? view.envelopes.filter((e) => e.id !== 0).reduce((sum, e) => sum + e.available, 0) : 0
  const showIncome = !!income && (income.salary > 0 || income.oneOff > 0)

  return (
    <div className="bud">
      <header className="bud-top">
        <nav className="bud-nav" aria-label="Wybór miesiąca">
          <button className="bud-arrow" onClick={() => setMonth(shiftMonth(month, -1))} aria-label="Poprzedni miesiąc">
            ‹
          </button>
          <div className="bud-nav-title">
            <strong>{monthLabel(month)}</strong>
            {month !== currentMonth() && (
              <button className="bud-today" onClick={() => setMonth(currentMonth())}>
                Wróć do bieżącego
              </button>
            )}
          </div>
          <button className="bud-arrow" onClick={() => setMonth(shiftMonth(month, 1))} aria-label="Następny miesiąc">
            ›
          </button>
        </nav>

        {showIncome && (
          <div className="bud-stats" aria-label="Wpływy w miesiącu">
            <div className="bud-stat">
              <span className="bud-stat-label">Wypłaty</span>
              <span className="bud-stat-value">{formatMoney(income!.salary)}</span>
            </div>
            <div className="bud-stat">
              <span className="bud-stat-label">Jednorazowe</span>
              <span className="bud-stat-value">{formatMoney(income!.oneOff)}</span>
            </div>
          </div>
        )}
      </header>

      {view && <CushionCard view={view} onChanged={() => run(() => Promise.resolve())} onError={setError} />}

      {view && (
        <section className="bud-card">
          <header className="bud-card-head">
            <h3>Koperty</h3>
            <span className="bud-card-sub">Zostało łącznie <b className={spareTotal < 0 ? 'neg' : ''}>{formatMoney(spareTotal)}</b></span>
          </header>
          <div className="bud-env bud-env-head">
            <span>Koperta</span>
            <span className="r">Plan</span>
            <span className="r">Z poprzedniego</span>
            <span className="r">Wydano</span>
            <span className="r">Zostało</span>
          </div>
          {view.envelopes.map((e) => (
            <EnvelopeRow key={`${e.id}-${month}-${e.planned}`} envelope={e} month={month} onSaved={() => run(() => Promise.resolve())} onError={setError} />
          ))}
          <div className="bud-note">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" aria-hidden>
              <circle cx="12" cy="12" r="9" />
              <path d="M12 11v5M12 8h.01" />
            </svg>
            <span>Plan przechodzi z poprzedniego miesiąca i można go zmienić. Niewykorzystane kwoty przechodzą dalej.</span>
          </div>
        </section>
      )}

      <section className="bud-card">
        <header className="bud-card-head">
          <h3>Stałe płatności</h3>
          <div className="bud-card-actions">
            <button className="fin-link" onClick={onOpenSubscriptions}>
              Subskrypcje →
            </button>
            {!adding && (
              <button className="fin-link" onClick={() => setAdding(true)}>
                Dodaj
              </button>
            )}
          </div>
        </header>
        {adding && (
          <RecurringForm
            defaultKind="other"
            onSaved={() => {
              setAdding(false)
              run(() => Promise.resolve())
            }}
            onCancel={() => setAdding(false)}
          />
        )}
        {fixed.length === 0 && !adding && <div className="fin-empty">Brak stałych płatności. Dodaj ratę albo poczekaj na wykrycie z historii.</div>}
        {fixed.map((item) => (
          <RecurringRow
            key={item.id}
            item={item}
            showPaid
            month={month}
            onChanged={() => run(() => Promise.resolve())}
            onError={setError}
          />
        ))}
        {fixed.length > 0 && <div className="bud-footer">Do opłacenia w tym miesiącu: {formatMoney(unpaid)}</div>}
        {subscriptionsTotal > 0 && (
          <div className="bud-footer">
            Subskrypcje: {formatMoney(subscriptionsTotal)} miesięcznie ({formatMoney(subscriptionsTotal * 12)} rocznie)
          </div>
        )}
      </section>

      {error && <div className="error">{error}</div>}
    </div>
  )
}

function CushionCard({ view, onChanged, onError }: { view: BudgetView; onChanged: () => void; onError: (text: string) => void }): JSX.Element {
  const { cushion } = view
  const [target, setTarget] = useState(moneyInput(cushion.target))
  const ratio = cushion.target > 0 ? (cushion.balance / cushion.target) * 100 : 100
  const percent = Math.max(0, Math.min(100, ratio))
  // przy bardzo małym postępie zostawiamy widoczny ślad paska
  const barWidth = percent > 0 ? Math.max(percent, 1.5) : 0

  useEffect(() => setTarget(moneyInput(cushion.target)), [cushion.target])

  const save = (): void => {
    const grosze = parseMoney(target)
    if (grosze === null || grosze < 0) {
      onError('Podaj cel poduszki w zł (0 lub więcej)')
      setTarget(moneyInput(cushion.target))
      return
    }
    window.api
      .financeSetCushionTarget(grosze)
      .then(onChanged)
      .catch((e: unknown) => onError(String(e)))
  }

  return (
    <section className="bud-card bud-cushion">
      <header className="bud-card-head">
        <h3>Poduszka finansowa</h3>
        <span className="bud-card-sub">{formatPercent(ratio)} celu</span>
      </header>
      <div className="bud-cushion-grid">
        <div className="bud-cushion-cell">
          <span className="bud-label">Na koncie</span>
          <span className="bud-cushion-value">{formatMoney(cushion.balance)}</span>
        </div>
        <label className="bud-cushion-cell bud-cushion-input">
          <span className="bud-label">Cel (zł)</span>
          <input value={target} onChange={(e) => setTarget(e.target.value)} onBlur={save} onKeyDown={(e) => e.key === 'Enter' && save()} inputMode="decimal" />
        </label>
      </div>
      <div className="bud-progress" role="img" aria-label={`Realizacja celu: ${formatPercent(ratio)}`}>
        <span style={{ width: `${barWidth}%` }} />
      </div>
      {cushion.surplus > 0 ? (
        <div className="bud-alert">
          Poduszka przekroczyła cel o {formatMoney(cushion.surplus)}. Te pieniądze warto przenieść na konto oszczędnościowe albo do inwestycji.
        </div>
      ) : (
        !cushion.reached && <div className="bud-hint bud-hint-flush">Do celu brakuje {formatMoney(cushion.target - cushion.balance)}.</div>
      )}
    </section>
  )
}

function EnvelopeRow({
  envelope,
  month,
  onSaved,
  onError
}: {
  envelope: BudgetEnvelope
  month: string
  onSaved: () => void
  onError: (text: string) => void
}): JSX.Element {
  const [planned, setPlanned] = useState(moneyInput(envelope.planned))
  // „Bez Koperty” nie ma planu: pokazujemy tylko, ile wydano bez kategorii
  const unassigned = envelope.id === 0
  // pojemność koperty w tym miesiącu: plan plus przeniesienie; pasek pokazuje, jaka jej część poszła
  const capacity = envelope.carryIn + envelope.planned
  const spentRatio = capacity > 0 ? Math.min(100, (envelope.actual / capacity) * 100) : 0
  const over = envelope.available < 0

  const save = (): void => {
    const grosze = parseMoney(planned)
    if (grosze === null || grosze < 0) {
      onError('Plan koperty: kwota 0 zł lub więcej')
      setPlanned(moneyInput(envelope.planned))
      return
    }
    if (grosze === envelope.planned) return
    window.api
      .financeSetPlanned(envelope.id, month, grosze)
      .then(onSaved)
      .catch((e: unknown) => onError(String(e)))
  }

  return (
    <div className="bud-env bud-env-row">
      <div className="bud-env-name">
        <span className="bud-env-title">{envelope.name}</span>
        {!unassigned && capacity > 0 && (
          <div className="bud-env-bar" aria-hidden>
            <span className={over ? 'over' : ''} style={{ width: `${spentRatio}%` }} />
          </div>
        )}
      </div>
      <span className="r">
        <input
          className="bud-planned"
          value={unassigned ? '—' : planned}
          disabled={unassigned}
          inputMode="decimal"
          aria-label={`Plan: ${envelope.name}`}
          onChange={(e) => setPlanned(e.target.value)}
          onBlur={save}
          onKeyDown={(e) => e.key === 'Enter' && save()}
        />
      </span>
      <span className="r bud-muted">{formatMoney(envelope.carryIn)}</span>
      <span className="r">{formatMoney(envelope.actual)}</span>
      <span className={`r bud-left ${over ? 'neg' : ''}`}>{formatMoney(envelope.available)}</span>
    </div>
  )
}
