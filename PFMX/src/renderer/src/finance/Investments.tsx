import { useCallback, useEffect, useState } from 'react'
import type { InvestEvent, InvestHolding } from '@shared/types'
import { formatDate, formatMoney, isoDate } from './format'

type Tab = 'assets' | 'history'

const CASH_LABEL: Record<string, string> = {
  deposit: 'Wpłata',
  withdrawal: 'Wypłata',
  interest: 'Odsetki',
  interest_tax: 'Podatek od odsetek'
}

/** Ilość w dziesięciotysięcznych -> tekst, bez zbędnych zer: 1092 -> „0,1092”, 20000 -> „2” */
function formatVolume(scaled: number): string {
  return (scaled / 10000).toLocaleString('pl-PL', { maximumFractionDigits: 4 })
}

/** Cena w walucie instrumentu, bez przeliczania (w pliku XTB jest w walucie waloru) */
function formatPrice(scaled: number): string {
  return (scaled / 10000).toLocaleString('pl-PL', { minimumFractionDigits: 2, maximumFractionDigits: 4 })
}

function formatWhen(ms: number): string {
  return formatDate(isoDate(new Date(ms)))
}

/** Inwestycje: aktywa (co posiadasz, za ile kupiono) i historia ruchów z eksportu XTB. */
export default function Investments({ onNotice, onError }: { onNotice: (text: string) => void; onError: (text: string) => void }): JSX.Element {
  const [tab, setTab] = useState<Tab>('assets')
  const [holdings, setHoldings] = useState<InvestHolding[] | null>(null)
  const [history, setHistory] = useState<InvestEvent[] | null>(null)
  const [busy, setBusy] = useState(false)

  const reload = useCallback(async (): Promise<void> => {
    const [h, e] = await Promise.all([window.api.financeInvestHoldings(), window.api.financeInvestHistory()])
    setHoldings(h)
    setHistory(e)
  }, [])

  useEffect(() => {
    reload().catch((e: unknown) => onError(String(e)))
  }, [reload, onError])

  const importXtb = async (): Promise<void> => {
    setBusy(true)
    try {
      const report = await window.api.financeInvestImport()
      if (report) {
        const parts = [`Dodano ${report.added}`]
        if (report.existing > 0) parts.push(`już były ${report.existing}`)
        if (report.skipped > 0) parts.push(`pominięto ${report.skipped}`)
        onNotice(`Import XTB: ${parts.join(', ')}.`)
        onError('')
        await reload()
      }
    } catch (e: unknown) {
      onError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const totalCost = (holdings ?? []).reduce((sum, h) => sum + h.cost, 0)

  return (
    <div className="inv">
      <div className="inv-bar">
        <div className="inv-tabs" role="tablist" aria-label="Widok inwestycji">
          <button role="tab" aria-selected={tab === 'assets'} className={`inv-tab ${tab === 'assets' ? 'is-on' : ''}`} onClick={() => setTab('assets')}>
            Aktywa
          </button>
          <button role="tab" aria-selected={tab === 'history'} className={`inv-tab ${tab === 'history' ? 'is-on' : ''}`} onClick={() => setTab('history')}>
            Historia
          </button>
        </div>
        <button className="inv-import" onClick={() => void importXtb()} disabled={busy}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />
          </svg>
          {busy ? 'Importuję…' : 'Importuj XTB'}
        </button>
      </div>

      {tab === 'assets' && (
        <section className="inv-section" aria-label="Aktywa">
          {holdings === null ? (
            <div className="fin-empty">Ładowanie…</div>
          ) : holdings.length === 0 ? (
            <div className="inv-empty">
              <p>Brak otwartych pozycji w imporcie.</p>
              <span>Zaimportuj eksport historii z XTB. Aktywa pokażą się, gdy zostanie coś kupione i nie sprzedane w całości.</span>
            </div>
          ) : (
            <>
              <div className="inv-row inv-row-head">
                <span>Nazwa</span>
                <span className="r">Ilość</span>
                <span className="r">Koszt zakupu</span>
              </div>
              {holdings.map((h) => (
                <div className="inv-row" key={h.ticker}>
                  <span className="inv-name">
                    <span className="inv-title">{h.instrument || h.ticker}</span>
                    <span className="inv-sub">{h.ticker}</span>
                  </span>
                  <span className="r inv-num">{formatVolume(h.volume)}</span>
                  <span className="r inv-num">{formatMoney(h.cost)}</span>
                </div>
              ))}
              <div className="inv-footer">Razem w kosztach zakupu: {formatMoney(totalCost)}</div>
            </>
          )}
        </section>
      )}

      {tab === 'history' && (
        <section className="inv-section" aria-label="Historia">
          {history === null ? (
            <div className="fin-empty">Ładowanie…</div>
          ) : history.length === 0 ? (
            <div className="inv-empty">
              <p>Brak operacji.</p>
              <span>Zaimportuj eksport historii z XTB.</span>
            </div>
          ) : (
            history.map((e) =>
              e.kind === 'buy' || e.kind === 'sell' ? (
                <div className="inv-trade" key={e.id}>
                  <span className={`inv-tag ${e.kind}`}>{e.kind === 'buy' ? 'Kupno' : 'Sprzedaż'}</span>
                  <span className="inv-name">
                    <span className="inv-title">{e.instrument || e.ticker}</span>
                    <span className="inv-sub">{formatWhen(e.occurredAt)}</span>
                  </span>
                  <span className="inv-trade-nums">
                    <span>
                      <i>{e.kind === 'buy' ? 'kupiono' : 'sprzedano'}</i> {formatVolume(e.volume)}
                    </span>
                    {e.leftAfter !== null && (
                      <span>
                        <i>zostało</i> {formatVolume(e.leftAfter)}
                      </span>
                    )}
                    <span>
                      <i>cena</i> {formatPrice(e.price)}
                    </span>
                  </span>
                  <span className={`inv-amount ${e.amount >= 0 ? 'pos' : 'neg'}`}>{formatMoney(e.amount, 'PLN', true)}</span>
                </div>
              ) : (
                <div className="inv-cash" key={e.id}>
                  <span className={`inv-cash-dot ${e.amount >= 0 ? 'pos' : 'neg'}`} aria-hidden />
                  <span className="inv-cash-label">{CASH_LABEL[e.kind] ?? e.kind}</span>
                  <span className="inv-sub">{formatWhen(e.occurredAt)}</span>
                  <span className={`inv-amount ${e.amount >= 0 ? 'pos' : 'neg'}`}>{formatMoney(e.amount, 'PLN', true)}</span>
                </div>
              )
            )
          )}
        </section>
      )}
    </div>
  )
}
