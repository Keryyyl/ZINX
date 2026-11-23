/** Przyciski akcji na wiadomościach. Używa ich lista (zaznaczenie) i czytnik (otwarta wiadomość). */
interface Props {
  /** Aktualny widok (folder): od niego zależy, które przyciski są widoczne */
  view: string
  busy: boolean
  allRead: boolean
  allStarred: boolean
  onToggleRead: () => void
  onStar: () => void
  onArchive: () => void
  onToInbox: () => void
  onSpam: () => void
  onNotSpam: () => void
  onTrash: () => void
  onDestroy: () => void
  className?: string
}

export default function MailActions(p: Props): JSX.Element {
  const { view, busy } = p
  return (
    <div className={`actions ${p.className ?? ''}`.trim()}>
      <button className="btn" disabled={busy} onClick={p.onToggleRead}>
        {p.allRead ? 'Oznacz jako nieprzeczytane' : 'Oznacz jako przeczytane'}
      </button>
      {view !== 'spam' && view !== 'trash' && (
        <button className="btn" disabled={busy} onClick={p.onStar}>
          {p.allStarred ? '☆ Usuń gwiazdkę' : '★ Oznacz gwiazdką'}
        </button>
      )}
      {(view === 'inbox' || view === 'starred' || view === 'all') && (
        <button className="btn" disabled={busy} onClick={p.onArchive}>
          Archiwizuj
        </button>
      )}
      {(view === 'archive' || view === 'trash') && (
        <button className="btn" disabled={busy} onClick={p.onToInbox}>
          Przenieś do odebranych
        </button>
      )}
      {view === 'spam' ? (
        <button className="btn" disabled={busy} onClick={p.onNotSpam}>
          To nie spam
        </button>
      ) : (
        view !== 'trash' && (
          <button
            className="btn"
            disabled={busy}
            onClick={p.onSpam}
            title="Przenosi do spamu wszystkie wiadomości od tych nadawców, także przyszłe"
          >
            Zgłoś spam
          </button>
        )
      )}
      {view === 'trash' ? (
        <button className="btn danger" disabled={busy} onClick={p.onDestroy}>
          Usuń na zawsze
        </button>
      ) : (
        <button className="btn danger" disabled={busy} onClick={p.onTrash}>
          Usuń
        </button>
      )}
    </div>
  )
}
