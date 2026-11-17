import { useCallback, useEffect, useState } from 'react'
import type { Snapshot } from '@shared/api'
import { dayKey, monthKey } from './format'
import { ClockWeatherCard, FinanceHabitsCard, MailCard, SystemCard } from './cards'
import PlanCard from './plan'
import QuickNote from './notes'

/** Dane odświeżają się co minutę, po powrocie do okna i po każdej zmianie */
const REFRESH_MS = 60_000

export default function App(): JSX.Element {
  const [snap, setSnap] = useState<Snapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)

  const load = useCallback(async (): Promise<void> => {
    const now = new Date()
    try {
      setSnap(await window.api.snapshot(dayKey(now), monthKey(now)))
      setError(null)
    } catch (e: unknown) {
      setError(String(e))
    }
    setTick((t) => t + 1)
  }, [])

  useEffect(() => {
    void load()
    const timer = window.setInterval(() => void load(), REFRESH_MS)
    const onFocus = (): void => void load()
    window.addEventListener('focus', onFocus)
    return () => {
      window.clearInterval(timer)
      window.removeEventListener('focus', onFocus)
    }
  }, [load])

  /** Zapis do CTHX, potem natychmiastowe odświeżenie widoku */
  const act = useCallback(
    async (fn: () => Promise<void>): Promise<void> => {
      setError(null)
      try {
        await fn()
      } catch (e: unknown) {
        setError(String(e))
      }
      await load()
    },
    [load]
  )

  const today = dayKey(new Date())
  const updated = snap ? new Date(snap.fetchedAt).toLocaleTimeString('pl-PL', { hour: '2-digit', minute: '2-digit' }) : '…'

  return (
    <div className="einpx">
      <header className="einpx-head" data-tauri-drag-region>
        <div className="einpx-title">
          <span className={`einpx-status ${error ? 'is-error' : ''}`} title={error ? 'Błąd odczytu danych' : 'Wszystko działa'} aria-hidden />
          <h1>EINPX</h1>
          <span className="muted small">Stan z {updated}</span>
        </div>
        <button className="einpx-btn" onClick={() => void load()} title="Odśwież dane">
          Odśwież
        </button>
      </header>

      {error && <div className="einpx-error">{error}</div>}

      <main className="einpx-layout">
        <div className="einpx-left">
          <PlanCard tick={tick} onChange={act} />
          <QuickNote />
        </div>
        <div className="einpx-right">
          <ClockWeatherCard />
          <SystemCard />
          <MailCard section={snap?.mail} />
          <FinanceHabitsCard
            finance={snap?.finance}
            habits={snap?.habits}
            onToggle={(habit, done) => act(() => window.api.setHabitDay(habit.id, today, done))}
          />
        </div>
      </main>
    </div>
  )
}

