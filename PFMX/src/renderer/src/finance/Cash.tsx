import { useCallback, useEffect, useState } from 'react'
import type { CashEntry, CashState } from '@shared/types'
import { formatDate, formatMoney, isoDate, moneyInput, parseMoney } from './format'

const SOURCE_TEXT: Record<CashEntry['source'], string> = {
  manual: 'ręcznie',
  atm: 'z bankomatu',
  correction: 'korekta stanu'
}

/** Ikona wpisu: banknot dla ręcznych, bankomat dla wypłat, strzałki dla korekty */
function EntryIcon({ source }: { source: CashEntry['source'] }): JSX.Element {
  const common = { width: 16, height: 16, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.6, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const, 'aria-hidden': true }
  if (source === 'atm') {
    return (
      <svg {...common}>
        <rect x="3" y="4" width="18" height="16" rx="2.5" />
        <path d="M7 9h10M7 13h6" />
        <path d="M14 17h3" />
      </svg>
    )
  }
  if (source === 'correction') {
    return (
      <svg {...common}>
        <path d="M7 8h11l-3-3M17 16H6l3 3" />
      </svg>
    )
  }
  return (
    <svg {...common}>
      <rect x="2.5" y="6.5" width="19" height="11" rx="2" />
      <circle cx="12" cy="12" r="2.4" />
      <path d="M6 10v4M18 10v4" />
    </svg>
  )
}

/** Gotówka: stan to suma wpisów; ręczne zmiany i korekty tworzą wpisy ze znakiem i opisem. */
export default function Cash({ onChanged }: { onChanged?: () => void }): JSX.Element {
  const [state, setState] = useState<CashState | null>(null)
  const [sign, setSign] = useState<1 | -1>(-1)
  const [amount, setAmount] = useState('')
  const [note, setNote] = useState('')
  const [target, setTarget] = useState('')
  const [targetNote, setTargetNote] = useState('')
  const [editing, setEditing] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [info, setInfo] = useState<string | null>(null)

  const reload = useCallback(async (): Promise<void> => {
    setState(await window.api.financeCashState())
  }, [])

  useEffect(() => {
    reload().catch((e: unknown) => setError(String(e)))
  }, [reload])

  const run = (action: () => Promise<unknown>, done?: string): void => {
    setError(null)
    setInfo(null)
    action()
      .then(() => {
        if (done) setInfo(done)
        return reload()
      })
      .then(() => onChanged?.())
      .catch((e: unknown) => setError(String(e)))
  }

  const add = (e: React.FormEvent): void => {
    e.preventDefault()
    const grosze = parseMoney(amount)
    if (grosze === null || grosze <= 0) {
      setError('Podaj kwotę większą od zera')
      return
    }
    run(async () => {
      await window.api.financeCashAdd(sign * grosze, note)
      setAmount('')
      setNote('')
    }, 'Zapisano wpis.')
  }

  const setBalance = (e: React.FormEvent): void => {
    e.preventDefault()
    const grosze = parseMoney(target)
    if (grosze === null || grosze < 0) {
      setError('Podaj stan gotówki (0 zł lub więcej)')
      return
    }
    run(async () => {
      const diff = await window.api.financeCashSetBalance(grosze, targetNote)
      setTarget('')
      setTargetNote('')
      setInfo(diff === null ? 'Stan już się zgadzał.' : `Dodano korektę ${formatMoney(diff, 'PLN', true)}.`)
    })
  }

  if (!state) return <div className="fin-empty">Ładowanie…</div>

  return (
    <div className="vault">
      <section className="vault-hero" aria-label="Stan gotówki">
        <div className="vault-hero-label">Gotówka w portfelu</div>
        <div className="vault-hero-amount">{formatMoney(state.balance)}</div>
        <div className="vault-hero-hint">Wypłaty z bankomatu dodają się same. Każdą zmianę możesz opisać i poprawić.</div>
      </section>

      <div className="vault-grid">
        <form className="vault-tile" onSubmit={setBalance}>
          <div className="vault-tile-title">Szybka korekta</div>
          <div className="vault-tile-sub">Wpisz, ile naprawdę masz w portfelu.</div>
          <div className="vault-row-inline">
            <input className="vault-input vault-input-amount" value={target} onChange={(e) => setTarget(e.target.value)} placeholder="Ile jest naprawdę (zł)" inputMode="decimal" aria-label="Rzeczywisty stan" />
          </div>
          <div className="vault-row-inline">
            <input className="vault-input" value={targetNote} onChange={(e) => setTargetNote(e.target.value)} placeholder="Opis korekty (opcjonalnie)" maxLength={80} aria-label="Opis korekty" />
            <button className="vault-btn" type="submit">
              Ustaw stan
            </button>
          </div>
        </form>

        <form className="vault-tile" onSubmit={add}>
          <div className="vault-tile-title">Nowy wpis</div>
          <div className="vault-tile-sub">Wydatek z minusem, wpłata z plusem.</div>
          <div className="vault-row-inline">
            <div className="vault-sign" role="group" aria-label="Rodzaj zmiany">
              <button type="button" className={sign === 1 ? 'active plus' : ''} onClick={() => setSign(1)} aria-pressed={sign === 1}>
                +
              </button>
              <button type="button" className={sign === -1 ? 'active minus' : ''} onClick={() => setSign(-1)} aria-pressed={sign === -1}>
                −
              </button>
            </div>
            <input className="vault-input vault-input-amount" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="Kwota (zł)" inputMode="decimal" aria-label="Kwota" />
          </div>
          <div className="vault-row-inline">
            <input className="vault-input" value={note} onChange={(e) => setNote(e.target.value)} placeholder="Opis, np. napiwek w barze" maxLength={80} aria-label="Opis" />
            <button className="vault-btn" type="submit">
              Dodaj wpis
            </button>
          </div>
        </form>
      </div>

      {error && <div className="error">{error}</div>}
      {info && <div className="notice">{info}</div>}

      <section className="vault-history">
        <header className="vault-history-head">Historia</header>
        {state.entries.length === 0 && <div className="fin-empty">Brak wpisów. Dodaj pierwszy albo ustaw stan.</div>}
        {state.entries.map((entry) =>
          editing === entry.id && entry.source !== 'atm' ? (
            <EditRow
              key={entry.id}
              entry={entry}
              onCancel={() => setEditing(null)}
              onSave={(amountGrosze, text) => {
                setEditing(null)
                run(() => window.api.financeCashUpdate({ id: entry.id, amount: amountGrosze, note: text }))
              }}
            />
          ) : (
            <div className="vault-entry" key={entry.id}>
              <div className={`vault-entry-icon vault-entry-${entry.source}`}>
                <EntryIcon source={entry.source} />
              </div>
              <div className="vault-entry-main">
                <div className="vault-entry-title">{entry.note || SOURCE_TEXT[entry.source]}</div>
                <div className="vault-entry-sub">
                  {formatDate(isoDate(new Date(entry.createdAt)))} · {SOURCE_TEXT[entry.source]}
                  {entry.source === 'atm' ? ' · tylko do odczytu' : ''}
                </div>
              </div>
              <div className={`vault-entry-amount ${entry.amount > 0 ? 'pos' : 'neg'}`}>{formatMoney(entry.amount, 'PLN', true)}</div>
              {entry.source !== 'atm' && (
                <div className="vault-entry-actions">
                  <button className="fin-link" onClick={() => setEditing(entry.id)}>
                    Edytuj
                  </button>
                  <button className="fin-link muted-link" onClick={() => run(() => window.api.financeCashDelete(entry.id), 'Usunięto wpis.')}>
                    Usuń
                  </button>
                </div>
              )}
            </div>
          )
        )}
      </section>
    </div>
  )
}

function EditRow({
  entry,
  onSave,
  onCancel
}: {
  entry: CashEntry
  onSave: (amount: number, note: string) => void
  onCancel: () => void
}): JSX.Element {
  const [amount, setAmount] = useState(moneyInput(Math.abs(entry.amount)))
  const [sign, setSign] = useState<1 | -1>(entry.amount > 0 ? 1 : -1)
  const [note, setNote] = useState(entry.note)

  const save = (): void => {
    const grosze = parseMoney(amount)
    if (grosze === null || grosze <= 0) return
    onSave(sign * grosze, note)
  }

  return (
    <div className="vault-edit">
      <div className="vault-sign">
        <button type="button" className={sign === 1 ? 'active plus' : ''} onClick={() => setSign(1)}>
          +
        </button>
        <button type="button" className={sign === -1 ? 'active minus' : ''} onClick={() => setSign(-1)}>
          −
        </button>
      </div>
      <input className="vault-input vault-input-amount" value={amount} onChange={(e) => setAmount(e.target.value)} aria-label="Kwota" inputMode="decimal" />
      <input className="vault-input" value={note} onChange={(e) => setNote(e.target.value)} aria-label="Opis" maxLength={80} />
      <div className="vault-entry-actions">
        <button className="fin-link" onClick={save}>
          Zapisz
        </button>
        <button className="fin-link muted-link" onClick={onCancel}>
          Anuluj
        </button>
      </div>
    </div>
  )
}
