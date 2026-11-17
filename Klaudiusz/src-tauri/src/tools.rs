//! Rejestr narzędzi, które model może wywołać. Etap 2: tylko odczyty (bez zmian w bazach).
//! Każde narzędzie zwraca JSON z polami po polsku. Błędy wracają jako tekst, nie panikują.

use crate::money::pln;
use crate::cthx_write::{self, Changes, ItemDraft};
use crate::{alerts, apps, exporter, journal, undo};
use crate::{cthx, dates, finance, mail, notes, system, weather};
use chrono::{Local, TimeZone};
use serde_json::{json, Value};

fn arg_i64(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

const WRITE_NOTE_TASK: &str = "Dodaje zadanie do CTHX. Najpierw wywołaj bez potwierdzone (podgląd), powiedz użytkownikowi co zostanie dodane i dopiero po jego zgodzie wywołaj ponownie z potwierdzone=true.";
const WRITE_NOTE_EVENT: &str = "Dodaje wydarzenie do CTHX. Najpierw wywołaj bez potwierdzone (podgląd), powiedz użytkownikowi co zostanie dodane i dopiero po jego zgodzie wywołaj ponownie z potwierdzone=true.";
const WRITE_NOTE_EDIT: &str = "Zmienia istniejący wpis (numer id z Plans_For_Today). Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true. Pusta godzina (\"\") usuwa godzinę.";

fn task_params(_event: bool) -> Value {
    json!({"type": "object", "properties": {
        "tytul": {"type": "string"},
        "opis": {"type": "string"},
        "data": {"type": "string", "description": "RRRR-MM-DD, domyślnie dziś"},
        "godzina_start": {"type": "string", "description": "GG:MM, opcjonalnie"},
        "godzina_koniec": {"type": "string", "description": "GG:MM, opcjonalnie"},
        "powtarzanie": {"type": "string", "enum": ["none", "daily", "weekly", "monthly", "yearly"]},
        "potwierdzone": {"type": "boolean"}
    }, "required": ["tytul"]})
}

fn edit_params(_what: &str) -> Value {
    json!({"type": "object", "properties": {
        "id": {"type": "integer", "description": "numer wpisu z planu"},
        "tytul": {"type": "string"},
        "opis": {"type": "string"},
        "data": {"type": "string"},
        "godzina_start": {"type": "string", "description": "GG:MM; pusty tekst usuwa godzinę"},
        "godzina_koniec": {"type": "string", "description": "GG:MM; pusty tekst usuwa godzinę"},
        "powtarzanie": {"type": "string", "enum": ["none", "daily", "weekly", "monthly", "yearly"]},
        "potwierdzone": {"type": "boolean"}
    }, "required": ["id"]})
}

fn no_args() -> Value {
    json!({"type": "object", "properties": {}})
}

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef { name: "Check_Cash", description: "Ile mam gotówki (suma wpisów gotówki w PFMX)", parameters: no_args() },
        ToolDef { name: "How_Much_On_Main", description: "Saldo konta Main", parameters: no_args() },
        ToolDef { name: "How_Much_Invested", description: "Suma kont oszczędnościowych i poduszki finansowej", parameters: no_args() },
        ToolDef { name: "Last_Exporter_Update", description: "Kiedy ostatnio zaktualizowano dane PFMX z Exportera", parameters: no_args() },
        ToolDef { name: "Plans_For_Today", description: "Plan na dziś: wydarzenia i zadania z CTHX", parameters: no_args() },
        ToolDef { name: "Plans_For_Tomorrow", description: "Plan na jutro z CTHX", parameters: no_args() },
        ToolDef { name: "Events_For_Today", description: "Wydarzenia na dziś z CTHX", parameters: no_args() },
        ToolDef { name: "Tasks_For_Today", description: "Zadania na dziś z CTHX", parameters: no_args() },
        ToolDef { name: "Not_Yet_Read_Emails", description: "Liczba nieprzeczytanych maili w skrzynce odbiorczej", parameters: no_args() },
        ToolDef { name: "Check_Last_Mail", description: "Ostatni mail w skrzynce odbiorczej (nadawca, temat, konto)", parameters: no_args() },
        ToolDef {
            name: "Important_Emails",
            description: "Nieprzeczytane maile z tagiem „Pilne” (liczba i kilka pierwszych)",
            parameters: no_args(),
        },
        ToolDef { name: "Check_Weather", description: "Pogoda w Warszawie: teraz, minimum i maksimum dnia, opis", parameters: no_args() },
        ToolDef { name: "Read_Habits", description: "Lista nawyków: czy zrobione dziś i ile razy w tym miesiącu", parameters: no_args() },
        ToolDef { name: "Habit_Progress", description: "Postęp nawyków w tym miesiącu w procentach celu", parameters: no_args() },
        ToolDef { name: "Kanban_Items", description: "Zadania kanbanu na bieżący miesiąc według kolumn", parameters: no_args() },
        ToolDef { name: "Battery_And_Disk", description: "Poziom baterii i zajętość dysków", parameters: no_args() },
        ToolDef { name: "Upcoming_Payment", description: "Płatności cykliczne z PFMX, które jeszcze nie są oznaczone jako zapłacone w tym miesiącu", parameters: no_args() },
        ToolDef { name: "Birthdays_This_Month", description: "Urodziny z kontaktów w bieżącym miesiącu", parameters: no_args() },
        ToolDef {
            name: "Find_Free_Slot",
            description: "Wolne okna w planie na dziś (lub podany dzień) między 08:00 a 22:00",
            parameters: json!({"type": "object", "properties": {
                "minuty": {"type": "integer", "description": "potrzebna długość w minutach, domyślnie 30"},
                "data": {"type": "string", "description": "dzień RRRR-MM-DD, domyślnie dziś"}
            }}),
        },
        ToolDef {
            name: "Search_Notes",
            description: "Szuka w szybkich notatkach z EINPX po fragmencie tekstu",
            parameters: json!({"type": "object", "properties": {"szukaj": {"type": "string"}}, "required": ["szukaj"]}),
        },
        ToolDef { name: "Add_Task", description: WRITE_NOTE_TASK, parameters: task_params(false) },
        ToolDef { name: "Add_Event", description: WRITE_NOTE_EVENT, parameters: task_params(false) },
        ToolDef { name: "Edit_Task", description: WRITE_NOTE_EDIT, parameters: edit_params("zadanie") },
        ToolDef { name: "Edit_Event", description: WRITE_NOTE_EDIT, parameters: edit_params("wydarzenie") },
        ToolDef {
            name: "Mark_Item_Done",
            description: "Odhacza (lub odznacza) zadanie albo wydarzenie z planu po numerze id z Plans_For_Today. Zapisuje dopiero po potwierdzeniu.",
            parameters: json!({"type": "object", "properties": {
                "id": {"type": "integer", "description": "numer wpisu z planu"},
                "data": {"type": "string", "description": "dzień RRRR-MM-DD, domyślnie dziś"},
                "zrobione": {"type": "boolean", "description": "true = odhacz, false = odznacz; domyślnie true"},
                "potwierdzone": {"type": "boolean"}
            }, "required": ["id"]}),
        },
        ToolDef {
            name: "Mark_Habit_Done",
            description: "Odhacza nawyk z CTHX po nazwie (np. „czytanie”) na dany dzień. Zapisuje dopiero po potwierdzeniu.",
            parameters: json!({"type": "object", "properties": {
                "nazwa": {"type": "string"},
                "data": {"type": "string", "description": "dzień RRRR-MM-DD, domyślnie dziś"},
                "zrobione": {"type": "boolean", "description": "true = odhacz, false = odznacz; domyślnie true"},
                "potwierdzone": {"type": "boolean"}
            }, "required": ["nazwa"]}),
        },
        ToolDef {
            name: "Set_Alert",
            description: "Ustawia przypomnienie (powiadomienie macOS). Podaj item_id z planu i minuty_przed (wtedy przypomnienie jest przed godziną wpisu), albo tekst z za_minut, albo tekst z o (GG:MM). Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true.",
            parameters: json!({"type": "object", "properties": {
                "tekst": {"type": "string", "description": "treść przypomnienia (przy za_minut albo o)"},
                "item_id": {"type": "integer", "description": "numer wpisu z planu"},
                "minuty_przed": {"type": "integer", "description": "ile minut przed wpisem, domyślnie 10"},
                "za_minut": {"type": "integer"},
                "o": {"type": "string", "description": "GG:MM dzisiaj albo w dniu z data"},
                "data": {"type": "string", "description": "RRRR-MM-DD, domyślnie dziś"},
                "potwierdzone": {"type": "boolean"}
            }}),
        },
        ToolDef { name: "List_Alerts", description: "Lista zaplanowanych przypomnień, które jeszcze nie wypadły", parameters: no_args() },
        ToolDef {
            name: "Change_Cash",
            description: "Zmienia gotówkę: kwota w złotych ze znakiem (+ wpłata, − wydatek) i powód. Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true.",
            parameters: json!({"type": "object", "properties": {
                "kwota": {"type": "number", "description": "w złotych, np. -50 albo 120.5"},
                "powod": {"type": "string", "description": "za co, np. obiad"},
                "potwierdzone": {"type": "boolean"}
            }, "required": ["kwota"]}),
        },
        ToolDef {
            name: "Open_App",
            description: "Otwiera aplikację ZINX: ZIM, PFMX, CTHX albo EINPX",
            parameters: json!({"type": "object", "properties": {"aplikacja": {"type": "string", "enum": ["ZIM", "PFMX", "CTHX", "EINPX"]}}, "required": ["aplikacja"]}),
        },
        ToolDef {
            name: "Search_Mail",
            description: "Szuka maili w skrzynce odbiorczej po nadawcy lub temacie i otwiera ZIM z pocztą (ZIM nie przyjmuje jeszcze zapytania z zewnątrz)",
            parameters: json!({"type": "object", "properties": {"szukaj": {"type": "string"}}, "required": ["szukaj"]}),
        },
        ToolDef {
            name: "Exporter_Launch",
            description: "Uruchamia Exporter, który pobiera wyciągi z Millennium do PFMX. Otworzy przeglądarkę do logowania, logowanie wykonujesz sam. Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true.",
            parameters: json!({"type": "object", "properties": {"potwierdzone": {"type": "boolean"}}}),
        },
        ToolDef {
            name: "Quick_Note",
            description: "Dopisuje szybką notatkę do EINPX. Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true.",
            parameters: json!({"type": "object", "properties": {
                "tekst": {"type": "string", "description": "treść notatki"},
                "potwierdzone": {"type": "boolean"}
            }, "required": ["tekst"]}),
        },
        ToolDef {
            name: "Undo_Last",
            description: "Cofa ostatnią zmianę (dodanie, edycję, odhaczenie, przypomnienie, gotówkę, notatkę). Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true.",
            parameters: json!({"type": "object", "properties": {"potwierdzone": {"type": "boolean"}}}),
        },
        ToolDef { name: "Change_Log", description: "Ostatnie zmiany wykonane przez Klaudiusza (dziennik)", parameters: no_args() },
        ToolDef {
            name: "Delete_Item",
            description: "Usuwa wpis z CTHX (tylko ręcznie dodane; wpisów z kontaktów nie usuwamy). Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true.",
            parameters: json!({"type": "object", "properties": {"id": {"type": "integer"}, "potwierdzone": {"type": "boolean"}}, "required": ["id"]}),
        },
        ToolDef {
            name: "Reschedule_Item",
            description: "Przenosi wpis z CTHX na inny dzień, a opcjonalnie zmienia godzinę rozpoczęcia. Najpierw wywołaj bez potwierdzone (podgląd), a po zgodzie użytkownika z potwierdzone=true.",
            parameters: json!({"type": "object", "properties": {
                "id": {"type": "integer"},
                "data": {"type": "string", "description": "nowy dzień RRRR-MM-DD"},
                "godzina_start": {"type": "string", "description": "GG:MM, opcjonalnie"},
                "potwierdzone": {"type": "boolean"}
            }, "required": ["id", "data"]}),
        },
        ToolDef {
            name: "Check_Item",
            description: "Szuka wpisów z CTHX po fragmencie tytułu lub opisu i pokazuje szczegóły (data, godzina, powtarzanie, opis, czy odhaczony dzisiaj). Tylko odczyt.",
            parameters: json!({"type": "object", "properties": {"szukaj": {"type": "string"}}, "required": ["szukaj"]}),
        },
        ToolDef { name: "Daily_Brief", description: "Krótkie podsumowanie dnia: pogoda, poczta, plan, finanse", parameters: no_args() },
    ]
}

/// Opisy narzędzi w formacie, jaki przyjmuje Ollama (`tools` w /api/chat)
pub fn ollama_tools() -> Value {
    Value::Array(
        definitions()
            .into_iter()
            .map(|d| json!({"type": "function", "function": {"name": d.name, "description": d.description, "parameters": d.parameters}}))
            .collect(),
    )
}

/// Wykonuje narzędzie o podanej nazwie. Zwraca JSON albo czytelny komunikat błędu.
pub fn run(name: &str, args: &Value) -> Result<Value, String> {
    // Słowa typu „jutro” zamieniamy na datę zanim trafią do narzędzia
    let normalized;
    let args = match args.get("data").and_then(Value::as_str).and_then(dates::normalize_day) {
        Some(day) => {
            let mut copy = args.clone();
            copy["data"] = Value::String(day);
            normalized = copy;
            &normalized
        }
        None => args,
    };
    match name {
        "Check_Cash" => {
            let c = finance::connect()?;
            Ok(json!({"gotowka": pln(finance::cash(&c)?)}))
        }
        "How_Much_On_Main" => {
            let c = finance::connect()?;
            Ok(json!({"konto_main": pln(finance::main_balance(&c)?)}))
        }
        "How_Much_Invested" => {
            let c = finance::connect()?;
            Ok(json!({"oszczednosci_i_poduszka": pln(finance::savings_and_buffer(&c)?)}))
        }
        "Last_Exporter_Update" => {
            let c = finance::connect()?;
            Ok(match finance::last_import(&c)? {
                Some((file, ms)) => json!({"plik": file, "kiedy": local_time(ms)}),
                None => json!({"info": "Brak importów z Exportera"}),
            })
        }
        "Plans_For_Today" => plan_for(&dates::today()),
        "Plans_For_Tomorrow" => plan_for(&dates::tomorrow()),
        "Events_For_Today" => filter_plan(&dates::today(), "wydarzenie"),
        "Tasks_For_Today" => filter_plan(&dates::today(), "zadanie"),
        "Not_Yet_Read_Emails" => {
            let c = mail::connect()?;
            Ok(json!({"nieprzeczytane": mail::unread(&c)?}))
        }
        "Check_Last_Mail" => {
            let c = mail::connect()?;
            Ok(json!({"ostatni": mail::latest(&c, 1)?}))
        }
        "Important_Emails" => {
            let c = mail::connect()?;
            let (total, list) = mail::important(&c, 5)?;
            Ok(json!({"nieprzeczytane_pilne": total, "pierwsze": list}))
        }
        "Check_Weather" => serde_json::to_value(weather::fetch()?).map_err(|e| e.to_string()),
        "Read_Habits" => {
            let c = cthx::connect()?;
            Ok(json!({"nawyki": cthx::habits(&c, &dates::today(), &dates::current_month())?}))
        }
        "Habit_Progress" => {
            let c = cthx::connect()?;
            let list = cthx::habits(&c, &dates::today(), &dates::current_month())?;
            let progress: Vec<Value> = list
                .iter()
                .map(|h| {
                    let procent = if h.cel_miesieczny > 0 { Some(h.w_tym_miesiacu * 100 / h.cel_miesieczny) } else { None };
                    json!({"nazwa": h.nazwa, "w_tym_miesiacu": h.w_tym_miesiacu, "cel": h.cel_miesieczny, "procent": procent})
                })
                .collect();
            Ok(json!({"postep": progress}))
        }
        "Kanban_Items" => {
            let c = cthx::connect()?;
            let items = cthx::kanban(&c, &dates::current_month())?;
            let column = |status: &str| -> Vec<Value> {
                items.iter().filter(|i| i.status == status).map(|i| json!({"tytul": i.tytul, "opis": i.opis})).collect()
            };
            Ok(json!({"miesiac": dates::current_month(), "do_zrobienia": column("todo"), "w_trakcie": column("doing"), "zrobione": column("done")}))
        }
        "Battery_And_Disk" => serde_json::to_value(system::info()?).map_err(|e| e.to_string()),
        "Upcoming_Payment" => {
            let c = finance::connect()?;
            let list = finance::upcoming_payments(&c, &dates::current_month())?;
            let suma: i64 = list.iter().map(|p| p.kwota_grosze).sum();
            Ok(json!({"miesiac": dates::current_month(), "platnosci": list, "suma": pln(suma)}))
        }
        "Birthdays_This_Month" => {
            let c = cthx::connect()?;
            Ok(json!({"miesiac": dates::current_month(), "urodziny": cthx::birthdays(&c, &dates::current_month())?}))
        }
        "Find_Free_Slot" => {
            let minuty = arg_i64(args, "minuty", 30).clamp(5, 720) as u32;
            let day = match arg_str(args, "data") {
                Some(d) if dates::valid_date(d) => d.to_string(),
                Some(_) => return Err("Nieprawidłowa data, podaj RRRR-MM-DD".into()),
                None => dates::today(),
            };
            let c = cthx::connect()?;
            let slots = cthx::free_slots(&cthx::plan(&c, &day)?, minuty);
            Ok(json!({"data": day, "minuty": minuty, "wolne": slots.iter().map(|(a, b)| json!({"od": a, "do": b})).collect::<Vec<_>>()}))
        }
        "Search_Notes" => {
            let query = arg_str(args, "szukaj").ok_or("Podaj, czego szukać")?;
            let found = notes::search(query, 10)?;
            if found.is_empty() {
                let total = notes::count()?;
                let info = if total == 0 {
                    "W EINPX nie ma jeszcze żadnych notatek".to_string()
                } else {
                    format!("Nie znaleziono notatek pasujących do „{query}” (wszystkich notatek: {total})")
                };
                return Ok(json!({"szukaj": query, "znalezione": [], "info": info}));
            }
            Ok(json!({"szukaj": query, "znalezione": found}))
        }
        "Add_Task" => add_item("task", args),
        "Add_Event" => add_item("event", args),
        "Edit_Task" => edit_item("task", args),
        "Edit_Event" => edit_item("event", args),
        "Mark_Item_Done" => mark_item(args),
        "Mark_Habit_Done" => mark_habit(args),
        "Set_Alert" => set_alert(args),
        "Undo_Last" => undo_last(args),
        "Delete_Item" => delete_item(args),
        "Reschedule_Item" => reschedule_item(args),
        "Check_Item" => {
            let query = str_arg(args, "szukaj").filter(|s| !s.is_empty()).ok_or("Podaj, czego szukać w CTHX")?;
            let conn = cthx::connect()?;
            let today = dates::today();
            let mut found = Vec::new();
            for (id, d) in cthx_write_find(&conn, &query)? {
                let done_today = cthx_write::is_done(&conn, "item_done", "item_id", id, &today).unwrap_or(false);
                found.push(json!({
                    "id": id,
                    "rodzaj": if d.kind == "task" { "zadanie" } else { "wydarzenie" },
                    "tytul": d.title,
                    "data": dates::pl_date(&d.date),
                    "godzina": match (&d.start, &d.end) { (Some(s), Some(e)) => format!("{s}–{e}"), (Some(s), None) => s.clone(), _ => String::new() },
                    "powtarzanie": d.recurrence,
                    "opis": d.description,
                    "zrobione_dzis": done_today,
                }));
            }
            Ok(json!({"szukaj": query, "znalezione": found}))
        }
        "Change_Log" => {
            let conn = journal::open()?;
            let list: Vec<Value> = journal::recent(&conn, 10)?
                .into_iter()
                .map(|(ms, desc, undone)| json!({"kiedy": dates::fmt_local(ms), "opis": desc, "cofniete": undone}))
                .collect();
            Ok(json!({"dziennik": list}))
        }
        "Exporter_Launch" => preview_or(args, "Uruchomię Exporter. Otworzy się przeglądarka do logowania w Millennium, logowanie zrobisz sam".into(), || {
            exporter::launch()?;
            Ok(json!({"status": "uruchomiono", "opis": "Exporter uruchomiony, zaloguj się w przeglądarce"}))
        }),
        "Quick_Note" => {
            let text = str_arg(args, "tekst").filter(|s| !s.is_empty()).ok_or("Podaj treść notatki")?;
            let opis = format!("Dopiszę notatkę: „{text}”");
            preview_or(args, opis.clone(), || {
                let id = notes::add(&text, dates::now_ms())?;
                journal::record(&format!("Notatka: {text}"), &json!({"op": "delete_note", "id": id}))?;
                Ok(json!({"status": "zapisano", "id": id, "opis": opis}))
            })
        }
        "Open_App" => {
            let app = str_arg(args, "aplikacja").ok_or("Podaj nazwę aplikacji")?;
            apps::open(&app)?;
            Ok(json!({"status": "otwarto", "aplikacja": app}))
        }
        "Search_Mail" => {
            let query = str_arg(args, "szukaj").filter(|s| !s.is_empty()).ok_or("Podaj, czego szukać w mailach")?;
            let found = {
                let conn = mail::connect()?;
                mail::search(&conn, &query, 5)?
            };
            // ZIM nie ma jeszcze adresu do otwarcia z wyszukiwaniem, więc otwieramy sam program
            apps::open("ZIM")?;
            Ok(json!({"szukaj": query, "znalezione": found, "uwaga": "ZIM otwarty, wyszukiwanie wpisz w jego pasku"}))
        }
        "Change_Cash" => change_cash(args),
        "List_Alerts" => {
            let conn = alerts::open()?;
            let list: Vec<Value> = alerts::pending(&conn)?
                .into_iter()
                .map(|a| json!({"id": a.id, "tekst": a.text, "kiedy": dates::fmt_local(a.fire_at_ms)}))
                .collect();
            Ok(json!({"przypomnienia": list}))
        }
        "Daily_Brief" => daily_brief(),
        other => Err(format!("Nieznane narzędzie: {other}")),
    }
}

fn confirmed(args: &Value) -> bool {
    args.get("potwierdzone").and_then(Value::as_bool).unwrap_or(false)
}

/// Bez potwierdzenia zwraca tylko podgląd. Zapis następuje dopiero przy `potwierdzone = true`.
fn preview_or(args: &Value, opis: String, apply: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    if confirmed(args) {
        apply()
    } else {
        Ok(json!({"status": "podglad", "opis": opis, "wymaga": "Zapytaj użytkownika o zgodę i wywołaj ponownie z potwierdzone=true"}))
    }
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(|s| s.trim().to_string())
}

fn draft_from_args(kind: &str, args: &Value) -> ItemDraft {
    ItemDraft {
        kind: kind.into(),
        title: str_arg(args, "tytul").unwrap_or_default(),
        description: str_arg(args, "opis").unwrap_or_default(),
        date: str_arg(args, "data").filter(|s| !s.is_empty()).unwrap_or_else(dates::today),
        start: str_arg(args, "godzina_start").filter(|s| !s.is_empty()),
        end: str_arg(args, "godzina_koniec").filter(|s| !s.is_empty()),
        recurrence: str_arg(args, "powtarzanie").filter(|s| !s.is_empty()).unwrap_or_else(|| "none".into()),
    }
}

fn describe_draft(d: &ItemDraft) -> String {
    let what = if d.kind == "task" { "zadanie" } else { "wydarzenie" };
    let when = match (&d.start, &d.end) {
        (Some(s), Some(e)) => format!(" {s}–{e}"),
        (Some(s), None) => format!(" o {s}"),
        _ => String::new(),
    };
    let rep = if d.recurrence == "none" { String::new() } else { format!(", powtarzanie: {}", d.recurrence) };
    format!("{what} „{}” na {}{when}{rep}", d.title, dates::pl_date(&d.date))
}

fn add_item(kind: &str, args: &Value) -> Result<Value, String> {
    let draft = draft_from_args(kind, args);
    cthx_write::validate(&draft)?;
    preview_or(args, format!("Dodam {}", describe_draft(&draft)), || {
        let conn = cthx_write::connect_write()?;
        let id = cthx_write::add(&conn, &draft)?;
        let desc = format!("Dodano {}", describe_draft(&draft));
        journal::record(&desc, &json!({"op": "delete_item", "id": id}))?;
        Ok(json!({"status": "zapisano", "id": id, "opis": desc}))
    })
}

fn edit_item(kind: &str, args: &Value) -> Result<Value, String> {
    let id = args.get("id").and_then(Value::as_i64).ok_or("Podaj numer wpisu (id)")?;
    let conn = cthx_write::connect_write()?;
    let before = cthx_write::load(&conn, id, kind)?;
    let changes = Changes {
        title: str_arg(args, "tytul"),
        description: str_arg(args, "opis"),
        date: str_arg(args, "data").filter(|s| !s.is_empty()),
        start: str_arg(args, "godzina_start"),
        end: str_arg(args, "godzina_koniec"),
        recurrence: str_arg(args, "powtarzanie").filter(|s| !s.is_empty()),
    };
    let after = cthx_write::apply_changes(before.clone(), changes);
    cthx_write::validate(&after)?;
    let opis = format!("Zmienię „{}” na: {}", before.title, describe_draft(&after));
    preview_or(args, opis.clone(), || {
        cthx_write::replace(&conn, id, &after)?;
        journal::record(
            &format!("Zmieniono „{}”", before.title),
            &json!({"op": "restore_item", "id": id, "draft": cthx_write::draft_to_json(&before)}),
        )?;
        Ok(json!({"status": "zapisano", "id": id, "opis": opis}))
    })
}

fn cthx_write_find(conn: &rusqlite::Connection, query: &str) -> Result<Vec<(i64, ItemDraft)>, String> {
    cthx_write::find_items(conn, query, 5)
}

fn delete_item(args: &Value) -> Result<Value, String> {
    let id = args.get("id").and_then(Value::as_i64).ok_or("Podaj numer wpisu (id)")?;
    let conn = cthx_write::connect_write()?;
    let (draft, source) = cthx_write::load_any(&conn, id)?;
    if source != "manual" {
        return Err("Ten wpis pochodzi z kontaktów, nie usuwam go".into());
    }
    let opis = format!("Usunę wpis: {}", describe_draft(&draft));
    preview_or(args, opis.clone(), || {
        cthx_write::delete_item(&conn, id)?;
        journal::record(&format!("Usunięto {}", describe_draft(&draft)), &json!({"op": "reinsert_item", "id": id, "draft": cthx_write::draft_to_json(&draft)}))?;
        Ok(json!({"status": "usunieto", "opis": opis}))
    })
}

fn reschedule_item(args: &Value) -> Result<Value, String> {
    let id = args.get("id").and_then(Value::as_i64).ok_or("Podaj numer wpisu (id)")?;
    let new_day = str_arg(args, "data").filter(|s| !s.is_empty()).ok_or("Podaj nowy dzień")?;
    if !dates::valid_date(&new_day) {
        return Err("Nieprawidłowa data, podaj RRRR-MM-DD".into());
    }
    let conn = cthx_write::connect_write()?;
    let (before, source) = cthx_write::load_any(&conn, id)?;
    if source != "manual" {
        return Err("Ten wpis pochodzi z kontaktów, nie przenoszę go".into());
    }
    let changes = Changes {
        date: Some(new_day.clone()),
        start: str_arg(args, "godzina_start").filter(|s| !s.is_empty()),
        ..Default::default()
    };
    let after = cthx_write::apply_changes(before.clone(), changes);
    cthx_write::validate(&after)?;
    let opis = format!("Przeniosę „{}” z {} na {}", before.title, dates::pl_date(&before.date), dates::pl_date(&new_day));
    preview_or(args, opis.clone(), || {
        cthx_write::replace(&conn, id, &after)?;
        journal::record(
            &format!("Przeniesiono „{}”", before.title),
            &json!({"op": "restore_item", "id": id, "draft": cthx_write::draft_to_json(&before)}),
        )?;
        Ok(json!({"status": "zapisano", "id": id, "opis": opis}))
    })
}

fn undo_last(args: &Value) -> Result<Value, String> {
    let conn = journal::open()?;
    let Some((entry_id, desc, op)) = journal::last_undoable(&conn)? else {
        return Ok(json!({"status": "nic_do_cofniecia", "opis": "Nie ma zmian do cofnięcia"}));
    };
    let opis = format!("Cofnę: {desc}");
    preview_or(args, opis, || {
        undo::execute(&op)?;
        journal::mark_undone(&conn, entry_id)?;
        Ok(json!({"status": "cofnieto", "opis": format!("Cofnięto: {desc}")}))
    })
}

fn change_cash(args: &Value) -> Result<Value, String> {
    let kwota = args.get("kwota").and_then(Value::as_f64).ok_or("Podaj kwotę w złotych (ze znakiem)")?;
    if !kwota.is_finite() {
        return Err("Nieprawidłowa kwota".into());
    }
    let grosze = (kwota * 100.0).round() as i64;
    if grosze == 0 {
        return Err("Kwota nie może być zerem".into());
    }
    let powod = str_arg(args, "powod").filter(|s| !s.is_empty()).unwrap_or_else(|| "zmiana gotówki (Klaudiusz)".into());
    let conn = finance::connect_write()?;
    let current = finance::cash(&conn)?;
    let after = current + grosze;
    let sign = if grosze > 0 { "+" } else { "" };
    let mut opis = format!("Dodam wpis gotówki {sign}{} (powód: {powod}). Gotówka po zmianie: {}", pln(grosze), pln(after));
    if after < 0 {
        opis.push_str(". Uwaga: wyjdzie ujemna gotówka");
    }
    preview_or(args, opis.clone(), || {
        let id = finance::add_cash(&conn, grosze, &powod, dates::now_ms())?;
        journal::record(&format!("Gotówka {} ({powod})", pln(grosze)), &json!({"op": "delete_cash", "id": id}))?;
        Ok(json!({"status": "zapisano", "id": id, "opis": opis, "gotowka_po_zmianie": pln(finance::cash(&conn)?)}))
    })
}

fn set_alert(args: &Value) -> Result<Value, String> {
    let now = dates::now_ms();
    let day = str_arg(args, "data").filter(|s| !s.is_empty()).unwrap_or_else(dates::today);
    if !dates::valid_date(&day) {
        return Err("Nieprawidłowa data, podaj RRRR-MM-DD".into());
    }
    let (text, fire_at, item_id) = if let Some(id) = args.get("item_id").and_then(Value::as_i64) {
        let minuty = arg_i64(args, "minuty_przed", 10).clamp(0, 1440);
        let c = cthx::connect()?;
        let plan = cthx::plan(&c, &day)?;
        let item = plan.iter().find(|i| i.id == id).ok_or("Nie ma takiego wpisu w planie tego dnia")?;
        let start = item
            .godzina
            .as_deref()
            .and_then(|g| g.split('–').next())
            .map(str::to_string)
            .filter(|s| dates::valid_time(s))
            .ok_or("Ten wpis nie ma godziny rozpoczęcia, więc nie ma kiedy przypomnieć")?;
        (format!("Za {minuty} min: {}", item.tytul), dates::local_ms(&day, &start)? - minuty * 60_000, Some(id))
    } else {
        let text = str_arg(args, "tekst").filter(|s| !s.is_empty()).ok_or("Podaj treść przypomnienia (tekst)")?;
        if let Some(m) = args.get("za_minut").and_then(Value::as_i64) {
            if !(1..=10_080).contains(&m) {
                return Err("za_minut: od 1 do 10080 minut".into());
            }
            (text, now + m * 60_000, None)
        } else if let Some(hm) = str_arg(args, "o").filter(|s| !s.is_empty()) {
            if !dates::valid_time(&hm) {
                return Err("Godzinę podaj w formacie GG:MM".into());
            }
            (text, dates::local_ms(&day, &hm)?, None)
        } else {
            return Err("Podaj item_id, za_minut albo o (GG:MM)".into());
        }
    };
    if fire_at <= now {
        return Err("Ten czas już minął, podaj przyszły moment".into());
    }
    let local = dates::fmt_local(fire_at);
    let when = format!("{} {}", dates::pl_date(&local[..10]), &local[11..]);
    let opis = format!("Ustawię przypomnienie „{text}” na {when}");
    preview_or(args, opis.clone(), || {
        let conn = alerts::open()?;
        let id = alerts::add(&conn, &text, fire_at, item_id)?;
        journal::record(&format!("Przypomnienie: {text}"), &json!({"op": "delete_alert", "id": id}))?;
        Ok(json!({"status": "zapisano", "id": id, "opis": opis}))
    })
}

fn mark_item(args: &Value) -> Result<Value, String> {
    let id = args.get("id").and_then(Value::as_i64).ok_or("Podaj numer wpisu (id) z planu")?;
    let day = str_arg(args, "data").filter(|s| !s.is_empty()).unwrap_or_else(dates::today);
    if !dates::valid_date(&day) {
        return Err("Nieprawidłowa data, podaj RRRR-MM-DD".into());
    }
    let done = args.get("zrobione").and_then(Value::as_bool).unwrap_or(true);
    let conn = cthx_write::connect_write()?;
    let title = cthx_write::item_title(&conn, id)?;
    let opis = format!("{} „{title}” na {}", if done { "Odhaczę" } else { "Odznaczę" }, dates::pl_date(&day));
    preview_or(args, opis.clone(), || {
        let was = cthx_write::is_done(&conn, "item_done", "item_id", id, &day)?;
        cthx_write::set_item_done(&conn, id, &day, done)?;
        journal::record(&format!("Odhaczenie „{title}”"), &json!({"op": "set_done", "table": "item_done", "column": "item_id", "id": id, "date": day, "was_done": was}))?;
        Ok(json!({"status": "zapisano", "opis": opis}))
    })
}

fn mark_habit(args: &Value) -> Result<Value, String> {
    let name = str_arg(args, "nazwa").filter(|s| !s.is_empty()).ok_or("Podaj nazwę nawyku")?;
    let day = str_arg(args, "data").filter(|s| !s.is_empty()).unwrap_or_else(dates::today);
    if !dates::valid_date(&day) {
        return Err("Nieprawidłowa data, podaj RRRR-MM-DD".into());
    }
    let done = args.get("zrobione").and_then(Value::as_bool).unwrap_or(true);
    let conn = cthx_write::connect_write()?;
    let (habit_id, habit_name) = cthx_write::find_habit(&conn, &name)?;
    let opis = format!("{} nawyk „{habit_name}” na {}", if done { "Odhaczę" } else { "Odznaczę" }, dates::pl_date(&day));
    preview_or(args, opis.clone(), || {
        let was = cthx_write::is_done(&conn, "habit_done", "habit_id", habit_id, &day)?;
        cthx_write::set_habit_done(&conn, habit_id, &day, done)?;
        journal::record(&format!("Nawyk „{habit_name}”"), &json!({"op": "set_done", "table": "habit_done", "column": "habit_id", "id": habit_id, "date": day, "was_done": was}))?;
        Ok(json!({"status": "zapisano", "opis": opis}))
    })
}

fn plan_for(day: &str) -> Result<Value, String> {
    let c = cthx::connect()?;
    Ok(json!({"data": day, "pozycje": cthx::plan(&c, day)?}))
}

fn filter_plan(day: &str, kind: &str) -> Result<Value, String> {
    let c = cthx::connect()?;
    let items: Vec<_> = cthx::plan(&c, day)?.into_iter().filter(|i| i.rodzaj == kind).collect();
    Ok(json!({"data": day, "pozycje": items}))
}

/// Zbiorcze podsumowanie: każda część osobno, błąd jednej nie psuje reszty
fn daily_brief() -> Result<Value, String> {
    let part = |name: &str| run(name, &Value::Null).unwrap_or_else(|e| json!({"blad": e}));
    Ok(json!({
        "pogoda": part("Check_Weather"),
        "poczta": {"nieprzeczytane": part("Not_Yet_Read_Emails"), "pilne": part("Important_Emails")},
        "plan_dnia": part("Plans_For_Today"),
        "finanse": {"main": part("How_Much_On_Main"), "gotowka": part("Check_Cash")},
    }))
}

fn local_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "nieznana".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test na prawdziwych bazach (tylko odczyt). Wypisuje wyniki: `cargo test smoke -- --nocapture`.
    #[test]
    fn smoke_all_read_tools() {
        for def in definitions() {
            match run(def.name, &json!({"szukaj": "a", "minuty": 30})) {
                Ok(v) => println!("{} OK: {}", def.name, v),
                Err(e) => println!("{} BŁĄD: {}", def.name, e),
            }
        }
    }

    #[test]
    fn unknown_tool_is_an_error() {
        assert!(run("Nie_ma_takiego", &Value::Null).is_err());
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;

    /// Zapisy na KOPII bazy: ustaw KLAUDIUSZ_TEST_HOME na folder z kopią cthx.sqlite.
    /// Bez zmiennej test się pomija, więc prawdziwa baza CTHX nie jest dotykana.
    #[test]
    fn writes_need_confirmation_and_land_in_copy() {
        let Ok(home) = std::env::var("KLAUDIUSZ_TEST_HOME") else { return };
        std::env::set_var("HOME", home);
        let day = "2026-11-09";
        let args = |v: Value| v;

        // bez potwierdzenia: tylko podgląd, nic nie zapisane
        let before = cthx::plan(&cthx::connect().unwrap(), day).unwrap().len();
        let r = run("Add_Event", &args(json!({"tytul": "Test wydarzenie", "data": day, "godzina_start": "10:00", "godzina_koniec": "11:00"}))).unwrap();
        assert_eq!(r["status"], "podglad");
        assert_eq!(cthx::plan(&cthx::connect().unwrap(), day).unwrap().len(), before);

        // z potwierdzeniem: zapis
        let r = run("Add_Event", &args(json!({"tytul": "Test wydarzenie", "data": day, "godzina_start": "10:00", "godzina_koniec": "11:00", "potwierdzone": true}))).unwrap();
        assert_eq!(r["status"], "zapisano");
        let plan = cthx::plan(&cthx::connect().unwrap(), day).unwrap();
        let ev = plan.iter().find(|i| i.tytul == "Test wydarzenie").expect("wydarzenie w planie");

        // walidacja: koniec przed początkiem
        assert!(run("Add_Event", &args(json!({"tytul": "Zły", "data": day, "godzina_start": "12:00", "godzina_koniec": "10:00", "potwierdzone": true}))).is_err());

        // odhaczenie i odznaczenie
        run("Mark_Item_Done", &args(json!({"id": ev.id, "data": day, "potwierdzone": true}))).unwrap();
        assert!(cthx::plan(&cthx::connect().unwrap(), day).unwrap().iter().any(|i| i.id == ev.id && i.zrobione));
        run("Mark_Item_Done", &args(json!({"id": ev.id, "data": day, "zrobione": false, "potwierdzone": true}))).unwrap();

        // edycja tytułu i usunięcie godziny końca przy wydarzeniu wymaga też początku, więc zmieniamy tytuł i godzinę
        run("Edit_Event", &args(json!({"id": ev.id, "tytul": "Test zmieniony", "godzina_koniec": "12:00", "potwierdzone": true}))).unwrap();
        let edited = cthx::plan(&cthx::connect().unwrap(), day).unwrap().into_iter().find(|i| i.id == ev.id).unwrap();
        assert_eq!(edited.tytul, "Test zmieniony");
        assert_eq!(edited.godzina.as_deref(), Some("10:00–12:00"));
        // zły rodzaj wpisu jest odrzucany
        assert!(run("Edit_Task", &args(json!({"id": ev.id, "tytul": "x", "potwierdzone": true}))).is_err());

        // nawyk po nazwie (częściowej)
        let r = run("Mark_Habit_Done", &args(json!({"nazwa": "czytan", "data": day, "potwierdzone": true}))).unwrap();
        assert_eq!(r["status"], "zapisano");
        assert!(run("Mark_Habit_Done", &args(json!({"nazwa": "nie-ma-takiego", "potwierdzone": true}))).is_err());
    }
}

#[cfg(test)]
mod alert_tests {
    use super::*;

    /// Test podglądu i zapisu przypomnienia na podanej (kopii) bazie. Wymaga KLAUDIUSZ_TEST_HOME.
    #[test]
    fn set_alert_previews_then_saves() {
        let Ok(home) = std::env::var("KLAUDIUSZ_TEST_HOME") else { return };
        std::env::set_var("HOME", home);
        let r = run("Set_Alert", &json!({"tekst": "Test przypomnienia", "za_minut": 30})).unwrap();
        assert_eq!(r["status"], "podglad");
        let r = run("Set_Alert", &json!({"tekst": "Test przypomnienia", "za_minut": 30, "potwierdzone": true})).unwrap();
        assert_eq!(r["status"], "zapisano");
        let list = run("List_Alerts", &Value::Null).unwrap();
        assert!(list["przypomnienia"].as_array().unwrap().iter().any(|a| a["tekst"] == "Test przypomnienia"));
        assert!(run("Set_Alert", &json!({"tekst": "stare", "o": "00:01", "potwierdzone": true})).is_err());
    }
}

#[cfg(test)]
mod cash_tests {
    use super::*;

    /// Zapis na KOPII PFMX: ustaw KLAUDIUSZ_TEST_HOME na folder z kopią app.pfmx.finance/pfmx.sqlite.
    #[test]
    fn change_cash_previews_then_saves_manual_entry() {
        let Ok(home) = std::env::var("KLAUDIUSZ_TEST_HOME") else { return };
        std::env::set_var("HOME", home);
        let before = finance::cash(&finance::connect().unwrap()).unwrap();
        let r = run("Change_Cash", &json!({"kwota": -12.5, "powod": "test kawa"})).unwrap();
        assert_eq!(r["status"], "podglad");
        assert_eq!(finance::cash(&finance::connect().unwrap()).unwrap(), before);
        assert!(run("Change_Cash", &json!({"kwota": 0, "potwierdzone": true})).is_err());
        let r = run("Change_Cash", &json!({"kwota": -12.5, "powod": "test kawa", "potwierdzone": true})).unwrap();
        assert_eq!(r["status"], "zapisano");
        assert_eq!(finance::cash(&finance::connect().unwrap()).unwrap(), before - 1250);
    }
}

#[cfg(test)]
mod note_tests {
    use super::*;

    /// Notatka na KOPII bazy EINPX: ustaw KLAUDIUSZ_TEST_HOME (folder z app.einpx.dashboard/einpx.sqlite).
    #[test]
    fn quick_note_previews_then_saves_to_copy() {
        let Ok(home) = std::env::var("KLAUDIUSZ_TEST_HOME") else { return };
        std::env::set_var("HOME", home);
        let r = run("Quick_Note", &json!({"tekst": "test notatki z Klaudiusza"})).unwrap();
        assert_eq!(r["status"], "podglad");
        let r = run("Quick_Note", &json!({"tekst": "test notatki z Klaudiusza", "potwierdzone": true})).unwrap();
        assert_eq!(r["status"], "zapisano");
        let found = notes::search("test notatki z Klaudiusza", 5).unwrap();
        assert_eq!(found.len(), 1);
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;

    /// Cofanie na KOPII: KLAUDIUSZ_TEST_HOME z app.cthx.planner/cthx.sqlite. Klaudiusz zapisze swój dziennik w tym samym folderze.
    #[test]
    fn undo_reverts_added_task_after_confirmation() {
        let Ok(home) = std::env::var("KLAUDIUSZ_TEST_HOME") else { return };
        std::env::set_var("HOME", home);
        let day = "2026-11-12";
        let before = cthx::plan(&cthx::connect().unwrap(), day).unwrap().len();
        run("Add_Task", &json!({"tytul": "Test cofania", "data": day, "potwierdzone": true})).unwrap();
        assert_eq!(cthx::plan(&cthx::connect().unwrap(), day).unwrap().len(), before + 1);
        let preview = run("Undo_Last", &json!({})).unwrap();
        assert_eq!(preview["status"], "podglad");
        assert_eq!(cthx::plan(&cthx::connect().unwrap(), day).unwrap().len(), before + 1);
        let done = run("Undo_Last", &json!({"potwierdzone": true})).unwrap();
        assert_eq!(done["status"], "cofnieto");
        assert_eq!(cthx::plan(&cthx::connect().unwrap(), day).unwrap().len(), before);
        let log = run("Change_Log", &Value::Null).unwrap();
        assert!(log["dziennik"].as_array().unwrap().iter().any(|e| e["opis"].as_str().unwrap_or("").contains("Test cofania") && e["cofniete"] == true));
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;

    /// Usuwanie i przenoszenie na KOPII CTHX: ustaw KLAUDIUSZ_TEST_HOME.
    #[test]
    fn delete_and_reschedule_with_undo_on_copy() {
        let Ok(home) = std::env::var("KLAUDIUSZ_TEST_HOME") else { return };
        std::env::set_var("HOME", home);
        let day = "2026-11-13";
        run("Add_Task", &json!({"tytul": "Test usuwania", "data": day, "potwierdzone": true})).unwrap();
        let found = run("Check_Item", &json!({"szukaj": "Test usuwania"})).unwrap();
        let id = found["znalezione"][0]["id"].as_i64().expect("znaleziono");
        // przeniesienie na inny dzień (podgląd, potem zapis)
        assert_eq!(run("Reschedule_Item", &json!({"id": id, "data": "2026-11-14"})).unwrap()["status"], "podglad");
        run("Reschedule_Item", &json!({"id": id, "data": "2026-11-14", "potwierdzone": true})).unwrap();
        assert_eq!(cthx::plan(&cthx::connect().unwrap(), "2026-11-14").unwrap().iter().filter(|i| i.id == id).count(), 1);
        // usunięcie i cofnięcie
        assert_eq!(run("Delete_Item", &json!({"id": id})).unwrap()["status"], "podglad");
        run("Delete_Item", &json!({"id": id, "potwierdzone": true})).unwrap();
        assert!(cthx::plan(&cthx::connect().unwrap(), "2026-11-14").unwrap().iter().all(|i| i.id != id));
        run("Undo_Last", &json!({"potwierdzone": true})).unwrap();
        assert!(cthx::plan(&cthx::connect().unwrap(), "2026-11-14").unwrap().iter().any(|i| i.id == id));
    }
}
