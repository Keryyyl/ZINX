//! Lokalny model (Ollama) proponuje kategorię wydatku. Odpowiedź musi być jednym z kluczy kategorii;
//! każda inna odpowiedź zostaje bez kategorii. Dane nie opuszczają komputera.

use crate::db::categorize::{self, AiCategory, Pending};
use crate::db::Db;
use crate::error::{Error, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

pub const OLLAMA_URL: &str = "http://127.0.0.1:11434";
pub const MODEL: &str = "gemma3:4b";
/// Ile przykładów z poprawek dajemy modelowi w każdym zapytaniu
const EXAMPLES: usize = 20;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub available: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PassReport {
    /// ile transakcji dostało kategorię w tym przebiegu
    pub tagged: usize,
    /// ile wydatków wciąż czeka na kategorię
    pub remaining: i64,
}

/// Opis kategorii dla modelu: klucz, nazwa i wskazówka, czego tu szukać.
fn hint(key: &str) -> &'static str {
    match key {
        "fixed" => "rachunki (prąd, gaz, czynsz, telefon, internet) i subskrypcje (streaming, aplikacje, członkostwa)",
        "food" => "sklepy spożywcze i zakupy na jedzenie (Biedronka, Lidl, Carrefour, Aldi, Auchan)",
        "transport" => "przejazdy (Uber, Bolt, Jakdojade, bilety, karta miejska, paliwo, parkingi)",
        "outings" => "wyjścia i rozrywka: kawiarnie, bary, restauracje, Żabka i małe sklepy na miejscu, kino, hobby, podróże",
        "oneoff" => "zdrowie (apteka, lekarz), dom (chemia, wyposażenie), ubrania, prezenty i wsparcie dla innych",
        "development" => "edukacja: kursy, szkoły, książki, platformy nauki",
        _ => "",
    }
}

pub fn system_prompt(categories: &[AiCategory], examples: &[(String, String)]) -> String {
    let mut out = String::from(
        "Przypisujesz wydatki z polskiego konta osobistego do jednej kategorii. \
         Dostajesz odbiorcę i opis transakcji. Wybierz dokładnie jeden klucz z listy poniżej. \
         Jeśli nie jesteś pewien, zwróć null. Odpowiadasz wyłącznie w formacie JSON: {\"category\": \"klucz\"} albo {\"category\": null}.\n\nKategorie:\n",
    );
    for c in categories {
        out.push_str(&format!("- {}: {} ({})\n", c.key, c.name, hint(&c.key)));
    }
    if !examples.is_empty() {
        out.push_str("\nPrzykłady od użytkownika (odbiorca -> kategoria):\n");
        for (merchant, category) in examples {
            out.push_str(&format!("- {merchant} -> {category}\n"));
        }
    }
    out
}

pub fn user_prompt(p: &Pending) -> String {
    let counterparty = if p.counterparty.trim().is_empty() { "(brak)" } else { p.counterparty.trim() };
    format!(
        "Odbiorca: {counterparty}\nOpis: {}\nKwota: {}.{:02} zł",
        p.description.trim(),
        p.amount / 100,
        p.amount % 100
    )
}

/// Schemat odpowiedzi: model może zwrócić wyłącznie klucz z listy albo null.
pub fn answer_schema(keys: &[&str]) -> Value {
    let mut choices: Vec<Value> = keys.iter().map(|k| json!(k)).collect();
    choices.push(Value::Null);
    json!({
        "type": "object",
        "properties": { "category": { "type": ["string", "null"], "enum": choices } },
        "required": ["category"]
    })
}

/// Wyciąga klucz z odpowiedzi; nieznany klucz albo zły format to brak kategorii.
pub fn parse_answer(content: &str, keys: &[&str]) -> Option<String> {
    let value: Value = serde_json::from_str(content.trim()).ok()?;
    let key = value.get("category")?.as_str()?;
    keys.contains(&key).then(|| key.to_string())
}

fn client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder().timeout(timeout).build().unwrap_or_default()
}

/// Czy Ollama działa i ma pobrany model.
pub async fn status() -> AiStatus {
    let res = match client(Duration::from_secs(2)).get(format!("{OLLAMA_URL}/api/tags")).send().await {
        Ok(res) if res.status().is_success() => res,
        _ => {
            return AiStatus { available: false, message: "Ollama nie działa. Uruchom ją, a kategorie dopiszą się same.".into() };
        }
    };
    let data: Value = res.json().await.unwrap_or(Value::Null);
    let models: Vec<String> = data["models"]
        .as_array()
        .map(|list| list.iter().filter_map(|m| m["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if models.iter().any(|m| m == MODEL || m.strip_prefix(MODEL).is_some_and(|rest| rest.is_empty())) {
        AiStatus { available: true, message: format!("Model {MODEL} gotowy") }
    } else {
        AiStatus { available: false, message: format!("Brak modelu {MODEL}. Pobierz go: ollama pull {MODEL}") }
    }
}

/// Jedno zapytanie do modelu. Zwraca treść odpowiedzi.
async fn ask(system: &str, user: &str, schema: &Value) -> Result<String> {
    let body = json!({
        "model": MODEL,
        "stream": false,
        "think": false,
        "keep_alive": "30m",
        "options": { "temperature": 0 },
        "format": schema,
        "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }]
    });
    let res = client(Duration::from_secs(120))
        .post(format!("{OLLAMA_URL}/api/chat"))
        .json(&body)
        .send()
        .await
        .map_err(|e| Error(format!("Nie udało się połączyć z Ollamą: {e}")))?;
    if !res.status().is_success() {
        return Err(Error(format!("Ollama zwróciła błąd {}", res.status().as_u16())));
    }
    let data: Value = res.json().await.map_err(|e| Error(format!("Niepoprawna odpowiedź Ollamy: {e}")))?;
    Ok(data["message"]["content"].as_str().unwrap_or("").to_string())
}

/// Przetwarza do `limit` wydatków bez kategorii. Przerywa się, gdy model przestaje odpowiadać.
pub async fn run_pass(db: Db, limit: usize) -> Result<PassReport> {
    let status = status().await;
    if !status.available {
        return Err(Error(status.message));
    }
    let (categories, examples, pending) = db.with(|c| {
        Ok((categorize::ai_categories(c)?, categorize::examples(c, EXAMPLES)?, categorize::pending(c, limit)?))
    })?;
    let keys: Vec<&str> = categories.iter().map(|c| c.key.as_str()).collect();
    let system = system_prompt(&categories, &examples);
    let schema = answer_schema(&keys);
    let mut tagged = 0;
    for p in pending {
        let content = ask(&system, &user_prompt(&p), &schema).await?;
        let envelope = parse_answer(&content, &keys).and_then(|key| categories.iter().find(|c| c.key == key).map(|c| c.id));
        if envelope.is_some() {
            tagged += 1;
        }
        db.with(|c| categorize::store_ai(c, p.id, envelope))?;
    }
    let remaining = db.with(|c| categorize::count_untagged_pending(c))?;
    Ok(PassReport { tagged, remaining })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats() -> Vec<AiCategory> {
        vec![
            AiCategory { id: 2, key: "food".into(), name: "Jedzenie".into() },
            AiCategory { id: 5, key: "transport".into(), name: "Transport".into() },
        ]
    }

    #[test]
    fn only_listed_keys_are_accepted() {
        let keys = ["food", "transport"];
        assert_eq!(parse_answer(r#"{"category": "food"}"#, &keys).as_deref(), Some("food"));
        assert_eq!(parse_answer(r#"{"category": "wakacje"}"#, &keys), None);
        assert_eq!(parse_answer(r#"{"category": null}"#, &keys), None);
        assert_eq!(parse_answer("nie JSON", &keys), None);
        assert_eq!(parse_answer(r#"{"kategoria": "food"}"#, &keys), None);
    }

    #[test]
    fn schema_limits_the_answer_to_known_keys_or_null() {
        let schema = answer_schema(&["food", "transport"]);
        let choices = schema["properties"]["category"]["enum"].as_array().unwrap();
        assert_eq!(choices.len(), 3);
        assert!(choices.contains(&Value::Null));
    }

    #[test]
    fn prompt_lists_categories_and_user_examples_without_account_numbers() {
        let prompt = system_prompt(&cats(), &[("BIEDRONKA".into(), "Jedzenie".into())]);
        assert!(prompt.contains("food: Jedzenie"));
        assert!(prompt.contains("BIEDRONKA -> Jedzenie"));
        let user = user_prompt(&Pending { id: 1, counterparty: "Uber".into(), description: "UBER ONE".into(), amount: 1299 });
        assert!(user.contains("Odbiorca: Uber"));
        assert!(user.contains("12.99 zł"));
        assert!(!user.contains("PL"), "numer rachunku nie trafia do modelu");
    }

    /// Prawdziwa Ollama i model z dysku: `cargo test -- --ignored real_model`. Sprawdza format odpowiedzi.
    #[test]
    #[ignore]
    fn real_model_answers_in_the_expected_format() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let st = status().await;
            assert!(st.available, "{}", st.message);
            let categories = cats();
            let keys: Vec<&str> = categories.iter().map(|c| c.key.as_str()).collect();
            let system = system_prompt(&categories, &[]);
            let schema = answer_schema(&keys);
            for (counterparty, description, expected) in [("Biedronka", "ZAKUP - FIZ. UŻYCIE KARTY", Some("food")), ("Uber", "UBER TRIP", Some("transport"))] {
                let p = Pending { id: 1, counterparty: counterparty.into(), description: description.into(), amount: 1000 };
                let content = ask(&system, &user_prompt(&p), &schema).await.unwrap();
                let got = parse_answer(&content, &keys);
                println!("{counterparty}: {content} -> {got:?}");
                assert_eq!(got.as_deref(), expected);
            }
        });
    }
}
