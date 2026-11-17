# CTHX (Tauri 2): kalendarz, lista zadań i habit tracker

Trzeci projekt w platformie ZINX. Wygląd i mechanika mają być kopią graficzną ZIM-a
(`src/renderer/src/styles.css` skopiowany z `ZIM/src/renderer/src/styles.css`, te same zmienne kolorów).
Pozostałe projekty w `ZINX/` są tylko do odczytu, zmiany wyłącznie w tym folderze.

## Podstrony
1. **Kalendarz:** siatka miesiąca (od poniedziałku) z wydarzeniami i zadaniami, agenda wybranego dnia.
2. **Lista To-do:** zadania i wydarzenia wybranego dnia. Przyciski „Dodaj zadanie do wykonania” i „Dodaj wydarzenie”.
   Wiersz: checkbox (zrobione w tym dniu), nazwa, opis, godzina, Edytuj, Usuń (drugie kliknięcie potwierdza).
3. **Habit tracker:** do ustalenia w kolejnej sesji (obecnie tylko miejsce w układzie).

## Architektura
- Interfejs: React + Vite w `src/renderer`, komponenty w `calendar/`, `todo/`, `habits/`.
  Komponenty znają tylko `window.api` (kontrakt w `src/shared/api.ts`); `src/renderer/src/bridge.ts` mapuje go na `invoke`.
- Logika wspólna (testowana w `src/shared`): `recurrence.ts` liczy, czy wpis wypada danego dnia
  (`occursOn`), siatkę miesiąca i daty. Rdzeń Rust nie zna powtarzalności, tylko przechowuje datę startu.
- Rdzeń: Rust w `src-tauri/src`. Komendy `cthx_*` w `commands.rs`.
- Baza: SQLite `cthx.sqlite` w folderze danych aplikacji. Tabele w `db.rs`:
  - `items`: zadania (`kind = task`) i wydarzenia (`kind = event`), data startu, godziny, powtarzalność.
  - `item_done`: wykonane wystąpienia (id wpisu + dzień), żeby powtarzalne zadanie dało się odhaczać każdego dnia osobno.
- Dashboard EINPX zapisuje wpisy i wykonanie bezpośrednio do `cthx.sqlite` (patrz `EINPX/CLAUDE.md`). CTHX pokazuje je po
  przycisku „Odśwież” (prawy górny róg) albo po powrocie do okna (`focus` ładuje dane ponownie).
- Okno: na macOS pasek tytułowy przezroczysty, przeciąganie przez `.titlebar-drag` z `data-tauri-drag-region`.
  Uprawnienie `core:window:allow-start-dragging` jest w `capabilities/default.json` (bez niego okno się nie rusza).

## Komendy
- `pnpm dev`: interfejs w przeglądarce, port 1422 (ZIM 1420, PFMX 1421). Bez rdzenia komendy nie zadziałają.
- `pnpm tauri dev`: aplikacja desktopowa.
- `pnpm typecheck`, `pnpm test` (vitest, tylko `src/shared`).
- `cd src-tauri && cargo test`: testy bazy i wpisów.

## Uwagi do środowiska
- Rust z Homebrew: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Dysk exFAT dokłada pliki `._*`, które psują budowanie: usuwaj je (`find . -name '._*' -not -path './node_modules/*' -delete`).
  Cel kompilacji jest w `~/Library/Caches/cthx-target` (`src-tauri/.cargo/config.toml`).
- Ikona: `icon-source.png` i ikony skopiowane z ZIM (tymczasowe). Po zmianie: `pnpm tauri icon icon-source.png`.

## Dalej
1. Habit tracker (ustalić z Karolem zakres przed kodem).
2. Edycja wpisu i usuwanie sprawdzone tylko w przeglądarce, do przetestowania w `pnpm tauri dev`.
3. Przycisk „Dodaj wydarzenie” i zadania: walidacja czasu w oknie zanim wyśle się do rdzenia (obecnie błąd z rdzenia).
