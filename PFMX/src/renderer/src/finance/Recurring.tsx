import { useEffect, useState } from 'react'
import type { EnvelopeInfo, Recurring, RecurringKind } from '@shared/types'
import { formatDate, formatMoney, moneyInput, parseMoney } from './format'

export const KIND_TEXT: Record<RecurringKind, string> = { other: 'Stała płatność', subscription: 'Subskrypcja' }

// kategorie zmieniają się rzadko; pobieramy je raz na sesję
let envelopesCache: Promise<EnvelopeInfo[]> | null = null
export function useEnvelopes(): EnvelopeInfo[] {
  const [list, setList] = useState<EnvelopeInfo[]>([])
  useEffect(() => {
    envelopesCache ??= window.api.financeEnvelopes()
    envelopesCache.then(setList).catch(() => setList([]))
  }, [])
  return list
}

/** Dodawanie i edycja płatności. Rodzaj wybiera się tu, więc płatność trafia na właściwą podstronę. */
export function RecurringForm({
  initial,
  defaultKind,
  onSaved,
  onCancel
}: {
  initial?: Recurring
  defaultKind: RecurringKind
  onSaved: () => void
  onCancel: () => void
}): JSX.Element {
  const [name, setName] = useState(initial?.name ?? '')
  const [kind, setKind] = useState<RecurringKind>(initial?.kind ?? defaultKind)
  const [amount, setAmount] = useState(initial ? moneyInput(initial.amount) : '')
  const [installments, setInstallments] = useState(initial?.installmentsLeft?.toString() ?? '')
  const [envelope, setEnvelope] = useState<number | null>(initial?.envelopeId ?? null)
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const envelopes = useEnvelopes()

  const submit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault()
    const grosze = parseMoney(amount)
    if (grosze === null || grosze <= 0) {
      setError('Podaj kwotę większą od zera')
      return
    }
    const trimmed = installments.trim()
    const count = trimmed === '' ? null : Number(trimmed)
    if (count !== null && (!Number.isInteger(count) || count < 0)) {
      setError('Liczba rat: liczba całkowita, 0 lub więcej. Zostaw puste, gdy płatność nie ma końca.')
      return
    }
    setSaving(true)
    setError(null)
    try {
      await window.api.financeSaveRecurring({ id: initial?.id ?? null, name, kind, amount: grosze, installmentsLeft: count, envelopeId: envelope })
      onSaved()
    } catch (err) {
      setError(String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <form className="fin-card fin-recurring-form" onSubmit={(e) => void submit(e)}>
      <label>
        Nazwa
        <input value={name} maxLength={60} onChange={(e) => setName(e.target.value)} placeholder="np. Rata za auto" required />
      </label>
      <label>
        Rodzaj
        <select value={kind} onChange={(e) => setKind(e.target.value as RecurringKind)}>
          <option value="other">{KIND_TEXT.other}</option>
          <option value="subscription">{KIND_TEXT.subscription}</option>
        </select>
      </label>
      <label>
        Kwota (zł)
        <input value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0,00" inputMode="decimal" required />
      </label>
      <label>
        Kategoria
        <select value={envelope ?? ''} onChange={(e) => setEnvelope(e.target.value === '' ? null : Number(e.target.value))}>
          <option value="">Opłaty i subskrypcje</option>
          {envelopes
            .filter((e) => e.key !== 'fixed' && e.key !== 'investments')
            .map((e) => (
              <option key={e.id} value={e.id}>
                {e.name}
              </option>
            ))}
        </select>
      </label>
      <label>
        Pozostałe raty
        <input value={installments} onChange={(e) => setInstallments(e.target.value)} placeholder="bez końca" inputMode="numeric" />
      </label>
      <div className="fin-form-actions">
        <button className="btn primary" type="submit" disabled={saving || !name.trim()}>
          {initial ? 'Zapisz zmiany' : 'Dodaj'}
        </button>
        <button className="btn" type="button" onClick={onCancel}>
          Anuluj
        </button>
      </div>
      {error && <div className="error fin-form-error">{error}</div>}
    </form>
  )
}

/** Jedna płatność na liście. Z opcją „opłacone” (ekran budżetu) pokazuje też odhaczanie. */
export function RecurringRow({
  item,
  showPaid,
  month,
  onChanged,
  onError,
  notice,
  leading,
  tag
}: {
  item: Recurring
  showPaid: boolean
  month: string
  onChanged: () => void
  onError: (text: string) => void
  /** dodatkowa linia pod wierszem, np. zmiana ceny abonamentu */
  notice?: React.ReactNode
  /** element po lewej stronie wiersza (np. logo usługi); puste na ekranie budżetu */
  leading?: React.ReactNode
  /** mała etykieta przy nazwie (np. cykl płatności) */
  tag?: React.ReactNode
}): JSX.Element {
  const [editing, setEditing] = useState(false)
  const envelopes = useEnvelopes()

  const togglePaid = (): void => {
    window.api
      .financeSetPaid(item.id, month, !item.paidThisMonth)
      .then(onChanged)
      .catch((e: unknown) => onError(String(e)))
  }

  const remove = (): void => {
    window.api
      .financeDeleteRecurring(item.id)
      .then(onChanged)
      .catch((e: unknown) => onError(String(e)))
  }

  if (editing) {
    return (
      <RecurringForm
        initial={item}
        defaultKind={item.kind}
        onSaved={() => {
          setEditing(false)
          onChanged()
        }}
        onCancel={() => setEditing(false)}
      />
    )
  }

  const installments = item.installmentsLeft === null ? 'bez końca' : `zostało rat: ${item.installmentsLeft}`
  return (
    <div className={`fin-recurring ${showPaid && item.paidThisMonth ? 'paid' : ''}`}>
    <div className="fin-row">
      {leading}
      {showPaid && (
        <input
          type="checkbox"
          className="fin-check"
          checked={item.paidThisMonth}
          onChange={togglePaid}
          aria-label={`Opłacone: ${item.name}`}
        />
      )}
      <div className="fin-main">
        <div className="fin-title">
          {item.name}
          {tag}
        </div>
        <div className="fin-sub">
          {installments}
          {envelopeName(item.envelopeId, envelopes)}
          {item.nextDueOn ? ` · następna ${formatDate(item.nextDueOn)}` : ''}
          {item.source === 'auto' ? ' · wykryta z historii' : ''}
          {item.paidAuto ? ' · opłacona automatycznie' : ''}
        </div>
      </div>
      <div className="fin-amount">{formatMoney(item.amount)}</div>
      <div className="fin-actions">
        <button className="fin-link" onClick={() => setEditing(true)}>
          Edytuj
        </button>
        <button className="fin-link muted-link" onClick={remove}>
          {item.source === 'auto' ? 'Ukryj' : 'Usuń'}
        </button>
      </div>
    </div>
    {notice}
    </div>
  )
}

/** „ · Transport” w opisie wiersza; dla domyślnej kategorii nic nie dopisujemy */
function envelopeName(id: number, envelopes: EnvelopeInfo[]): string {
  const found = envelopes.find((e) => e.id === id)
  return found && found.key !== 'fixed' ? ` · ${found.name}` : ''
}
