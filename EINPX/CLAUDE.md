# EINPX (Tauri 2): dashboard

Osobne okno z podglądem ZIM (poczta), PFMX (finanse) i CTHX (plan dnia, nawyki). Port dev 1424.
Stack taki sam jak CTHX: React + Vite + TypeScript w `src/renderer`, rdzeń Rust w `src-tauri/src`, kontrakt w `src/shared/api.ts`.

## Źródła danych
- ZIM: `~/Library/Application Support/app.zim.mail/zim.sqlite` (tylko odczyt). Nieodczytane = `messages.read = 0 AND folder = 'inbox'` we wszystkich kontach. Ostatnie 3 z tej samej skrzynki.
- PFMX: `~/Library/Application Support/app.pfmx.finance/pfmx.sqlite` (tylko odczyt). Saldo konta `Main` (ostatnie `balance_after`), suma kont `savings`, gotówka (suma `fin_cash`). Koperty i inwestycje nie są pokazywane.
- CTHX: `~/Library/Application Support/app.cthx.planner/cthx.sqlite`. Odczyt planu dnia (`occurs_on` w `dates.rs` odpowiada `occursOn` z CTHX) i nawyków. Zapis: dodawanie wpisów (`source = 'manual'`), `item_done`, `habit_done`.

- Pogoda: Open-Meteo (bez klucza), miejsce w `src/renderer/src/config.ts`. Połączenie dopuszczone w CSP (`tauri.conf.json`).
- Plan dnia: `einpx_day_plan(date)`. Oś czasu i układ nakładających się wpisów pochodzą z kopii `src/shared/timeline.ts` (z CTHX). Kopię trzeba aktualizować ręcznie, gdy zmieni się CTHX.

- Komputer: `einpx_system` uruchamia `pmset -g batt` i `df -k -P` (`src-tauri/src/system.rs`). Dyski: `/` i woluminy w `/Volumes/`. Karta jest w jednym wierszu pod zegarem.

- Szybkie notatki: własna baza EINPX `~/Library/Application Support/app.einpx.dashboard/einpx.sqlite` (tabela `notes`, `src-tauri/src/notes.rs`). To jedyna baza, do której EINPX pisze poza CTHX.

Układ: po lewej plan dnia (wpisy bez godziny na górze, oś 00–24), po prawej godzina/pogoda, poczta, finanse, nawyki.

Odświeżanie co 60 s, po powrocie do okna i po każdym zapisie. Baz nie trzeba mieć uruchomionych aplikacji.

## Zasady
- Zmiany tylko w tym folderze. ZIM, PFMX i CTHX czytamy; do CTHX zapisujemy wyłącznie tabele wpisów i wykonania.
- Walidacja wpisów kopiuje `check` z CTHX (`src-tauri/src/cthx.rs`). Zmiana reguł w CTHX wymaga zmiany tutaj.
- Ścieżki baz są w `src-tauri/src/paths.rs`. Zmiana identyfikatora aplikacji w którymkolwiek projekcie wymaga zmiany tam.

## Komendy
- `pnpm dev`: sama przeglądarka (bez komend Rust nie działa).
- `pnpm tauri dev`: aplikacja desktopowa.
- `pnpm typecheck`, `pnpm test`.
- `cd src-tauri && cargo test`: testy dat i zapisu. Test zapisu działa na kopii bazy: `EINPX_TEST_HOME=<folder z kopią>` (struktura jak w `~/`).
- `cargo test snapshot_from_real_databases -- --ignored --nocapture`: odczyt prawdziwych danych, tylko do wypisania.

## Uwagi do środowiska
- Rust z Homebrew: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Usuwaj pliki `._*` z exFAT: `find . -name '._*' -not -path './node_modules/*' -delete`.
- Cel kompilacji: `~/Library/Caches/einpx-target`.
