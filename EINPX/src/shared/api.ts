/** Kontrakt między interfejsem a rdzeniem Rust. Komendy po stronie Rust mają nazwy einpx_*. */

export interface MailItem {
  id: number
  /** nadawca, tak jak zapisał go ZIM */
  from: string
  subject: string
  /** konto, na które przyszła wiadomość */
  account: string
  /** milisekundy od epoki */
  receivedAt: number
}

export interface MailSummary {
  unread: number
  latest: MailItem[]
}

export interface FinanceSummary {
  /** saldo konta „Main”, grosze */
  main: number
  /** konta oszczędnościowe, grosze */
  savingsAccounts: number
  /** gotówka, grosze */
  cash: number
}

export type ItemKind = 'task' | 'event'

export interface PlanItem {
  id: number
  kind: ItemKind
  title: string
  description: string
  /** GG:MM albo null */
  startTime: string | null
  endTime: string | null
  done: boolean
}

export interface HabitLine {
  id: number
  name: string
  /** ile razy w miesiącu */
  goal: number
  doneThisMonth: number
  doneToday: boolean
}

/** Wynik jednej sekcji: dane albo komunikat, żeby błąd jednej aplikacji nie blokował reszty */
export interface Section<T> {
  data: T | null
  error: string | null
}

export interface Snapshot {
  mail: Section<MailSummary>
  finance: Section<FinanceSummary>
  habits: Section<HabitLine[]>
  /** milisekundy od epoki */
  fetchedAt: number
}

export interface Battery {
  percent: number
  /** np. „charged”, „charging”, „discharging” */
  state: string
  onAcPower: boolean
}

export interface Disk {
  name: string
  mount: string
  /** zajętość w procentach */
  usedPercent: number
  /** wolne miejsce, bajty */
  freeBytes: number
}

export interface SystemInfo {
  /** null = komputer bez baterii */
  battery: Battery | null
  disks: Disk[]
}

export interface Note {
  id: number
  text: string
  /** milisekundy od epoki */
  createdAt: number
}

/** Aplikacje ZINX, które można otworzyć z tagów */
export type AppName = 'ZIM' | 'PFMX' | 'CTHX' | 'EINPX'

export interface NewItem {
  kind: ItemKind
  title: string
  description: string
  /** RRRR-MM-DD */
  date: string
  startTime: string | null
  endTime: string | null
}

export interface Api {
  /** today = RRRR-MM-DD, month = RRRR-MM (lokalnie, z przeglądarki) */
  snapshot(today: string, month: string): Promise<Snapshot>
  /** Bateria i dyski */
  system(): Promise<SystemInfo>
  /** Plan jednego dnia (RRRR-MM-DD) */
  dayPlan(date: string): Promise<PlanItem[]>
  addItem(input: NewItem): Promise<void>
  /** Szybkie notatki (własna baza EINPX) */
  notesList(): Promise<Note[]>
  noteAdd(text: string): Promise<void>
  noteDelete(id: number): Promise<void>
  /** Otwiera aplikację ZINX (tylko z listy AppName) */
  openApp(app: AppName): Promise<void>
  setItemDone(itemId: number, date: string, done: boolean): Promise<void>
  setHabitDay(habitId: number, date: string, done: boolean): Promise<void>
}
