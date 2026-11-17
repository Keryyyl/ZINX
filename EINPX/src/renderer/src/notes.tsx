import { useCallback, useEffect, useState, type FormEvent } from 'react'
import type { Note } from '@shared/api'
import { Card } from './cards'

/** Szybka notatka: wpisz, Cmd/Ctrl+Enter zapisuje, lista ostatnich poniżej */
export default function QuickNote(): JSX.Element {
  const [text, setText] = useState('')
  const [notes, setNotes] = useState<Note[]>([])
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

  const load = useCallback(async (): Promise<void> => {
    try {
      setNotes(await window.api.notesList())
    } catch (e: unknown) {
      setError(String(e))
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const save = async (e?: FormEvent): Promise<void> => {
    e?.preventDefault()
    if (!text.trim()) return
    setSaving(true)
    setError(null)
    try {
      await window.api.noteAdd(text)
      setText('')
      await load()
    } catch (err: unknown) {
      setError(String(err))
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id: number): Promise<void> => {
    setError(null)
    try {
      await window.api.noteDelete(id)
      await load()
    } catch (err: unknown) {
      setError(String(err))
    }
  }

  return (
    <Card title="Szybka notatka" subtitle="EINPX" className="einpx-notes">
      <form className="einpx-note-form" onSubmit={(e) => void save(e)}>
        <textarea
          placeholder="Luźna myśl… (Cmd+Enter zapisuje)"
          value={text}
          rows={2}
          maxLength={2000}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void save()
          }}
        />
        <div className="einpx-note-actions">
          <button className="einpx-btn primary small-btn" disabled={saving || !text.trim()}>
            Zapisz
          </button>
        </div>
      </form>
      {error && <p className="einpx-section-error">{error}</p>}
      {notes.length > 0 && (
        <ul className="einpx-note-list">
          {notes.slice(0, 5).map((n) => (
            <li key={n.id} className="einpx-note-item">
              <span className="einpx-note-text">{n.text}</span>
              <span className="einpx-note-meta">
                {new Date(n.createdAt).toLocaleString('pl-PL', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })}
              </span>
              <button className="einpx-note-del" onClick={() => void remove(n.id)} aria-label="Usuń notatkę" title="Usuń">
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
    </Card>
  )
}
