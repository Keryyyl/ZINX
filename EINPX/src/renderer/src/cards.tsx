import { useEffect, useState, type ReactNode } from 'react'
import type { AppName, FinanceSummary, HabitLine, MailSummary, Section, SystemInfo } from '@shared/api'
import { plural, zl } from './format'
import { fetchWeather, type Weather } from './weather'

/** Tag aplikacji: klik otwiera ją; przy błędzie pokazuje krótko komunikat */
export function AppTag({ app }: { app: AppName }): JSX.Element {
  const [failed, setFailed] = useState(false)

  const open = (): void => {
    window.api
      .openApp(app)
      .then(() => setFailed(false))
      .catch(() => {
        setFailed(true)
        window.setTimeout(() => setFailed(false), 2500)
      })
  }

  return (
    <button className={`einpx-tag einpx-tag-link ${failed ? 'is-error' : ''}`} onClick={open} title={`Otwórz ${app}`}>
      {failed ? `Brak ${app}` : app}
    </button>
  )
}

/** Obudowa karty z tytułem, etykietą źródła i opcjonalnym przyciskiem w nagłówku */
export function Card({
  title,
  subtitle,
  app,
  children,
  className,
  action
}: {
  title: string
  subtitle?: string
  /** jeśli podana, tag jest klikalny i otwiera aplikację */
  app?: AppName
  children: ReactNode
  className?: string
  action?: ReactNode
}): JSX.Element {
  return (
    <section className={`einpx-card ${className ?? ''}`}>
      <div className="einpx-card-head">
        <h2>{title}</h2>
        <div className="einpx-card-head-right">
          {app ? <AppTag app={app} /> : subtitle && <span className="einpx-tag">{subtitle}</span>}
          {action}
        </div>
      </div>
      {children}
    </section>
  )
}

/** Komunikat zamiast danych, gdy sekcja się nie wczytała (np. brak bazy danej aplikacji) */
export function SectionError<T>({ section }: { section: Section<T> | undefined }): JSX.Element | null {
  if (!section) return <p className="muted">Ładowanie…</p>
  if (section.error) return <p className="einpx-section-error">{section.error}</p>
  return null
}

/** Godzina, pogoda i temperatura w jednym wierszu */
export function ClockWeatherCard(): JSX.Element {
  const [now, setNow] = useState(() => new Date())
  const [weather, setWeather] = useState<Weather | null>(null)
  const [weatherError, setWeatherError] = useState<string | null>(null)

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 30_000)
    return () => window.clearInterval(timer)
  }, [])

  useEffect(() => {
    let cancelled = false
    const load = (): void => {
      fetchWeather()
        .then((w) => {
          if (!cancelled) {
            setWeather(w)
            setWeatherError(null)
          }
        })
        .catch((e: unknown) => {
          if (!cancelled) setWeatherError(String(e))
        })
    }
    load()
    const timer = window.setInterval(load, 10 * 60_000)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [])

  return (
    <section className="einpx-card einpx-clock">
      <div className="einpx-clock-time">
        <span className="einpx-clock-hour">{now.toLocaleTimeString('pl-PL', { hour: '2-digit', minute: '2-digit' })}</span>
        <span className="muted small">{now.toLocaleDateString('pl-PL', { weekday: 'long', day: 'numeric', month: 'long' })}</span>
      </div>
      <div className="einpx-clock-weather">
        {weather ? (
          <>
            <span className="einpx-clock-temp">{weather.temperature}°C</span>
            <span className="einpx-clock-range" title="Przewidywany zakres dnia">
              {weather.tempMin}°–{weather.tempMax}°
            </span>
            <span className="small">
              {weather.description}
              <span className="muted"> · {weather.place}</span>
            </span>
          </>
        ) : (
          <span className="muted small">{weatherError ? 'Brak pogody' : 'Pogoda…'}</span>
        )}
      </div>
    </section>
  )
}

export function MailCard({ section }: { section: Section<MailSummary> | undefined }): JSX.Element {
  const mail = section?.data
  return (
    <Card title="Poczta" app="ZIM">
      <SectionError section={section} />
      {mail && (
        <>
          <div className="einpx-unread">
            <span className="einpx-unread-count">{mail.unread}</span>
            <span className="muted">{plural(mail.unread, 'nieodczytana', 'nieodczytane', 'nieodczytanych')}</span>
          </div>
          <ul className="einpx-list">
            {mail.latest.length === 0 && <li className="muted">Brak wiadomości w skrzynce odbiorczej.</li>}
            {mail.latest.map((m) => (
              <li key={m.id} className="einpx-mail">
                <div className="einpx-mail-top">
                  <span className="einpx-mail-from">{m.from}</span>
                  <span className="muted small einpx-mail-date">
                    {new Date(m.receivedAt).toLocaleString('pl-PL', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })}
                  </span>
                </div>
                <div className="einpx-mail-subject">{m.subject || '(bez tematu)'}</div>
                <div className="muted small einpx-mail-account">na {m.account}</div>
              </li>
            ))}
          </ul>
        </>
      )}
    </Card>
  )
}

/** Nagłówek połowy karty (bez obudowy), takie same klasy jak w Card */
function PanelHead({ title, app }: { title: string; app: AppName }): JSX.Element {
  return (
    <div className="einpx-card-head">
      <h2>{title}</h2>
      <div className="einpx-card-head-right">
        <AppTag app={app} />
      </div>
    </div>
  )
}

function FinanceBody({ section }: { section: Section<FinanceSummary> | undefined }): JSX.Element {
  const fin = section?.data
  return (
    <>
      <SectionError section={section} />
      {fin && (
        <div className="einpx-fin-list">
          <FinanceRow label="Konto Main" amount={fin.main} />
          <FinanceRow label="Oszczędnościowe" amount={fin.savingsAccounts} />
          <FinanceRow label="Gotówka" amount={fin.cash} />
        </div>
      )}
    </>
  )
}

/* Wiersz tylko z efektem hover (bez akcji po kliknięciu) */
function FinanceRow({ label, amount }: { label: string; amount: number }): JSX.Element {
  return (
    <div className="einpx-fin-row">
      <span className="einpx-fin-label">{label}</span>
      <span className="einpx-fin-amount">{zl(amount)}</span>
      <svg className="einpx-fin-go" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
        <path d="M7 17L17 7M9 7h8v8" />
      </svg>
    </div>
  )
}

function HabitsBody({ section, onToggle }: { section: Section<HabitLine[]> | undefined; onToggle: (habit: HabitLine, done: boolean) => Promise<void> }): JSX.Element {
  const habits = section?.data
  return (
    <>
      <SectionError section={section} />
      {habits && (
        <ul className="einpx-list">
          {habits.length === 0 && <li className="muted">Brak nawyków. Dodaj je w CTHX.</li>}
          {habits.map((h) => {
            const pct = h.goal > 0 ? Math.min(100, Math.round((h.doneThisMonth / h.goal) * 100)) : 0
            // cały wiersz przełącza wykonanie; kliknięcie w checkbox obsługuje sam checkbox
            return (
              <li
                key={h.id}
                className={`einpx-row einpx-habit ${h.doneToday ? 'is-done' : ''}`}
                onClick={(e) => {
                  if ((e.target as HTMLElement).tagName !== 'INPUT') void onToggle(h, !h.doneToday)
                }}
              >
                <input type="checkbox" checked={h.doneToday} onChange={(e) => void onToggle(h, e.target.checked)} aria-label={`Dzisiaj: ${h.name}`} />
                <span className="einpx-row-title">{h.name}</span>
                <span className="einpx-habit-count">
                  {h.doneThisMonth}/{h.goal}
                </span>
                <span className="einpx-habit-bar" title={`${pct}% celu w tym miesiącu`} aria-hidden>
                  <span style={{ width: `${pct}%` }} />
                </span>
              </li>
            )
          })}
        </ul>
      )}
    </>
  )
}

/** Finanse i nawyki w jednej karcie, dwie równe połowy */
export function FinanceHabitsCard({
  finance,
  habits,
  onToggle
}: {
  finance: Section<FinanceSummary> | undefined
  habits: Section<HabitLine[]> | undefined
  onToggle: (habit: HabitLine, done: boolean) => Promise<void>
}): JSX.Element {
  return (
    <section className="einpx-card einpx-pair">
      <div className="einpx-pair-col">
        <PanelHead title="Finanse" app="PFMX" />
        <FinanceBody section={finance} />
      </div>
      <div className="einpx-pair-divider" aria-hidden />
      <div className="einpx-pair-col">
        <PanelHead title="Nawyki" app="CTHX" />
        <HabitsBody section={habits} onToggle={onToggle} />
      </div>
    </section>
  )
}

/** Bateria i zajętość dysków w jednym wierszu */
export function SystemCard(): JSX.Element {
  const [info, setInfo] = useState<SystemInfo | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    const load = (): void => {
      window.api
        .system()
        .then((s) => {
          if (!cancelled) {
            setInfo(s)
            setError(null)
          }
        })
        .catch((e: unknown) => {
          if (!cancelled) setError(String(e))
        })
    }
    load()
    const timer = window.setInterval(load, 60_000)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [])

  return (
    <section className="einpx-card einpx-system">
      {error && <p className="einpx-section-error">{error}</p>}
      {!info && !error && <p className="muted small">Ładowanie…</p>}
      {info && (
        <div className="einpx-sys-grid">
          {info.battery && (
            <div className="einpx-sys-tile">
              <div className="einpx-sys-head">
                <IconBattery />
                <span className="einpx-sys-label">Bateria</span>
              </div>
              <div className="einpx-sys-value">{info.battery.percent}%</div>
              <div className="einpx-sys-sub">{batteryState(info.battery.state, info.battery.onAcPower)}</div>
              <Meter percent={info.battery.percent} />
            </div>
          )}
          {info.disks.map((d) => (
            <div key={d.mount} className="einpx-sys-tile" title={`${d.mount}, wolne ${gb(d.freeBytes)}`}>
              <div className="einpx-sys-head">
                <IconDisk />
                <span className="einpx-sys-label">{d.name}</span>
              </div>
              <div className={`einpx-sys-value ${d.usedPercent >= 90 ? 'is-full' : ''}`}>{d.usedPercent}%</div>
              <div className="einpx-sys-sub">wolne {gb(d.freeBytes)}</div>
              <Meter percent={d.usedPercent} />
            </div>
          ))}
        </div>
      )}
    </section>
  )
}

function batteryState(state: string, onAc: boolean): string {
  if (state === 'charged') return 'naładowana'
  if (state === 'charging') return 'ładuje się'
  if (state === 'discharging') return 'na baterii'
  if (state === 'finishing charge') return 'kończy ładowanie'
  return onAc ? 'zasilanie z sieci' : 'na baterii'
}

function gb(bytes: number): string {
  return `${(bytes / 1024 ** 3).toFixed(0)} GB`
}

function Meter({ percent }: { percent: number }): JSX.Element {
  return (
    <span className="einpx-meter" aria-hidden>
      <span style={{ width: `${Math.min(100, Math.max(0, percent))}%` }} />
    </span>
  )
}

/* Minimalistyczne ikony statusu sprzętu (linia 1.6 px, jak w Apple/Linear) */
function IconBattery(): JSX.Element {
  return (
    <svg className="einpx-sys-icon" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden>
      <rect x="2.5" y="7" width="16" height="10" rx="2.5" />
      <path d="M21 10.5v3" />
      <rect x="5" y="9.5" width="8" height="5" rx="1" fill="currentColor" stroke="none" />
    </svg>
  )
}

function IconDisk(): JSX.Element {
  return (
    <svg className="einpx-sys-icon" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden>
      <rect x="3" y="4" width="18" height="16" rx="3" />
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 8.8V7.5M12 16.5v-1.3" />
    </svg>
  )
}
