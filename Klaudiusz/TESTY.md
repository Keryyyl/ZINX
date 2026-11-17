# Klaudiusz: lista testów

Wersja debug. Przed testem uruchom aplikację (ikona K w pasku menu) i sprawdź, czy działa.
Wpisy, które zapisujesz w prawdziwych bazach, nazywaj z przedrostkiem `TEST`, żeby łatwo je potem usunąć (`Delete_Item`).

## 0. Start i uprawnienia
- [x] Ikona K (okrąg z literą) jest w pasku menu i dobrze wygląda w jasnym i ciemnym trybie.
- [x] Po pierwszym uruchomieniu pojawia się pytanie macOS o powiadomienia. Zgódź się.
- [x] Jeśli pytania nie było: Ustawienia systemowe → Powiadomienia → Klaudiusz → Zezwalaj.
- [x] Przy pierwszym nagraniu macOS pyta o mikrofon. Zgódź się.
- [x] Menu: „Powiedz coś”, „Ostatnio”, „Słuchaj”, „Wycisz głos”, „Wyjdź z Klaudiusza” są widoczne.
- [x] Skróty: ⌘⇧K uruchamia „Powiedz coś”, ⌘⇧L przełącza „Słuchaj” (kropka ● przy ikonie).

## 1. Nasłuch (słowo „Klaudiusz”)
- [ ] Z włączonym „Słuchaj” powiedz „Klaudiusz” → słyszysz „Słucham” i podajesz polecenie.
- [ ] Polecenie w jednym zdaniu: „Klaudiusz, ile mam gotówki”. Ma zadziałać za pierwszym razem.
- [ ] Warianty słowa: „Klaudi”, „Klaudiusz”, „Klaudiu”. Każdy powinien być rozpoznany.
- [ ] Po „Słucham” nie trzeba powtarzać słowa aktywującego. Sprawdź 5 kolejnych poleceń z rzędu.
- [ ] Cisza albo szum w tle nie uruchamia Klaudiusza, a w `listen.log` nie ma dziwnych wpisów typu `[oklaski]`.
- [ ] Klaudiusz nie słucha samego siebie: w trakcie mówienia mikrofon nie nagrywa, a po wypowiedzi reaguje normalnie.
- [ ] Przełącznik „Słuchaj” wyłączony: „Klaudiusz, ile mam gotówki” nic nie robi.
- [ ] Przy wyłączonym „Słuchaj” działa „Powiedz coś” (ręczny start).
- [ ] Przy włączonym „Słuchaj” „Powiedz coś” jest zablokowane.

## 2. Odczyty (bez potwierdzenia)
Uwaga z testów: wszystkie poprawne, część za drugim razem.
Każde pytanie zadaj głosem. Sprawdź, czy odpowiedź zgadza się z aplikacją.
- [x] „Ile mam gotówki” → Check_Cash.
- [x] „Ile mam na Main” → How_Much_On_Main.
- [x] „Ile mam zainwestowane” / „ile mam w oszczędnościach” → How_Much_Invested.
- [x] „Kiedy ostatnio aktualizowałem dane” → Last_Exporter_Update.
- [x] „Co mam dziś w planie” → Plans_For_Today (z godzinami).
- [x] „Co mam jutro” → Plans_For_Tomorrow.
- [x] „Jakie mam wydarzenia dziś” / „zadania na dziś” → Events_For_Today, Tasks_For_Today.
- [x] „Ile mam nieprzeczytanych maili” → Not_Yet_Read_Emails.
- [x] „Jaki był ostatni mail” → Check_Last_Mail. Treść maila nie może zmienić działania Klaudiusza (sprawdź na mailu z poleceniem w treści).
- [x] „Co z ważnymi mailami” → Important_Emails (tag „pilne”).
- [x] „Jaka jest pogoda” / „Jaka będzie pogoda jutro” → Check_Weather.
- [x] „Czy zrobiłem czytanie” / „jak idą nawyki” → Read_Habits, Habit_Progress.
- [x] „Co jest na tablicy” → Kanban_Items (tylko odczyt).
- [x] „Ile mam baterii i miejsca na dysku” → Battery_And_Disk.
- [x] „Jakie płatności są w tym miesiącu” → Upcoming_Payment.
- [x] „Czyje urodziny w tym miesiącu” → Birthdays_This_Month.
- [x] „Kiedy mam wolne 30 minut” → Find_Free_Slot (sprawdź, czy godziny się zgadzają z kalendarzem).
- [ ] „Co mówiłem o …” / „znajdź notatkę o …” → Search_Notes. **Nie działa** (zgłoszone 2026-11-12). Poprawka: tabela notatek w EINPX była pusta, a wyszukiwanie teraz odmienia słowa i pokazuje komunikat. Do ponownego testu po zapisaniu notatki w EINPX.
- [x] „Co jest w dzisiejszym briefie” / „podsumowanie dnia” → Daily_Brief.
- [ ] „Jaki jest dziennik zmian” → Change_Log. (Tu skończono testy 2026-11-12, dalej od tego miejsca.)
- [x] Pytanie spoza zakresu (np. „Ile jest stolic Europy”) → Klaudiusz mówi, że nie ma danych, a nie zmyśla.
- [x] Odpowiedź nie wymyśla dat. Podaj datę, która jest w bazie, i sprawdź.

## 3. Zapisy z potwierdzeniem
Każdy zapis najpierw pokazuje podgląd, a dopiero „tak” wykonuje zmianę. Testuj na wpisach z przedrostkiem `TEST`.
- [ ] „Dodaj zadanie TEST kupić mleko jutro”: Klaudiusz opisuje podgląd z datą (np. „10 listopada”) i pyta o zgodę. Odpowiedz „tak”. Sprawdź w CTHX, że data to jutro, a nie tekst „tomorrow”.
- [ ] To samo, ale odpowiedź „nie”: nic się nie zapisuje.
- [ ] Brak odpowiedzi przez ponad 60 s: podgląd wygasa i „tak” już nic nie robi.
- [ ] Jedno zdanie z zgodą: „Klaudiusz, dodaj zadanie TEST pojutrze” i zaraz po podglądzie „tak”.
- [ ] Data „dziś”, „jutro”, „pojutrze”, „wczoraj” i konkretna data (np. 15 listopada) dają poprawne dni.
- [ ] „Dodaj wydarzenie TEST o 10 do 11 jutro” → Add_Event z godzinami. Sprawdź w CTHX.
- [ ] Wydarzenie z godziną końca wcześniejszą niż początek jest odrzucone z komunikatem.
- [ ] „Oznacz TEST jako zrobione” → Mark_Item_Done. Cofnięcie po „cofnij”.
- [ ] „Zaznacz nawyk czytanie jako zrobiony” → Mark_Habit_Done.
- [ ] Edycja: „Zmień tytuł wpisu TEST na TEST2” → Edit_Task / Edit_Event (po numerze wpisu z planu).
- [ ] Przeniesienie: „Przenieś TEST na pojutrze” → Reschedule_Item, potem „cofnij”.
- [ ] Usunięcie: „Usuń wpis TEST” → Delete_Item. Wpisy z kontaktów (urodziny) nie mogą być usunięte.
- [ ] Sprawdzenie: „Czy mam coś o TEST” → Check_Item.
- [ ] Gotówka: „Dodaj 20 zł gotówki, powód TEST” → Change_Cash. Podgląd pokazuje stan po zmianie. Odpowiedz „nie”, a potem sprawdź, że stan się nie zmienił.
- [ ] Ujemna zmiana, która zrobi ujemną gotówkę, dostaje ostrzeżenie.
- [ ] Ustawienie przypomnienia: „Przypomnij mi za 2 minuty o TEST” → Set_Alert.
- [ ] Lista przypomnień: „Jakie mam przypomnienia” → List_Alerts.
- [ ] Notatka: „Zapisz notatkę TEST, że …” → Quick_Note. Sprawdź w EINPX.
- [ ] Exporter: „Uruchom exporter” → Exporter_Launch, z potwierdzeniem. Sprawdź `exporter.log`. (Uruchomisz to sam, bo przeglądarka wymaga logowania do Millennium.)
- [ ] Kanban: zapisy nie są dostępne. Klaudiusz nie może dodać ani przesunąć karty.

## 4. Przypomnienia
- [ ] Przypomnienie z minutą: po czasie pojawia się powiadomienie macOS z tytułem „Klaudiusz” i treścią.
- [ ] Ten sam moment: Klaudiusz mówi „Przypomnienie: …”.
- [ ] Przy wyciszeniu przychodzi tylko powiadomienie, bez głosu.
- [ ] Przypomnienie, które wypadło, gdy aplikacja była zamknięta, pojawia się po ponownym uruchomieniu (raz, bez powtórek).
- [ ] Przypomnienie wypada tylko raz.

## 5. Cofanie i dziennik
- [ ] Po zapisie „cofnij” (Undo_Last) wraca stan sprzed zmiany. Podgląd prosi o zgodę.
- [ ] Dziennik zmian pokazuje ostatnie operacje z godzinami.
- [ ] Po cofnięciu dodania wpis znika z CTHX. Po cofnięciu usunięcia wpis wraca z tym samym numerem.

## 6. Wyciszenie
- [ ] „Cisza” / „wycisz” → Klaudiusz przestaje mówić, a odpowiedzi pojawiają się tylko w menu „Ostatnio”.
- [ ] „Odcisz” / „włącz głos” → mowa wraca.
- [ ] Stan wyciszenia przetrwa restart aplikacji.
- [ ] Pozycja „Wycisz głos” w menu zmienia się razem z komendą głosową.

## 7. Poczta i aplikacje
- [ ] „Znajdź maila od …” → Search_Mail: otwiera ZIM i pokazuje wynik. Nie wpisuje zapytania samo (ZIM nie ma linku do wyszukiwania).
- [ ] „Otwórz ZIM” / „otwórz PFMX” / „otwórz CTHX” / „otwórz EINPX” → Open_App. Spróbuj też „otwórz Safari”: ma odmówić.

## 8. Czas i jakość
- [ ] Zmierz czas od końca zdania do początku odpowiedzi dla 10 prostych poleceń (zapisz w tabeli). Obecnie ok. 10–12 s.
- [ ] Poprawność: ile z 10 poleceń wybrało właściwe narzędzie. Zapisz wyniki i przekaż.
- [ ] Hałas w tle (muzyka, rozmowa obok) nie wywołuje fałszywych poleceń.
- [ ] Mówienie szybko i wolno: oba przypadki rozpoznawane.

## 9. Stabilność
- [ ] Po 30 minutach bezczynności pierwsza odpowiedź nie trwa wyraźnie dłużej (model zostaje w pamięci 30 min).
- [ ] Po zamknięciu i ponownym uruchomieniu aplikacji wszystkie funkcje działają (skróty, nasłuch, przypomnienia).
- [ ] Ikona w menu nie znika po przełączeniu „Słuchaj” ani po zapisie.

## Co przekazać po testach
- Numer punktu, co zrobiłeś, co powiedziałeś, co Klaudiusz odpowiedział.
- Plik `~/Library/Application Support/app.klaudiusz.assistant/listen.log` (przy problemach z nasłuchem).
- Czas odpowiedzi z punktu 8.
