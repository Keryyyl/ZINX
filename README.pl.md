🇬🇧 [English version](README.md)

# ZINX

Zbiór osobistych aplikacji na macOS, które mają działać razem. ZIM jest też planowany na Windows, iOS i Androida.
Wszystkie dane są przechowywane lokalnie, na komputerze, w bazach SQLite. Nic nie trafia do chmury, poza tym, czego wymaga samo działanie (np. pobieranie poczty, pogoda).

Każda aplikacja to osobny projekt z własnym `CLAUDE.md`, opisującym architekturę i zasady.

## Aplikacje

| Projekt | Co robi |
|---|---|
| **[ZIM](ZIM/)** | Klient poczty. Pobiera skrzynkę, sortuje wiadomości (lokalny model AI), wysyła maile i pilnuje kolejki wysyłki. Wersja Tauri 2 poprzedniej aplikacji Electron (`ZIM-OG`, poza repozytorium). |
| **[CTHX](CTHX/)** | Planer: kalendarz miesiąca, lista zadań i wydarzeń dnia, habit tracker i tablica kanban. Obsługuje powtarzalne wpisy i urodziny z kontaktów. |
| **[PFMX](PFMX/)** | Finanse osobiste: konta, budżet w kopertach, płatności cykliczne i subskrypcje, gotówka, inwestycje. Importuje historię z banku, kategoryzuje wydatki lokalnym modelem i przypina paragony do transakcji. |
| **[EINPX](EINPX/)** | Dashboard: jedno okno z podglądem poczty (ZIM), salda i wydatków (PFMX) oraz planu dnia (CTHX), plus pogoda i szybkie notatki. |
| **[Klaudiusz](Klaudiusz/)** | Asystent głosowy po polsku, działający w pasku menu. Słucha słowa „Klaudiusz”, odczytuje dane z pozostałych aplikacji, dodaje wpisy po potwierdzeniu głosem, przypomina i obsługuje skróty klawiszowe. |
| **Exporter** | Lokalny skrypt Pythona, który pobiera historię wyciągów z banku do pliku CSV. Nie jest częścią repozytorium, bo działa na prywatnych danych logowania. |

## Jak to się łączy

- Każda aplikacja ma własną bazę SQLite w `~/Library/Application Support/app.<nazwa>/`.
- EINPX, Klaudiusz i inne czytają te bazy tylko do odczytu, a zapisy idą wyłącznie przez właściwą aplikację lub przez jawne potwierdzenie użytkownika.
- Wspólny wygląd: ZIM, CTHX, PFMX i EINPX używają tego samego stylu CSS.

## Stos technologiczny

- **Tauri 2** (Rust + WebView) dla ZIM, CTHX, PFMX, EINPX i Klaudiusza.
- **React + Vite + TypeScript** w warstwie interfejsu.
- **SQLite** (`rusqlite`) jako baza danych.
- **Ollama** (lokalne modele językowe) i **whisper.cpp** (rozpoznawanie mowy) do funkcji AI.

## Uruchomienie

Każdy projekt uruchamia się osobno, w jego katalogu:

```bash
pnpm install
pnpm tauri dev
```

Testy rdzenia w Rust:

```bash
cd <projekt>/src-tauri && cargo test
```

Wymagania: macOS, Node.js, Rust (rustup) i `pnpm`. Klaudiusz dodatkowo potrzebuje Ollamy i whisper.cpp.

## Prywatność

- Repozytorium nie zawiera haseł, kluczy API, numerów kont ani prywatnych baz danych. Dane testowe używają przykładowych wartości.
- Hasła do poczty są przechowywane w schowku systemowym (Keychain), nie w plikach.
- Folder z danymi osobistymi eksportu bankowego i repozytorium `ZIM-OG` są wykluczone w `.gitignore`.
