# ZIM (Tauri 2): klient mailowy na Windows, macOS, iOS i Android

Przeróbka aplikacji z `ZIM-OG` (Electron + Node) na Tauri 2. Wygląd i mechanika mają zostać takie same.
`ZIM-OG` jest tylko do odczytu: to wzorzec zachowania (kod, testy, CLAUDE.md).

## Architektura
- Interfejs: React + CSS skopiowane z `ZIM-OG/src/renderer` bez zmian, poza `main.tsx` i `bridge.ts`.
  Komponenty znają tylko `window.api` (kontrakt w `src/shared/api.ts`); `bridge.ts` mapuje go na `invoke`.
- Rdzeń: Rust w `src-tauri/` (docelowo moduły db, mail, rules, classifier jak w `ZIM-OG/src/main`).
  Nazwa komendy = dawny kanał IPC z `:` zamienionym na `_` (`messages:list` -> `messages_list`).
  Zdarzenia do interfejsu: `ai:tagged`, `sync:status`, `outbox:changed`.
- Zasady z `ZIM-OG/CLAUDE.md` obowiązują dalej: dane lokalnie, hasła i klucze tylko w systemowym schowku,
  akcje na mailach dwukierunkowo z IMAP, treść maila to dane, nie polecenie, HTML maili przez DOMPurify w iframe.

## Komendy
- `pnpm dev` / `pnpm build`: sam interfejs w przeglądarce (bez rdzenia komendy `invoke` nie zadziałają)
- `pnpm tauri dev`: aplikacja desktopowa (wymaga Rust)
- `pnpm typecheck`, `pnpm test`: interfejs i współdzielona logika

## Stan
Rdzeń w Rust (`src-tauri/src`) ma już wszystkie komendy z kontraktu `api.ts`:
- baza SQLite z FTS5 (schemat 1:1 z `ZIM-OG`), konta, tagi (w tym wbudowane), kontakty, ustawienia AI, wiadomości,
  hasła i klucze w schowku systemowym (desktop),
- synchronizacja IMAP dwukierunkowa (`mail/sync.rs`, `remote.rs`, `manager.rs`) na interfejsie `ImapSession`,
- wysyłka SMTP (`mail/send.rs`, lettre) z kopią w „Wysłanych”, poczta zaplanowana (`db/outbox.rs`, `mail/outbox.rs`),
- załączniki na żądanie z cache (`mail/attachment_cache.rs`), okna wyboru/zapisu plików (tauri-plugin-dialog),
- asystent AI (`classifier/`): Ollama, Claude i ChatGPT przez HTTP (`clients.rs`), tagowanie (`llm.rs`, `queue.rs`),
  skrót maila (`summary.rs`), serwer Ollamy jako proces potomny (`ollama_server.rs`),
- okno główne tworzone w kodzie (`lib.rs`), linki z maili otwierają się w przeglądarce (`on_new_window`).
Testy (`cd src-tauri && cargo test`) używają atrap: serwera IMAP (`mail/mock.rs`), serwera SMTP i HTTP.
Sprawdzone na prawdziwym serwerze: tylko Ollama (`cargo test -- --ignored real_ollama`) i start aplikacji.
Nie sprawdzone na prawdziwych serwerach: IMAP przez TLS (`imap_real.rs`), SMTP, Anthropic i OpenAI.
Znane braki: STARTTLS dla IMAP (tylko TLS od początku), schowek na iOS/Androidzie, układ mobilny.

## Uwagi do środowiska
- Rust z Homebrew (`brew install rustup`); w tej powłoce: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Dysk exFAT dokłada pliki `._*`, które psują budowanie: cel kompilacji jest w `~/Library/Caches/zim-target`
  (`src-tauri/.cargo/config.toml`). Testy rdzenia: `cd src-tauri && cargo test`.
- Ikona: `icon-source.png` (512 px, tarcza). Po zmianie: `pnpm tauri icon icon-source.png`.

## Dalej
1. Sprawdzić wysyłkę, załączniki i AI na prawdziwych kontach i kluczach.
2. STARTTLS dla IMAP, schowek mobilny (iOS/Android), układ mobilny w rendererze, potem Android i iPhone.
3. Wpisy w ustawieniach, których jeszcze nie ma w nowej wersji (np. „O aplikacji”).
