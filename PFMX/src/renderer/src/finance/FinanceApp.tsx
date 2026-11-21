import { useCallback, useEffect, useRef, useState } from 'react'
import type { AutoImportReport, BudgetEnvelope, BudgetView, CashState, DailyFlow, FinanceAccountSummary, FinanceAiStatus, FinanceKind, FinanceTransaction, IncomeKind, ReceiptInfo, Recurring } from '@shared/types'
import Budget from './Budget'
import { AccountsSection, KIND_LABEL } from './Accounts'
import Subscriptions from './Subscriptions'
import Investments from './Investments'
import { Alerts } from './Alerts'
import Cash from './Cash'
import { countFilters, FilterPopup, NO_FILTERS, type Filters } from './FilterPopup'
import { buildAlerts } from './alertRules'
import { cumulativeSeries, currentMonth, formatDate, formatMoney, formatPercent, isoDate, monthLabel, windowStart } from './format'
import './finance.css'

type Section = 'overview' | 'accounts' | 'transactions' | 'budget' | 'cash' | 'subscriptions' | 'investments'

const MAIN: { id: Section; label: string; key: string }[] = [
  { id: 'overview', label: 'Przegląd', key: 'o' },
  { id: 'accounts', label: 'Konta', key: 'a' },
  { id: 'transactions', label: 'Transakcje', key: 't' },
  { id: 'budget', label: 'Budżet', key: 'b' },
  { id: 'cash', label: 'Gotówka', key: 'k' },
  { id: 'investments', label: 'Inwestycje', key: 'i' },
  { id: 'subscriptions', label: 'Subskrypcje', key: 's' }
]


const TITLES: Record<Section, { title: string; subtitle: string }> = {
  overview: { title: 'Przegląd', subtitle: 'Stan wszystkich kont' },
  accounts: { title: 'Konta', subtitle: 'Rachunki, z których liczymy saldo' },
  transactions: { title: 'Transakcje', subtitle: 'Historia z importów' },
  budget: { title: 'Budżet', subtitle: 'Koperty, poduszka i stałe płatności' },
  cash: { title: 'Gotówka', subtitle: 'Ile masz w portfelu i skąd się bierze' },
  subscriptions: { title: 'Subskrypcje', subtitle: 'Płatności, które wracają co miesiąc' },
  investments: { title: 'Inwestycje', subtitle: 'Aktywa i historia ruchów z XTB' }
}

/** Skróty G + litera: przełączają sekcję, gdy nie piszemy w polu tekstowym */
const SHORTCUTS: Record<string, Section> = Object.fromEntries(MAIN.map((m) => [m.key, m.id])) as Record<string, Section>


/** Czy transakcja pasuje do wpisanego tekstu (bez względu na wielkość liter) */
function matchesText(t: FinanceTransaction, query: string): boolean {
  const q = query.trim().toLowerCase()
  if (q === '') return true
  return `${t.counterparty} ${t.description} ${t.kind} ${t.accountName}`.toLowerCase().includes(q)
}

/** Sumujemy tylko konta w złotówkach; inwestycje w innych walutach dojdą z kursami. */
function totals(accounts: FinanceAccountSummary[]): { all: number; byKind: Partial<Record<FinanceKind, number>> } {
  const plnBanking = accounts.filter((a) => a.currency === 'PLN' && a.kind !== 'broker')
  const byKind: Partial<Record<FinanceKind, number>> = {}
  for (const a of plnBanking) byKind[a.kind] = (byKind[a.kind] ?? 0) + a.balance
  return { all: plnBanking.reduce((sum, a) => sum + a.balance, 0), byKind }
}

/** Ostatnie 30 dni: skumulowany przepływ netto jako linia z miękkim gradientem pod spodem */
function Sparkline({ values }: { values: number[] }): JSX.Element {
  const width = 220
  const height = 44
  const pad = 3
  const min = Math.min(0, ...values)
  const max = Math.max(0, ...values)
  const span = max - min || 1
  const x = (i: number): number => (values.length > 1 ? (i / (values.length - 1)) * width : 0)
  const y = (v: number): number => pad + (1 - (v - min) / span) * (height - 2 * pad)
  const line = values.map((v, i) => `${i === 0 ? 'M' : 'L'}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ')
  const area = `${line} L${width},${y(0).toFixed(1)} L0,${y(0).toFixed(1)} Z`
  return (
    <svg className="fin-spark" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label="Przepływ netto z ostatnich 30 dni">
      <defs>
        <linearGradient id="fin-spark-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: 'var(--fin-accent)', stopOpacity: 0.28 }} />
          <stop offset="1" style={{ stopColor: 'var(--fin-accent)', stopOpacity: 0 }} />
        </linearGradient>
      </defs>
      <path d={area} fill="url(#fin-spark-fill)" />
      <path d={line} fill="none" stroke="var(--fin-accent)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

/** Znacznik przed transakcją: przelew, BLIK albo pierwsza litera odbiorcy */
function txGlyph(t: FinanceTransaction): string {
  const text = `${t.kind} ${t.description}`.toUpperCase()
  if (text.includes('BLIK')) return 'BLIK'
  if (text.includes('PRZELEW')) return '⇄'
  return (t.counterparty || t.kind || '?').trim().charAt(0).toUpperCase()
}

/** Mikro-ikony etykiet w szczegółach transakcji */
function DetailIcon({ name }: { name: 'calendar' | 'wallet' | 'tag' | 'chart' }): JSX.Element {
  const svg = { width: 12, height: 12, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const, 'aria-hidden': true, className: 'fin-tx-ico' }
  if (name === 'calendar') {
    return (
      <svg {...svg}>
        <rect x="3.5" y="5" width="17" height="15" rx="2.5" />
        <path d="M3.5 10h17M8 3v4M16 3v4" />
      </svg>
    )
  }
  if (name === 'wallet') {
    return (
      <svg {...svg}>
        <path d="M4 7.5h14.5a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" />
        <path d="M4 7.5 6 5h10" />
        <circle cx="16.5" cy="13" r="1" />
      </svg>
    )
  }
  if (name === 'tag') {
    return (
      <svg {...svg}>
        <path d="M3.5 12.5V4.5a1 1 0 0 1 1-1h8l8 8-8.5 8.5z" />
        <circle cx="8" cy="8" r="1.4" />
      </svg>
    )
  }
  return (
    <svg {...svg}>
      <path d="M3.5 20.5h17M6 16.5l4-5 3.5 3 5-6.5" />
    </svg>
  )
}

/** Minimalistyczna ikona kanału płatności: BLIK, przelew, karta, a w pozostałych przypadkach inicjał */
function ChannelIcon({ t }: { t: FinanceTransaction }): JSX.Element {
  const text = `${t.kind} ${t.description}`.toUpperCase()
  const svg = { width: 14, height: 14, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const, 'aria-hidden': true }
  if (text.includes('BLIK')) {
    return (
      <svg {...svg}>
        <rect x="7" y="2.5" width="10" height="19" rx="2.5" />
        <path d="M11 18.5h2" />
      </svg>
    )
  }
  if (text.includes('PRZELEW')) {
    return (
      <svg {...svg}>
        <path d="M4 8h14l-3-3M20 16H6l3 3" />
      </svg>
    )
  }
  if (text.includes('KART')) {
    return (
      <svg {...svg}>
        <rect x="2.5" y="5.5" width="19" height="13" rx="2.5" />
        <path d="M2.5 10h19M6 15h3" />
      </svg>
    )
  }
  return <>{txGlyph(t)}</>
}

function Tile({ label, value }: { label: string; value: string }): JSX.Element {
  return (
    <div className="fin-tile">
      <div className="fin-tile-label">{label}</div>
      <div className="fin-tile-value">{value}</div>
    </div>
  )
}

function Card({ title, action, children }: { title: string; action?: JSX.Element; children: React.ReactNode }): JSX.Element {
  return (
    <section className="fin-card">
      <header className="fin-card-head">
        <h3>{title}</h3>
        {action}
      </header>
      {children}
    </section>
  )
}

function AccountRows({ accounts }: { accounts: FinanceAccountSummary[] }): JSX.Element {
  return (
    <>
      {accounts.map((a) => (
        <div className="ov-row" key={a.id}>
          <span className={`fin-dot fin-dot-${a.kind}`} aria-hidden />
          <div className="fin-main">
            <div className="fin-title">{a.name}</div>
            <div className="fin-sub">
              {KIND_LABEL[a.kind]}
              {a.ibanMasked ? ` · ${a.ibanMasked}` : ''}
              {a.lastBookedOn ? ` · ostatnia ${formatDate(a.lastBookedOn)}` : ''}
            </div>
          </div>
          <div className="fin-amount">{formatMoney(a.balance, a.currency)}</div>
        </div>
      ))}
    </>
  )
}

/** Paragon przy transakcji: dołącz zdjęcie albo podejrzyj zapisany (ładuje się przy rozwinięciu wiersza) */
function ReceiptActions({ transactionId }: { transactionId: number }): JSX.Element {
  const [info, setInfo] = useState<ReceiptInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    window.api
      .financeReceiptInfo(transactionId)
      .then((r) => alive && setInfo(r))
      .catch((e: unknown) => alive && setError(String(e)))
    return () => {
      alive = false
    }
  }, [transactionId])

  const attach = (): void => {
    setBusy(true)
    setError(null)
    window.api
      .financeReceiptAttach(transactionId)
      .then((r) => {
        if (r) setInfo(r)
      })
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false))
  }
  const show = (): void => {
    setError(null)
    window.api.financeReceiptOpen(transactionId).catch((e: unknown) => setError(String(e)))
  }
  // usunięcie wymaga drugiego kliknięcia, żeby przypadkiem nie skasować dobrego paragonu
  const [confirming, setConfirming] = useState(false)
  const remove = (): void => {
    if (!confirming) {
      setConfirming(true)
      return
    }
    setConfirming(false)
    setBusy(true)
    setError(null)
    window.api
      .financeReceiptRemove(transactionId)
      .then(() => setInfo(null))
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false))
  }

  return (
    <div className="fin-tx-receipts">
      <button className="fin-tx-action" onClick={attach} disabled={busy}>
        Dołącz paragon
      </button>
      <button className="fin-tx-action" onClick={show} disabled={!info}>
        Wyświetl paragon
      </button>
      <button className="fin-tx-action fin-tx-remove" onClick={remove} disabled={busy || !info}>
        {confirming ? 'Na pewno usunąć?' : 'Usuń paragon'}
      </button>
      {info && <span className="fin-tx-receipt-name">{info.fileName}</span>}
      {!info && <span className="fin-tx-receipt-name">brak paragonu</span>}
      {error && <span className="fin-tx-receipt-error">{error}</span>}
    </div>
  )
}

function TransactionRows({
  rows,
  showAccount,
  envelopes,
  onTagged,
  fancyIcons = false
}: {
  rows: FinanceTransaction[]
  showAccount: boolean
  envelopes: BudgetEnvelope[]
  /** ikony kanałów płatności zamiast tekstu (lista Transakcji) */
  fancyIcons?: boolean
  /** learned = ile innych transakcji dostało kategorię przez zapamiętaną regułę */
  onTagged: (error: string | null, learned?: number) => void
}): JSX.Element {
  // rozwinięty wiersz pokazuje szczegóły; jeden naraz
  const [open, setOpen] = useState<number | null>(null)
  const setIncome = (transactionId: number, kind: IncomeKind): void => {
    window.api
      .financeIncomeSet(transactionId, kind)
      .then(() => onTagged(null))
      .catch((e: unknown) => onTagged(String(e)))
  }

  const tag = (transactionId: number, value: string): void => {
    window.api
      .financeTagTransaction(transactionId, value === '' ? null : Number(value))
      .then((learned) => onTagged(null, learned))
      .catch((e: unknown) => onTagged(String(e)))
  }
  return (
    <>
      {rows.map((t) => {
        const expanded = open === t.id
        return (
          <div className={`fin-tx ${expanded ? 'open' : ''}`} key={t.id}>
            <div className="fin-row fin-tx-row">
              <div className="fin-date">{formatDate(t.bookedOn)}</div>
              <span className={`fin-avatar ${fancyIcons ? 'fin-chan' : ''}`} aria-hidden>
                {fancyIcons ? <ChannelIcon t={t} /> : txGlyph(t)}
              </span>
              <div className="fin-main">
                <div className="fin-title">{t.counterparty || t.kind}</div>
                <div className="fin-sub">
                  {t.description}
                  {showAccount ? ` · ${t.accountName}` : ''}
                </div>
              </div>
              {t.ownTransfer ? (
                <span className="fin-tag fin-tag-static">Przelew własny</span>
              ) : t.amount > 0 ? (
                <select
                  id={`tx-cat-${t.id}`}
                  className="fin-tag"
                  aria-label="Rodzaj wpływu"
                  value={t.incomeKind ?? ''}
                  onChange={(e) => setIncome(t.id, e.target.value as IncomeKind)}
                >
                  <option value="" disabled>
                    wybierz rodzaj
                  </option>
                  <option value="salary">Wypłata</option>
                  <option value="one_off">Jednorazowy wpływ</option>
                </select>
              ) : envelopes.length > 0 && (
                <select
                  id={`tx-cat-${t.id}`}
                  className="fin-tag"
                  aria-label="Koperta transakcji"
                  value={t.envelopeId ?? ''}
                  onChange={(e) => tag(t.id, e.target.value)}
                >
                  <option value="">Bez Koperty</option>
                  {envelopes.map((e) => (
                    <option key={e.id} value={e.id}>
                      {e.name}
                    </option>
                  ))}
                </select>
              )}
              <div className={`fin-amount ${t.amount > 0 ? 'pos' : t.amount < 0 ? 'neg' : ''}`}>{formatMoney(t.amount, t.currency, true)}</div>
              <button
                className="fin-peek"
                aria-label={expanded ? 'Zwiń szczegóły' : 'Pokaż szczegóły'}
                aria-expanded={expanded}
                onClick={() => setOpen(expanded ? null : t.id)}
              >
                →
              </button>
            </div>
            {expanded && fancyIcons && (
              <div className="fin-tx-details fancy">
                <dl className="fin-tx-meta">
                  <div className="fin-tx-cell">
                    <dt>
                      <DetailIcon name="calendar" />
                      Data
                    </dt>
                    <dd>{formatDate(t.bookedOn)}</dd>
                  </div>
                  <div className="fin-tx-cell">
                    <dt>
                      <DetailIcon name="wallet" />
                      Konto
                    </dt>
                    <dd>{t.accountName}</dd>
                  </div>
                  <div className="fin-tx-cell">
                    <dt>
                      <DetailIcon name="tag" />
                      Rodzaj
                    </dt>
                    <dd>{t.kind || '—'}</dd>
                  </div>
                  <div className="fin-tx-cell">
                    <dt>
                      <DetailIcon name="chart" />
                      Saldo po operacji
                    </dt>
                    <dd>{formatMoney(t.balanceAfter, t.currency)}</dd>
                  </div>
                  <div className="fin-tx-cell wide">
                    <dt>Opis</dt>
                    <dd>{t.description || '—'}</dd>
                  </div>
                </dl>
                <div className="fin-tx-actions">
                  {!t.ownTransfer && (t.amount > 0 || envelopes.length > 0) && (
                    <button
                      className="fin-tx-action"
                      onClick={() => document.getElementById(`tx-cat-${t.id}`)?.focus()}
                    >
                      {t.amount > 0 ? 'Zmień rodzaj' : 'Zmień kategorię'}
                    </button>
                  )}
                </div>
                <ReceiptActions transactionId={t.id} />
              </div>
            )}
            {expanded && !fancyIcons && (
              <>
              <dl className="fin-tx-details">
                <div>
                  <dt>Data</dt>
                  <dd>{formatDate(t.bookedOn)}</dd>
                </div>
                <div>
                  <dt>Konto</dt>
                  <dd>{t.accountName}</dd>
                </div>
                <div>
                  <dt>Rodzaj</dt>
                  <dd>{t.kind || '—'}</dd>
                </div>
                <div>
                  <dt>Saldo po operacji</dt>
                  <dd>{formatMoney(t.balanceAfter, t.currency)}</dd>
                </div>
                <div className="wide">
                  <dt>Opis</dt>
                  <dd>{t.description || '—'}</dd>
                </div>
              </dl>
              <ReceiptActions transactionId={t.id} />
              </>
            )}
          </div>
        )
      })}
    </>
  )
}

export default function FinanceApp(): JSX.Element {
  const [section, setSection] = useState<Section>('overview')
  const [accounts, setAccounts] = useState<FinanceAccountSummary[]>([])
  const [transactions, setTransactions] = useState<FinanceTransaction[]>([])
  const [budget, setBudget] = useState<BudgetView | null>(null)
  const [flow, setFlow] = useState<DailyFlow[]>([])
  const [recurring, setRecurring] = useState<Recurring[]>([])
  const [aiStatus, setAiStatus] = useState<FinanceAiStatus | null>(null)
  const [aiBusy, setAiBusy] = useState(false)
  const [cash, setCash] = useState<CashState | null>(null)
  // zamknięte powiadomienia: zapamiętane w tej przeglądarce, żeby nie wracały
  const [dismissed, setDismissed] = useState<string[]>(() => {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem('pfmx.alertsDismissed') ?? '[]')
      return Array.isArray(saved) ? saved.filter((x): x is string => typeof x === 'string') : []
    } catch {
      return []
    }
  })
  // w sekcji Transakcje można zawęzić do jednego konta; null = wszystkie
  const [filter, setFilter] = useState<number | null>(null)
  // kategoria działa od razu; filtry z okna działają, dopóki nie wyjdziemy z sekcji albo nie zrestartujemy ich
  const [category, setCategory] = useState('')
  const [filters, setFilters] = useState<Filters | null>(null)
  const [popupOpen, setPopupOpen] = useState(false)
  // szukanie tekstowe działa na wczytanej liście: odbiorca, opis i rodzaj
  const [text, setText] = useState('')
  // widok samych przelewów między własnymi kontami
  const [onlyOwn, setOnlyOwn] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const reload = useCallback(async (): Promise<void> => {
    // na ekranie Transakcje: konto, kategoria i filtry razem (wyszukiwanie zawsze zwraca spójny wynik)
    const txs =
      section === 'transactions'
        ? window.api.financeSearchTransactions({
            accountId: filter,
            category: category === '' ? null : Number(category),
            kind: filters?.kind ?? null,
            dateFrom: filters?.dateFrom ?? null,
            dateTo: filters?.dateTo ?? null,
            amountMin: filters?.amountMin ?? null,
            amountMax: filters?.amountMax ?? null
          })
        : window.api.financeTransactions(null, 8)
    if (section === 'transactions' && countFilters(filters) > 0) {
      txs.then((rows) => setNotice(`Wyniki filtrowania: ${rows.length}`)).catch(() => undefined)
    }
    const [accs, transactionsNow, view, days, plans, wallet] = await Promise.all([
      window.api.financeAccounts(),
      txs,
      window.api.financeBudget(currentMonth()),
      window.api.financeDailyFlow(windowStart(30)),
      window.api.financeRecurring(currentMonth()),
      window.api.financeCashState()
    ])
    setCash(wallet)
    setAccounts(accs)
    setTransactions(transactionsNow)
    setBudget(view)
    setFlow(days)
    setRecurring(plans)
  }, [section, filter, category, filters])

  useEffect(() => {
    reload().catch((e: unknown) => setError(String(e)))
  }, [reload])

  useEffect(() => {
    if (section === 'transactions') void window.api.financeAiStatus().then(setAiStatus).catch(() => setAiStatus(null))
  }, [section])

  // odczyt folderu Millennium przy otwarciu i co minutę; tło czyta go też, gdy okno jest schowane
  // ref, żeby zmiana sekcji nie uruchamiała odczytu od nowa
  const reloadRef = useRef(reload)
  reloadRef.current = reload
  const readExports = useCallback(async (): Promise<void> => {
    try {
      const report: AutoImportReport = await window.api.financeAutoImportNow()
      const added = report.imported.reduce((sum, f) => sum + f.added, 0)
      if (report.errors.length > 0) setError(report.errors.join('\n'))
      if (added > 0) {
        setNotice(`Z folderu Millennium dodano ${added} transakcji.`)
        await reloadRef.current()
      }
    } catch (e) {
      setError(String(e))
    }
  }, [])

  useEffect(() => {
    void readExports()
    const timer = setInterval(() => void readExports(), 60_000)
    return () => clearInterval(timer)
  }, [readExports])

  const dismiss = (key: string): void => {
    setDismissed((prev) => {
      const next = [...prev, key].slice(-200)
      try {
        localStorage.setItem('pfmx.alertsDismissed', JSON.stringify(next))
      } catch {
        // brak zapisu = powiadomienie wróci po odświeżeniu
      }
      return next
    })
  }

  const runAi = async (): Promise<void> => {
    setAiBusy(true)
    setError(null)
    try {
      const report = await window.api.financeAiRun()
      setNotice(`Przypisano ${report.tagged} wydatków. Zostało ${report.remaining} bez kategorii.`)
      await reload()
    } catch (e) {
      setError(String(e))
    } finally {
      setAiBusy(false)
      void window.api.financeAiStatus().then(setAiStatus).catch(() => setAiStatus(null))
    }
  }

  const exportCsv = async (): Promise<void> => {
    setError(null)
    setNotice(null)
    try {
      const path = await window.api.financeExportCsv(filter)
      if (path) setNotice(`Zapisano eksport: ${path}`)
    } catch (e) {
      setError(String(e))
    }
  }

  const go = (next: Section): void => {
    setError(null)
    setNotice(null)
    setCategory('')
    setFilters(null)
    setPopupOpen(false)
    setSection(next)
  }

  // G, potem litera sekcji (w ciągu 1,5 s); nie działa w polach tekstowych ani z klawiszami modyfikującymi
  useEffect(() => {
    let gAt = 0
    const onKey = (e: KeyboardEvent): void => {
      if (e.metaKey || e.ctrlKey || e.altKey) return
      const t = e.target as HTMLElement
      if (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName)) return
      const key = e.key.toLowerCase()
      if (Date.now() - gAt < 1500 && SHORTCUTS[key]) {
        gAt = 0
        e.preventDefault()
        go(SHORTCUTS[key])
      } else {
        gAt = key === 'g' ? Date.now() : 0
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const importCsv = async (): Promise<void> => {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const report = await window.api.financeImportCsv()
      if (!report) return
      const per = Object.entries(report.perAccount)
        .map(([name, count]) => `${name}: ${count}`)
        .join(' · ')
      setNotice(`Dodano ${report.added}, pominięto ${report.skipped} już znanych${per ? `. ${per}` : ''}.`)
      await reload()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const totalsNow = totals(accounts)
  const head = TITLES[section]
  const hasAccounts = accounts.length > 0
  // „Bez Koperty” (id 0) to nie koperta: nie wchodzi do wyboru ani do sum planu
  const envelopes = (budget?.envelopes ?? []).filter((e) => e.id !== 0)
  const spareTotal = envelopes.reduce((sum, e) => sum + e.available, 0)

  let body: JSX.Element
  if (section === 'overview') {
    const planned = envelopes.reduce((sum, e) => sum + e.planned, 0)
    const spent = envelopes.reduce((sum, e) => sum + e.actual, 0)
    const budgetPercent = planned > 0 ? Math.min(100, Math.round((spent / planned) * 100)) : 0
    const cushion = budget?.cushion
    const cushionRatio = cushion && cushion.target > 0 ? (cushion.balance / cushion.target) * 100 : 0
    const cushionPercent = Math.min(100, cushionRatio)
    body = (
      <div className="ov">
        <div className="fin-tiles">
          <div className="fin-tile fin-tile-spark">
            <div className="fin-tile-label">Razem</div>
            <div className="fin-tile-value">{formatMoney(totalsNow.all)}</div>
            <Sparkline values={cumulativeSeries(flow, 30)} />
          </div>
          <Tile label="Konto zwykłe" value={formatMoney(totalsNow.byKind.bank ?? 0)} />
          <Tile label="Oszczędności" value={formatMoney(totalsNow.byKind.savings ?? 0)} />
          <Tile label="Poduszka" value={formatMoney(totalsNow.byKind.buffer ?? 0)} />
        </div>
        {hasAccounts ? (
          <div className="fin-split">
            <div className="fin-col-main">
              <Card title="Ostatnie transakcje" action={<button className="fin-link" onClick={() => go('transactions')}>Wszystkie</button>}>
                {transactions.length > 0 ? (
                  <TransactionRows rows={transactions} showAccount envelopes={[]} onTagged={setError} />
                ) : (
                  <div className="fin-empty">Brak transakcji. Zaimportuj CSV z banku.</div>
                )}
              </Card>
              <Card title={`Budżet · ${monthLabel(currentMonth())}`} action={<button className="fin-link" onClick={() => go('budget')}>Otwórz</button>}>
                <div className="fin-budget">
                  <div className="fin-kv">
                    <span>Wydano</span>
                    <strong>
                      {planned > 0 ? `${formatMoney(spent)} z ${formatMoney(planned)}` : formatMoney(spent)}
                    </strong>
                  </div>
                  <div className="fin-progress" aria-hidden>
                    <span style={{ width: `${budgetPercent}%` }} />
                  </div>
                  <div className="fin-hint">
                    {planned > 0 ? `Zostało w kopertach ${formatMoney(spareTotal)}` : 'Ustaw plan w kopertach, żeby zobaczyć postęp.'}
                  </div>
                </div>
              </Card>
            </div>
            <aside className="fin-col-side ov-side">
              <div className="ov-block">
                <header className="ov-head">
                  <span className="ov-label">Konta</span>
                  <button className="ov-action" onClick={() => go('accounts')}>
                    Zarządzaj
                  </button>
                </header>
                <AccountRows accounts={accounts} />
              </div>
              <div className="ov-block">
                <header className="ov-head">
                  <span className="ov-label">Poduszka finansowa</span>
                  <button className="ov-action" onClick={() => go('budget')}>
                    Budżet
                  </button>
                </header>
                {cushion && (
                  <>
                    <div className="ov-kv">
                      <span>{formatPercent(cushionRatio)} celu</span>
                      <strong>
                        {formatMoney(cushion.balance)} / {formatMoney(cushion.target)}
                      </strong>
                    </div>
                    <div className="ov-glow-bar" aria-hidden>
                      <span style={{ width: `${cushionPercent > 0 ? Math.max(cushionPercent, 1.5) : 0}%` }} />
                    </div>
                    {cushion.surplus > 0 ? (
                      <div className="ov-note">Nadwyżka {formatMoney(cushion.surplus)} ponad cel. Przenieś ją na oszczędności albo inwestycje.</div>
                    ) : (
                      !cushion.reached && <div className="ov-note">Do celu brakuje {formatMoney(cushion.target - cushion.balance)}.</div>
                    )}
                  </>
                )}
              </div>
              <div className="ov-block ov-single">
                <span className="ov-micro">Gotówka</span>
                <span className="ov-single-end">
                  <strong>{formatMoney(cash?.balance ?? 0)}</strong>
                  <button className="ov-action" onClick={() => go('cash')}>
                    Otwórz
                  </button>
                </span>
              </div>
              <div className="ov-block ov-single">
                <span className="ov-micro">Oszczędności</span>
                <span className="ov-single-end">
                  <strong>{formatMoney(totalsNow.byKind.savings ?? 0)}</strong>
                  <button className="ov-action" onClick={() => go('accounts')}>
                    Konta
                  </button>
                </span>
              </div>
            </aside>
          </div>
        ) : (
          <div className="fin-empty-card">
            <p>Nie masz jeszcze kont. Dodaj je w sekcji Konta, a potem zaimportuj eksport z banku.</p>
            <button className="fin-ghost fin-ghost-inline" onClick={() => go('accounts')}>
              Dodaj konto
            </button>
          </div>
        )}
      </div>
    )
  } else if (section === 'accounts') {
    body = <AccountsSection accounts={accounts} onAdded={() => reload()} onNotice={setNotice} onError={setError} />
  } else if (section === 'transactions') {
    const shown = (onlyOwn ? transactions.filter((t) => t.ownTransfer) : transactions).filter((t) => matchesText(t, text))
    const actions = (
      <div className="fin-header-actions">
        <button className={`fin-pill ${onlyOwn ? 'is-on' : ''}`} onClick={() => setOnlyOwn((v) => !v)}>
          Przelewy własne
        </button>
        <button
          className="fin-pill"
          disabled={!aiStatus?.available || aiBusy}
          title={aiStatus?.message ?? 'Sprawdzam model…'}
          onClick={() => void runAi()}
        >
          {aiBusy ? 'Kategoryzuję…' : 'Kategoryzuj'}
        </button>
        <button className="fin-pill" onClick={() => void exportCsv()}>
          Eksport CSV
        </button>
      </div>
    )
    body = (
      <>
        <div className="fin-cmdbar">
          <div className="fin-cmdbar-row">
          <label className="fin-search-input">
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
              <circle cx="7" cy="7" r="4.8" fill="none" stroke="currentColor" strokeWidth="1.5" />
              <path d="M10.6 10.6 14 14" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            </svg>
            <input value={text} onChange={(e) => setText(e.target.value)} placeholder="Szukaj odbiorcy lub opisu" aria-label="Szukaj w transakcjach" />
          </label>
          <button className={`fin-pill ${countFilters(filters) > 0 ? 'is-on' : ''}`} onClick={() => setPopupOpen(true)}>
            {countFilters(filters) > 0 ? `Filtry (${countFilters(filters)})` : 'Filtry'}
          </button>
          <div className="fin-cmdbar-spacer" />
          {actions}
          </div>
          <div className="fin-cmdbar-row">
          <label className="fin-pill fin-pill-select">
            <span className="visually-hidden">Kategoria</span>
            <select className="fin-pill-input" value={category} onChange={(e) => setCategory(e.target.value)} aria-label="Kategoria">
              <option value="">Wszystkie kategorie</option>
              <option value="0">Bez Koperty</option>
              {envelopes.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                </option>
              ))}
            </select>
          </label>
          <div className="fin-pills" role="group" aria-label="Konto">
            <button className={`fin-pill ${filter === null ? 'is-on' : ''}`} onClick={() => setFilter(null)}>
              Wszystkie
            </button>
            {accounts.map((a) => (
              <button key={a.id} className={`fin-pill ${filter === a.id ? 'is-on' : ''}`} onClick={() => setFilter(a.id)}>
                {a.name}
              </button>
            ))}
          </div>
          </div>
        </div>
        {popupOpen && (
          <FilterPopup
            initial={filters}
            onApply={(next) => {
              setFilters(next)
              setPopupOpen(false)
            }}
            onReset={() => setFilters(null)}
            onClose={() => setPopupOpen(false)}
          />
        )}
        <div className="fin-tx-surface">
          {shown.length > 0 ? (
            <TransactionRows
              rows={shown}
              showAccount={filter === null}
              envelopes={envelopes}
              fancyIcons
              onTagged={(err, learned) => {
                setError(err)
                if (!err) {
                  if (learned) setNotice(`Zapamiętano regułę. Ta kategoria trafiła też do ${learned} innych transakcji tego odbiorcy.`)
                  void reload()
                }
              }}
            />
          ) : (
            <div className="fin-empty">
              {text.trim() !== '' ? 'Nic nie pasuje do wyszukiwania.' : onlyOwn ? 'Brak przelewów własnych w tym widoku.' : 'Brak transakcji na tym koncie.'}
            </div>
          )}
        </div>
      </>
    )
  } else if (section === 'budget') {
    body = <Budget onOpenSubscriptions={() => go('subscriptions')} />
  } else if (section === 'cash') {
    body = <Cash onChanged={() => void reload()} />
  } else if (section === 'subscriptions') {
    body = <Subscriptions />
  } else {
    body = <Investments onNotice={setNotice} onError={setError} />
  }

  return (
    <div className="app finance-app">
      <div className="titlebar-drag" data-tauri-drag-region />
      <aside className="sidebar sidebar-left">
        <h1>PFMX</h1>
        <nav>
          {MAIN.map((s) => (
            <button
              key={s.id}
              className={`view ${section === s.id ? 'active' : ''}`}
              onPointerDown={() => go(s.id)}
              onClick={() => go(s.id)}
            >
              <span className="view-label">{s.label}</span>
              <kbd className="hint" aria-hidden>
                <span>G</span>
                <span>{s.key.toUpperCase()}</span>
              </kbd>
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <button className="side-action" disabled={busy} onClick={() => void readExports()}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <path d="M20 11a8 8 0 0 0-14.9-3.5L4 10M4 13a8 8 0 0 0 14.9 3.5L20 14M4 4v6h6M20 20v-6h-6" />
            </svg>
            Odśwież dane
          </button>
          <button className="side-action" disabled={busy} onClick={() => void importCsv()}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />
            </svg>
            {busy ? 'Importuję…' : 'Importuj CSV'}
          </button>
        </div>
      </aside>
      <section className="list finance-main">
        <header className="list-header">
          <div className="fin-header-row">
            <div className="list-title">
              <h2>{head.title}</h2>
              <span className="muted">{head.subtitle}</span>
            </div>
          </div>
          {notice && <div className="notice">{notice}</div>}
          {error && <div className="error">{error}</div>}
        </header>
        <div className="fin-body">{body}</div>
      </section>
      <Alerts
        items={buildAlerts({ cushion: budget?.cushion ?? null, recurring, today: isoDate(new Date()), month: currentMonth(), now: Date.now() }).filter(
          (a) => !dismissed.includes(a.key)
        )}
        onDismiss={dismiss}
      />
    </div>
  )
}
