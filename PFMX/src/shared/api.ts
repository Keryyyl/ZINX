import type {
  AutoImportReport,
  AiPassReport,
  BudgetView,
  FinanceAiStatus,
  IncomeKind,
  IncomeSummary,
  TransactionQuery,
  CashEntry,
  CashState,
  EnvelopeInfo,
  DailyFlow,
  FinanceAccount,
  FinanceAccountSummary,
  FinanceImportReport,
  FinanceKind,
  FinanceTransaction,
  Recurring,
  RecurringInput,
  InvestEvent,
  InvestHolding,
  InvestImportReport,
  ReceiptInfo,
} from './types'

/** Kontrakt między interfejsem a rdzeniem (Rust). Komendy są w src-tauri/src/commands.rs. */
export interface Api {
  financeAccounts(): Promise<FinanceAccountSummary[]>
  /** Przepływ netto dzień po dniu od podanej daty (RRRR-MM-DD) */
  financeDailyFlow(since: string): Promise<DailyFlow[]>
  /** Zapisuje transakcje w układzie eksportu banku; zwraca ścieżkę pliku albo null przy anulowaniu */
  financeExportCsv(accountId: number | null): Promise<string | null>
  financeEnvelopes(): Promise<EnvelopeInfo[]>
  financeCashState(): Promise<CashState>
  /** wpis ręczny: + dodaje, − odejmuje */
  financeCashAdd(amount: number, note: string): Promise<number>
  /** ustawia stan na wpisaną kwotę; zwraca kwotę korekty albo null, gdy stan się zgadzał */
  financeCashSetBalance(amount: number, note: string): Promise<number | null>
  financeCashUpdate(entry: Pick<CashEntry, 'id' | 'amount' | 'note'>): Promise<void>
  financeCashDelete(id: number): Promise<void>
  financeAddAccount(input: { name: string; kind: FinanceKind; iban: string | null }): Promise<FinanceAccount>
  financeTransactions(accountId: number | null, limit?: number): Promise<FinanceTransaction[]>
  /** null = użytkownik zamknął okno wyboru pliku */
  financeImportCsv(): Promise<FinanceImportReport | null>
  /** Czyta eksporty z folderu Millennium (ten sam odczyt co w tle) */
  financeAutoImportNow(): Promise<AutoImportReport>
  financeBudget(month: string): Promise<BudgetView>
  financeSetPlanned(envelopeId: number, month: string, amount: number): Promise<void>
  financeSetCushionTarget(amount: number): Promise<void>
  /** zwraca, ile innych transakcji tego odbiorcy dostało kategorię przez zapamiętaną regułę */
  financeTagTransaction(transactionId: number, envelopeId: number | null): Promise<number>
  financeAiStatus(): Promise<FinanceAiStatus>
  financeIncomeSet(transactionId: number, kind: IncomeKind): Promise<void>
  financeIncomeSummary(month: string): Promise<IncomeSummary>
  financeSearchTransactions(query: TransactionQuery): Promise<FinanceTransaction[]>
  /** kategoryzuje do 25 wydatków lokalnym modelem */
  financeAiRun(): Promise<AiPassReport>
  financeRecurring(month: string): Promise<Recurring[]>
  financeSaveRecurring(input: RecurringInput): Promise<number>
  financeDeleteRecurring(id: number): Promise<void>
  financeSetPaid(id: number, month: string, paid: boolean): Promise<void>
  /** null = użytkownik zamknął okno wyboru pliku */
  financeInvestImport(): Promise<InvestImportReport | null>
  financeInvestHoldings(): Promise<InvestHolding[]>
  financeInvestHistory(): Promise<InvestEvent[]>
  /** paragon przypięty do transakcji, albo null */
  financeReceiptInfo(transactionId: number): Promise<ReceiptInfo | null>
  /** użytkownik wybiera zdjęcie w oknie systemowym; null = anulowano */
  financeReceiptAttach(transactionId: number): Promise<ReceiptInfo | null>
  /** otwiera paragon w domyślnej aplikacji systemu */
  financeReceiptOpen(transactionId: number): Promise<void>
}
