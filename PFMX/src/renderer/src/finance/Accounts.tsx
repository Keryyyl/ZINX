import { useEffect, useState } from 'react'
import type { FinanceAccountSummary, FinanceKind } from '@shared/types'
import { formatDate, formatMoney, formatPercent } from './format'

export const KIND_LABEL: Record<FinanceKind, string> = {
  bank: 'Konto zwykłe',
  savings: 'Oszczędnościowe',
  buffer: 'Poduszka finansowa',
  broker: 'Inwestycyjne'
}

/** Kolor akcentu karty według rodzaju konta (ten sam, co kropki na liście) */
const ACCENT: Record<FinanceKind, string> = {
  bank: '#6f9cf5',
  savings: '#a98bf7',
  buffer: '#4fd1c0',
  broker: '#8a8a93'
}

/** Udział kont w sumie salda: tylko dodatnie konta w złotówkach, tak jak w przeglądzie */
function shares(accounts: FinanceAccountSummary[]): { account: FinanceAccountSummary; percent: number }[] {
  const positive = accounts.filter((a) => a.currency === 'PLN' && a.kind !== 'broker' && a.balance > 0)
  const total = positive.reduce((sum, a) => sum + a.balance, 0)
  if (total <= 0) return []
  return positive.map((account) => ({ account, percent: (account.balance / total) * 100 }))
}

/** Scalony pasek proporcji kapitału: jeden segment na konto */
function CapitalBar({ accounts }: { accounts: FinanceAccountSummary[] }): JSX.Element {
  const parts = shares(accounts)
  if (parts.length === 0) {
    return <div className="fin-capital-empty">Brak dodatnich sald w złotówkach, nie ma czego porównać.</div>
  }
  return (
    <div className="fin-capital">
      <div className="fin-capital-bar" role="img" aria-label="Udział kont w kapitale">
        {parts.map(({ account, percent }) => (
          <div
            key={account.id}
            className="fin-capital-seg"
            style={{ width: `${percent}%`, ['--seg' as string]: ACCENT[account.kind] }}
            title={`${account.name}: ${formatPercent(percent)}`}
          />
        ))}
      </div>
      <div className="fin-capital-legend">
        {parts.map(({ account, percent }) => (
          <span key={account.id}>
            <i className="fin-capital-dot" style={{ ['--seg' as string]: ACCENT[account.kind] }} aria-hidden />
            {account.name} · {formatPercent(percent)}
          </span>
        ))}
      </div>
    </div>
  )
}

function AccountCard({ account }: { account: FinanceAccountSummary }): JSX.Element {
  return (
    <article className="fin-account-card" style={{ ['--acc' as string]: ACCENT[account.kind] }}>
      <header className="fin-account-top">
        <span className="fin-account-kind">
          <i className="fin-dot" style={{ ['--fin-dot' as string]: ACCENT[account.kind] }} aria-hidden />
          {KIND_LABEL[account.kind]}
        </span>
        {account.ibanMasked && <span className="fin-account-mask">{account.ibanMasked}</span>}
      </header>
      <div className="fin-account-name">{account.name}</div>
      <div className="fin-account-balance">{formatMoney(account.balance, account.currency)}</div>
      <div className="fin-account-foot">
        {account.lastBookedOn ? `ostatnia transakcja ${formatDate(account.lastBookedOn)}` : 'Brak transakcji'}
      </div>
    </article>
  )
}

function AddAccountPopup({
  onClose,
  onAdded,
  onNotice,
  onError
}: {
  onClose: () => void
  onAdded: () => void
  onNotice: (text: string) => void
  onError: (text: string) => void
}): JSX.Element {
  const [name, setName] = useState('')
  const [kind, setKind] = useState<FinanceKind>('bank')
  const [iban, setIban] = useState('')

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const submit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault()
    const finalName = name.trim()
    try {
      await window.api.financeAddAccount({ name: finalName, kind, iban: iban.trim() || null })
      onError('')
      onNotice(`Dodano konto „${finalName}”.`)
      onAdded()
      onClose()
    } catch (err) {
      onNotice('')
      onError(String(err))
    }
  }

  return (
    <div className="fin-popup-overlay" onClick={onClose}>
      <form className="fin-popup fin-add-popup" role="dialog" aria-label="Nowe konto" onClick={(e) => e.stopPropagation()} onSubmit={(e) => void submit(e)}>
        <header className="fin-popup-head">
          <h3>Nowe konto</h3>
          <button type="button" className="fin-popup-close" onClick={onClose} aria-label="Zamknij">
            ×
          </button>
        </header>
        <div className="fin-popup-body">
          <label>
            Nazwa
            <input value={name} maxLength={40} onChange={(e) => setName(e.target.value)} placeholder="np. Zwykłe" required autoFocus />
          </label>
          <label>
            Rodzaj
            <select value={kind} onChange={(e) => setKind(e.target.value as FinanceKind)}>
              {(Object.keys(KIND_LABEL) as FinanceKind[]).map((k) => (
                <option key={k} value={k}>
                  {KIND_LABEL[k]}
                </option>
              ))}
            </select>
          </label>
          <label>
            Numer rachunku (opcjonalnie)
            <input value={iban} onChange={(e) => setIban(e.target.value)} placeholder="PL…" />
          </label>
        </div>
        <footer className="fin-popup-foot">
          <button type="button" className="fin-ghost fin-ghost-inline" onClick={onClose}>
            Anuluj
          </button>
          <button type="submit" className="fin-ghost fin-ghost-inline is-on" disabled={!name.trim()}>
            Dodaj konto
          </button>
        </footer>
      </form>
    </div>
  )
}

export function AccountsSection({
  accounts,
  onAdded,
  onNotice,
  onError
}: {
  accounts: FinanceAccountSummary[]
  onAdded: () => void
  onNotice: (text: string) => void
  onError: (text: string) => void
}): JSX.Element {
  const [adding, setAdding] = useState(false)
  const close = (): void => setAdding(false)

  return (
    <div className="fin-accounts">
      <div className="fin-accounts-bar">
        <span className="fin-accounts-count">
          {accounts.length === 0 ? 'Brak kont' : `Kont: ${accounts.length}`}
        </span>
        <button className="fin-add-btn" onClick={() => setAdding(true)}>
          + Nowe konto
        </button>
      </div>

      {accounts.length > 0 && <CapitalBar accounts={accounts} />}

      {accounts.length > 0 ? (
        <div className="fin-account-grid">
          {accounts.map((a) => (
            <AccountCard key={a.id} account={a} />
          ))}
        </div>
      ) : (
        <div className="fin-card fin-empty">Dodaj pierwsze konto przyciskiem u góry.</div>
      )}

      {adding && <AddAccountPopup onClose={close} onAdded={onAdded} onNotice={onNotice} onError={onError} />}
    </div>
  )
}
