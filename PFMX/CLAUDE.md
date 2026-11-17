# PFMX (Tauri 2): finanse osobiste

Osobna aplikacja w platformie ZINX. Moduł mailowy jest w `ZINX/ZIM` i nie ma z PFMX nic wspólnego.
Wygląd (`src/renderer/src/styles.css`) jest skopiowany z ZIM, żeby aplikacje wyglądały tak samo.

## Architektura
- Interfejs: React + Vite w `src/renderer`, ekrany w `src/renderer/src/finance/`.
  Komponenty znają tylko `window.api` (kontrakt w `src/shared/api.ts`); `src/renderer/src/bridge.ts` mapuje go na `invoke`.
- Rdzeń: Rust w `src-tauri/src`. Komendy w `commands.rs`, nazwy `finance_*`.
- Baza: SQLite `pfmx.sqlite` w folderze danych aplikacji. Tabele w `db/finance.rs` (konta, transakcje, importy),
  `db/budget.rs` (koperty, poduszka), `db/recurring.rs` (płatności cykliczne i subskrypcje).
- Tło: zamknięcie okna chowa je do zasobnika (`tray.rs`); odczyt CSV i wykrywanie płatności działają co 10 minut.
- Ikona: `icon-source.png` (512 px, strzałka). Po zmianie: `pnpm tauri icon icon-source.png`.

## Komendy
- `pnpm tauri dev`: aplikacja (port interfejsu 1421, nie koliduje z ZIM na 1420)
- `pnpm typecheck`: typy interfejsu
- `cd src-tauri && cargo test`: testy rdzenia

## Finanse
- Kwoty w groszach (i64), nigdy w liczbach zmiennoprzecinkowych (`finance/money.rs`).
- Import eksportu Millennium (`finance/millennium.rs`): jeden plik = wszystkie konta, rozpoznawane po numerze rachunku.
  Saldo sprawdzane krok po kroku; błąd salda przerywa import bez zapisu. Brakujące konto też przerywa import.
- Eksport (`finance_export_csv`): ten sam układ co plik banku, da się go ponownie zaimportować. Nazwa
  `eksport_pfmx.csv` nie zaczyna się od `historia_`, więc automatyczny odczyt go nie łapie.
- Deduplikacja: klucz = konto + data + kwota + saldo po transakcji. Ponowny import niczego nie dubluje.
- Odczyt folderu `Dokumenty/Millennium` (`finance/auto_import.rs`): pliki `historia_*.csv`, usuwane dopiero po udanym imporcie.
- Konta: rodzaje `bank`, `savings`, `buffer`, `broker` (rezerwa pod XTB).
- Budżet: koperty z planem; plan kopiowany z poprzedniego miesiąca; niewykorzystane kwoty rolują się do przodu.
  Stan realny = transakcje przypisane do koperty (ręcznie; później lokalne AI).
- Poduszka: suma kont `buffer` względem celu (domyślnie 2000 zł). Nadwyżka tylko się pokazuje; program nie przelewa.
- Płatności cykliczne (`db/recurring.rs`):
  - klucz odbiorcy: pole odbiorcy, a przy zakupach kartą pierwsze znaczące słowo z opisu (bez słów szumu),
  - cykliczna: raz w miesiącu, co 25–35 dni, kwota w granicach 10%, albo oznaczona przez bank jako „PŁATNOŚĆ CYKLICZNA”,
  - transakcje o tym samym kluczu i kwocie odhaczają płatność automatycznie (`fin_recurring_match`); cofnięcie zostaje zapamiętane,
  - rodzaje `other` (ekran Budżet) i `subscription` (podstrona Subskrypcje); znani dostawcy usług (lista w `SUBSCRIPTION_VENDORS`) są subskrypcjami,
  - zmiana ceny: najnowsza kwota oznaczona przez bank nowsza niż ostatnia płatność i inna niż zapisana (tylko subskrypcje).
- Powiadomienia (`finance/alertRules.ts`): nadwyżka poduszki, zmiana ceny, płatność w ciągu 3 dni, nowo wykryta płatność.
  Zamknięte zapisują się w `localStorage` tej przeglądarki (klucz `pfmx.alertsDismissed`).

- Kategorie (koperty budżetu), w kolejności: Bez Koperty (wiersz wyliczany, nie koperta: wydatki bez kategorii),
  Jedzenie i Wydatki Domowe (`food`), Wyjścia, Transport, Wydatki Jednorazowe (`oneoff`), Inwestycje (`investments`, nie do wydatków),
  Rozwój (`development`), Opłaty i Subskrypcje (`fixed`). Transakcja ma co najwyżej jedną kategorię.
- Płatność cykliczna ma kategorię (`fin_recurring_category`, domyślnie Opłaty i subskrypcje). Dopasowane do niej transakcje
  dostają tę kategorię automatycznie; zmiana kategorii przenosi już dopasowane transakcje.
  Plan „Opłat” liczy tylko płatności z tej kategorii, więc karta miesięczna w Transporcie nie jest liczona dwa razy.
- Gotówka (`db/cash.rs`, `fin_cash`): stan to suma wpisów. Ręczne wpisy ze znakiem i opisem, korekta do wpisanego stanu
  (różnica jako osobny wpis), wypłaty z bankomatu dodają się same (`sync_atm`). Wpisy z bankomatu są tylko do odczytu.
- Transakcje nieotagowane (przelewy własne, wypłaty z bankomatu, wpływy) nie liczą się do kopert.
- Odświeżanie po imporcie, po starcie i w cyklu w tle: `finance::refresh_derived` (płatności cykliczne, dopasowania, bankomat).

- Kategorie transakcji (`db/categorize.rs`, `finance/ai.rs`). Kolejność:
  1. wykluczenia: wypłaty z bankomatu, przelewy własne, wpływy (nigdy nie dostają kategorii),
  2. reguły z poprawek: ręczne przypisanie zapisuje regułę odbiorca -> kategoria i stosuje ją do pozostałych wydatków tego odbiorcy (`fin_tx_rule`),
  3. model lokalny `gemma3:4b` w Ollamie, w tle co 2 minuty, po 15 wydatków. Dostaje listę kategorii i ostatnie reguły jako przykłady.
     Odpowiedź musi być kluczem kategorii albo null; ocenione transakcje nie wracają do kolejki (`fin_tx_ai`).
     Do modelu nie trafia numer rachunku. Test z prawdziwym modelem: `cargo test -- --ignored real_model`.
- Ręczne przypisanie zawsze wygrywa z regułą i z AI.

- Wpływy (`db/income.rs`): dwa rodzaje, `salary` (wypłata: ten sam nadawca w co najmniej dwóch miesiącach, co 25–35 dni)
  i `one_off` (jednorazowy). Ręczna zmiana zapisuje się jako `manual` i nie jest nadpisywana. Przelewy własne nie są wpływem.
  W budżecie widać sumy obu rodzajów w miesiącu.
- Przelew własny (`OWN_TRANSFER_SQL` w `db/finance.rs`): numer konta z listy, albo „wewnętrzny”/„własny” w polu rodzaju lub opisie,
  w obu wariantach wielkości liter (SQLite nie zmienia wielkości liter polskich znaków). Taki wpis nie jest wydatkiem ani wpływem.
- Wyszukiwanie transakcji (`search_transactions` w `db/finance.rs`, ekran Transakcje): kategoria (Wszystkie albo jedna,
  „Bez Koperty” = id 0), rodzaj (wpływ/płatność), zakres dat i wielkości (wartość bezwzględna). Puste pole = bez ograniczenia.
- Paragony (`db/receipts.rs`, `finance_receipt_*` w `commands.rs`): przy rozwinięciu transakcji „Dołącz paragon” kopiuje zdjęcie/PDF do folderu `paragony` w danych aplikacji; tabela `fin_receipts` pamięta nazwę pliku. Jeden paragon na transakcję; nowy zastępuje stary (stary plik usuwany). „Wyświetl paragon” otwiera plik w domyślnej aplikacji systemu.
- Nawigacja w pasku bocznym reaguje na wciśnięcie (`onPointerDown`), nie na zakończone kliknięcie.

## Uwagi do środowiska
- Rust z Homebrew: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Dysk exFAT dokłada pliki `._*`, które psują budowanie: usuwaj je (`find . -name '._*' -not -path './node_modules/*' -delete`).
  Cel kompilacji jest w `~/Library/Caches/pfmx-target` (`src-tauri/.cargo/config.toml`).

## Dalej
1. Ocena jakości kategorii na Twoich danych (kilkadziesiąt ręcznie oznaczonych transakcji); porównanie z Hermes3, jeśli wyniki Gemmy będą słabe.
2. Ekran Inwestycje (XTB, waluty, kursy NBP).
3. Pakowanie wydania (`pnpm tauri build`).
