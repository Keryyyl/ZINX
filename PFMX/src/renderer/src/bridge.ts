import { invoke } from '@tauri-apps/api/core'
import type { Api } from '@shared/api'

/**
 * Most do rdzenia w Rust. Nazwa komendy to nazwa funkcji `finance_*` w commands.rs,
 * a argumenty idą pod nazwami parametrów (camelCase, Tauri sam zmienia je na snake_case).
 */
export const api: Api = {
  financeAccounts: () => invoke('finance_accounts_summary'),
  financeDailyFlow: (since) => invoke('finance_daily_flow', { since }),
  financeExportCsv: (accountId) => invoke('finance_export_csv', { accountId }),
  financeEnvelopes: () => invoke('finance_envelopes_list'),
  financeCashState: () => invoke('finance_cash_state'),
  financeCashAdd: (amount, note) => invoke('finance_cash_add', { amount, note }),
  financeCashSetBalance: (amount, note) => invoke('finance_cash_set_balance', { amount, note }),
  financeCashUpdate: (entry) => invoke('finance_cash_update', { id: entry.id, amount: entry.amount, note: entry.note }),
  financeCashDelete: (id) => invoke('finance_cash_delete', { id }),
  financeAddAccount: (input) => invoke('finance_account_add', { name: input.name, kind: input.kind, iban: input.iban }),
  financeTransactions: (accountId, limit) => invoke('finance_transactions_list', { accountId, limit }),
  financeImportCsv: () => invoke('finance_import_csv'),
  financeAutoImportNow: () => invoke('finance_auto_import_now'),
  financeBudget: (month) => invoke('finance_budget_view', { month }),
  financeSetPlanned: (envelopeId, month, amount) => invoke('finance_budget_set_planned', { envelopeId, month, amount }),
  financeSetCushionTarget: (amount) => invoke('finance_cushion_set_target', { amount }),
  financeTagTransaction: (transactionId, envelopeId) => invoke('finance_tag_transaction', { transactionId, envelopeId }),
  financeAiStatus: () => invoke('finance_ai_status'),
  financeAiRun: () => invoke('finance_ai_run'),
  financeIncomeSet: (transactionId, kind) => invoke('finance_income_set', { transactionId, kind }),
  financeIncomeSummary: (month) => invoke('finance_income_summary', { month }),
  financeSearchTransactions: (query) => invoke('finance_transactions_search', { query }),
  financeRecurring: (month) => invoke('finance_recurring_list', { month }),
  financeSaveRecurring: (input) => invoke('finance_recurring_save', { input }),
  financeDeleteRecurring: (id) => invoke('finance_recurring_delete', { id }),
  financeSetPaid: (id, month, paid) => invoke('finance_recurring_set_paid', { id, month, paid }),
  financeInvestImport: () => invoke('finance_invest_import'),
  financeInvestHoldings: () => invoke('finance_invest_holdings'),
  financeInvestHistory: () => invoke('finance_invest_history'),
  financeReceiptInfo: (transactionId) => invoke('finance_receipt_info', { transactionId }),
  financeReceiptAttach: (transactionId) => invoke('finance_receipt_attach', { transactionId }),
  financeReceiptOpen: (transactionId) => invoke('finance_receipt_open', { transactionId }),
  financeReceiptRemove: (transactionId) => invoke('finance_receipt_remove', { transactionId })
}
