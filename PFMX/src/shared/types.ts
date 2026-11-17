/** Finanse osobiste: rodzaje kont jak w rdzeniu (`db/finance.rs`) */
export type FinanceKind = 'bank' | 'savings' | 'buffer' | 'broker'

export interface FinanceAccount {
  id: number
  name: string
  kind: FinanceKind
  iban: string | null
  currency: string
}

/** Konto z saldem; kwoty w groszach */
export interface FinanceAccountSummary {
  id: number
  name: string
  kind: FinanceKind
  ibanMasked: string | null
  currency: string
  balance: number
  lastBookedOn: string | null
}

export interface FinanceTransaction {
  id: number
  accountId: number
  accountName: string
  bookedOn: string
  kind: string
  counterparty: string
  description: string
  /** ze znakiem, w groszach */
  amount: number
  currency: string
  balanceAfter: number
  /** Koperta przypisana do transakcji; null = nieotagowana */
  envelopeId: number | null
  /** dla wpływów: wypłata albo jednorazowy wpływ; null dla wydatków */
  incomeKind: IncomeKind | null
  /** przelew między własnymi kontami (w obie strony) */
  ownTransfer: boolean
}

export interface FinanceImportReport {
  added: number
  skipped: number
  perAccount: Record<string, number>
}

export interface BudgetEnvelope {
  id: number
  key: string
  name: string
  /** plan na wybrany miesiąc, grosze */
  planned: number
  /** niewykorzystane z poprzednich miesięcy */
  carryIn: number
  /** wydano w tym miesiącu (z otagowanych transakcji) */
  actual: number
  /** carryIn + planned - actual; może być ujemne */
  available: number
}

export interface BudgetCushion {
  target: number
  /** suma kont „buffer” w złotówkach */
  balance: number
  /** ponad cel; ma trafić na oszczędności albo inwestycje (program tylko informuje) */
  surplus: number
  reached: boolean
}

export interface BudgetView {
  /** RRRR-MM */
  month: string
  envelopes: BudgetEnvelope[]
  cushion: BudgetCushion
}

export type RecurringKind = 'subscription' | 'other'

export interface Recurring {
  id: number
  name: string
  kind: RecurringKind
  amount: number
  /** null = bez końca */
  installmentsLeft: number | null
  source: 'auto' | 'manual'
  ended: boolean
  paidThisMonth: boolean
  /** opłacone automatycznie z transakcji, nie ręcznie */
  paidAuto: boolean
  /** ostatnia dopasowana płatność, RRRR-MM-DD */
  lastPaidOn: string | null
  /** spodziewana następna płatność (ostatnia + miesiąc) */
  nextDueOn: string | null
  /** nowa kwota z banku przy abonamencie, która różni się od zapisanej */
  observedAmount: number | null
  /** moment dodania (ms od epoki) */
  createdAt: number
  /** kategoria płatności (koperta budżetu) */
  envelopeId: number
}

export interface RecurringInput {
  id: number | null
  name: string
  kind: RecurringKind
  amount: number
  installmentsLeft: number | null
  /** null = domyślna kategoria „Opłaty i subskrypcje” */
  envelopeId?: number | null
}

export interface AutoImportReport {
  imported: { fileName: string; added: number; skipped: number; deleted: boolean }[]
  errors: string[]
}

/** Przepływ netto jednego dnia, w groszach */
export interface DailyFlow {
  /** RRRR-MM-DD */
  date: string
  net: number
}

export interface EnvelopeInfo {
  id: number
  key: string
  name: string
}

export type CashSource = 'manual' | 'atm' | 'correction'

export interface CashEntry {
  id: number
  /** + wpływ, − wydatek; w groszach */
  amount: number
  note: string
  source: CashSource
  createdAt: number
}

export interface CashState {
  balance: number
  entries: CashEntry[]
}

export interface FinanceAiStatus {
  available: boolean
  message: string
}

export interface AiPassReport {
  /** ile transakcji dostało kategorię w tym przebiegu */
  tagged: number
  /** ile wydatków wciąż czeka na kategorię */
  remaining: number
}

/** Rodzaj wpływu: wypłata (powtarza się co miesiąc) albo jednorazowy */
export type IncomeKind = 'salary' | 'one_off'

export interface IncomeSummary {
  /** suma wypłat w miesiącu, w groszach */
  salary: number
  /** suma jednorazowych wpływów w miesiącu, w groszach */
  oneOff: number
}

/** Kryteria wyszukiwania transakcji; puste pole = bez ograniczenia */
export interface TransactionQuery {
  accountId: number | null
  /** id kategorii; 0 = „Bez Koperty” */
  category: number | null
  /** 'income' (wpływ) albo 'spending' (płatność) */
  kind: 'income' | 'spending' | null
  /** RRRR-MM-DD, włącznie */
  dateFrom: string | null
  dateTo: string | null
  /** w groszach, wartość bezwzględna */
  amountMin: number | null
  amountMax: number | null
}

/** Inwestycje z eksportu XTB. Kwoty w groszach (PLN), ilości i ceny w dziesięciotysięcznych. */
export interface InvestHolding {
  ticker: string
  instrument: string
  volume: number
  /** koszt zakupu tego, co zostało, w groszach */
  cost: number
}

export type InvestEventKind = 'deposit' | 'withdrawal' | 'interest' | 'interest_tax' | 'buy' | 'sell'

export interface InvestEvent {
  id: number
  kind: InvestEventKind
  ticker: string
  instrument: string
  volume: number
  /** cena w walucie instrumentu, w dziesięciotysięcznych */
  price: number
  /** PLN w groszach, ze znakiem z punktu widzenia konta */
  amount: number
  occurredAt: number
  /** dla kupna i sprzedaży: ile sztuk zostało po operacji */
  leftAfter: number | null
}

export interface InvestImportReport {
  added: number
  existing: number
  skipped: number
}

/** Paragon przypięty do transakcji (plik leży po stronie aplikacji) */
export interface ReceiptInfo {
  fileName: string
  addedAt: number
}
