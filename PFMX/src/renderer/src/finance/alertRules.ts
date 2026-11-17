import type { BudgetCushion, Recurring } from '@shared/types'
import { formatDate, formatMoney } from './format'

export interface Alert {
  /** stały klucz: po zamknięciu nie wraca, dopóki nie zmieni się sprawa */
  key: string
  title: string
  text: string
}

/** Dni od `from` do `to` (RRRR-MM-DD); ujemne, gdy `to` jest wcześniej */
function daysBetween(from: string, to: string): number {
  const ms = Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)
  return Math.round(ms / 86_400_000)
}

/** Sprawy do pokazania w popupie: nadwyżka poduszki, zmiana ceny, zbliżająca się płatność, nowo wykryta płatność */
export function buildAlerts(input: { cushion: BudgetCushion | null; recurring: Recurring[]; today: string; month: string; now: number }): Alert[] {
  const out: Alert[] = []
  const { cushion, recurring, today, month, now } = input

  if (cushion && cushion.surplus > 0) {
    out.push({
      key: `surplus-${month}-${cushion.surplus}`,
      title: 'Nadwyżka poduszki',
      text: `Poduszka przekroczyła cel o ${formatMoney(cushion.surplus)}. Przenieś ją na oszczędności albo inwestycje.`
    })
  }

  for (const r of recurring) {
    if (r.kind === 'subscription' && r.observedAmount !== null) {
      out.push({
        key: `price-${r.id}-${r.observedAmount}`,
        title: 'Zmiana ceny',
        text: `${r.name}: ${formatMoney(r.amount)} → ${formatMoney(r.observedAmount)}. Zaktualizuj kwotę na liście subskrypcji.`
      })
    }
    if (!r.ended && !r.paidThisMonth && r.nextDueOn) {
      const days = daysBetween(today, r.nextDueOn)
      if (days >= 0 && days <= 3) {
        out.push({
          key: `due-${r.id}-${r.nextDueOn}`,
          title: 'Zbliża się płatność',
          text: `${r.name}: ${formatMoney(r.amount)} do ${formatDate(r.nextDueOn)}.`
        })
      }
    }
    if (r.source === 'auto' && now - r.createdAt < 24 * 60 * 60 * 1000) {
      out.push({
        key: `new-${r.id}`,
        title: 'Wykryto płatność cykliczną',
        text: `${r.name} · ${formatMoney(r.amount)}. Jeśli to pomyłka, ukryj ją na liście.`
      })
    }
  }
  return out
}
