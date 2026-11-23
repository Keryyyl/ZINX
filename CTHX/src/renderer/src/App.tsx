import { useCallback, useEffect, useMemo, useState } from 'react'
import type { Done, Item, ItemKind } from '@shared/types'
import { dayKey } from '@shared/recurrence'
import CalendarScreen from './calendar/CalendarScreen'
import TodoScreen from './todo/TodoScreen'
import ItemDialog from './todo/ItemDialog'
import HabitsScreen from './habits/HabitsScreen'
import KanbanScreen from './kanban/KanbanScreen'

const SIDEBAR_KEY = 'cthx.sidebarCollapsed'
const MAC = /Mac/.test(navigator.userAgent)

/** Stan menu pamiętany w tej przeglądarce; brak zapisu = menu tylko na tę sesję */
function useSidebar(): [boolean, () => void] {
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(SIDEBAR_KEY) === '1'
    } catch {
      return false
    }
  })
  const toggle = useCallback((): void => {
    setCollapsed((prev) => {
      try {
        localStorage.setItem(SIDEBAR_KEY, prev ? '0' : '1')
      } catch {
        // brak zapisu nie blokuje przełączania
      }
      return !prev
    })
  }, [])
  return [collapsed, toggle]
}

export type Section = 'calendar' | 'todo' | 'habits' | 'kanban'

const SECTIONS: { id: Section; label: string }[] = [
  { id: 'calendar', label: 'Kalendarz' },
  { id: 'todo', label: 'Lista To-do' },
  { id: 'habits', label: 'Habit tracker' },
  { id: 'kanban', label: 'Kanban' }
]

/** Stan wykonania: klucz to „id|dzień” */
export const doneKey = (itemId: number, day: string): string => `${itemId}|${day}`

export default function App(): JSX.Element {
  const [section, setSection] = useState<Section>('calendar')
  const [sidebarCollapsed, toggleSidebar] = useSidebar()
  const [day, setDay] = useState(() => dayKey(new Date()))
  const [items, setItems] = useState<Item[]>([])
  const [done, setDone] = useState<Done[]>([])
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [syncing, setSyncing] = useState(false)
  const [dialog, setDialog] = useState<{ kind: ItemKind; item?: Item } | null>(null)

  const reload = useCallback(async (): Promise<void> => {
    const [list, doneList] = await Promise.all([window.api.itemsList(), window.api.doneList()])
    setItems(list)
    setDone(doneList)
  }, [])

  useEffect(() => {
    reload().catch((e: unknown) => setError(String(e)))
  }, [reload])

  /** Urodziny z kontaktów. Przy starcie cicho (błąd tylko w konsoli); ręcznie pokazuje wynik albo błąd. */
  const syncBirthdays = useCallback(
    (manual: boolean): void => {
      setSyncing(true)
      window.api
        .birthdaysSync()
        .then((r) => {
          if (manual) {
            setError(null)
            setNotice(`Urodziny z kontaktów: dodano ${r.added}, zaktualizowano ${r.updated}, usunięto ${r.removed}.`)
          }
          return reload()
        })
        .catch((e: unknown) => {
          if (manual) setError(String(e))
          else console.warn('Synchronizacja urodzin nie powiodła się:', e)
        })
        .finally(() => setSyncing(false))
    },
    [reload]
  )

  useEffect(() => {
    syncBirthdays(false)
  }, [syncBirthdays])

  // po powrocie do okna dane same się odświeżają (np. po dodaniu wpisu z panelu EINPX)
  useEffect(() => {
    const onFocus = (): void => {
      reload().catch((e: unknown) => setError(String(e)))
    }
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  }, [reload])

  // Cmd+B (macOS) albo Ctrl+B: chowa i pokazuje menu, jak w ZIM
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key.toLowerCase() === 'b' && (MAC ? e.metaKey : e.ctrlKey) && !e.altKey && !e.shiftKey) {
        e.preventDefault()
        toggleSidebar()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [toggleSidebar])

  const doneSet = useMemo(() => new Set(done.map((d) => doneKey(d.itemId, d.date))), [done])

  const toggle = (item: Item, on: boolean, forDay: string): void => {
    setError(null)
    window.api
      .itemSetDone(item.id, forDay, on)
      .then(reload)
      .catch((e: unknown) => setError(String(e)))
  }

  const remove = (item: Item): void => {
    setError(null)
    window.api
      .itemDelete(item.id)
      .then(reload)
      .catch((e: unknown) => setError(String(e)))
  }

  return (
    <div className={`app cthx ${sidebarCollapsed ? 'sidebar-collapsed' : ''}`}>
      <div className="titlebar-drag" data-tauri-drag-region />
      <button
        className="cthx-sidebar-toggle"
        onClick={toggleSidebar}
        aria-label="Pokaż lub ukryj menu"
        aria-expanded={!sidebarCollapsed}
        title={`Pokaż / ukryj menu (${MAC ? '⌘B' : 'Ctrl+B'})`}
      >
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden>
          <path d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </button>
      <button className="cthx-refresh" onClick={() => window.location.reload()} aria-label="Odśwież dane" title="Odśwież dane">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
          <path d="M20 12a8 8 0 1 1-2.3-5.6M20 4v5h-5" />
        </svg>
      </button>
      <aside className="sidebar cthx-sidebar" aria-hidden={sidebarCollapsed}>
        <h1>CTHX</h1>
        <nav>
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              className={`view ${section === s.id ? 'active' : ''}`}
              onPointerDown={() => setSection(s.id)}
              onClick={() => setSection(s.id)}
            >
              {s.label}
            </button>
          ))}
        </nav>
      </aside>

      {/* zamglenie reszty ekranu, gdy menu jest otwarte; kliknięcie poza menu je chowa */}
      {!sidebarCollapsed && <div className="cthx-scrim" onClick={toggleSidebar} aria-hidden />}

      <section className="list cthx-main">
        {section === 'calendar' && (
          <CalendarScreen
            items={items}
            doneSet={doneSet}
            day={day}
            onDay={setDay}
            onToggle={toggle}
            onEdit={(item) => setDialog({ kind: item.kind, item })}
            onAdd={(kind) => setDialog({ kind })}
            onSyncBirthdays={() => syncBirthdays(true)}
            syncing={syncing}
          />
        )}
        {section === 'todo' && (
          <TodoScreen
            items={items}
            doneSet={doneSet}
            day={day}
            onDay={setDay}
            onToggle={toggle}
            onEdit={(item) => setDialog({ kind: item.kind, item })}
            onDelete={remove}
            onAdd={(kind) => setDialog({ kind })}
          />
        )}
        {section === 'habits' && <HabitsScreen />}
        {section === 'kanban' && <KanbanScreen />}
        {notice && !error && <div className="notice cthx-notice">{notice}</div>}
        {error && <div className="error cthx-error">{error}</div>}
      </section>

      {dialog && (
        <ItemDialog
          kind={dialog.kind}
          item={dialog.item}
          defaultDay={day}
          onClose={() => setDialog(null)}
          onSaved={() => {
            setDialog(null)
            reload().catch((e: unknown) => setError(String(e)))
          }}
        />
      )}
    </div>
  )
}
