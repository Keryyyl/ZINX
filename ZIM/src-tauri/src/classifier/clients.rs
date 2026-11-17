//! Klienci modeli: Ollama (lokalnie), Anthropic i OpenAI (chmura). Każdy zwraca treść odpowiedzi (JSON jako tekst).

use super::errors::{AiError, AiResult, Reason};
use super::schema::strict_schema;
use serde_json::{json, Value};
use std::time::Duration;

pub const OLLAMA_URL: &str = "http://127.0.0.1:11434";
pub const ANTHROPIC_URL: &str = "https://api.anthropic.com";
pub const OPENAI_URL: &str = "https://api.openai.com";

const OLLAMA_TIMEOUT: Duration = Duration::from_secs(120);
const CLOUD_TIMEOUT: Duration = Duration::from_secs(60);
/// Miejsce także na krótkie „myślenie” modeli, które zawsze je włączają
const ANTHROPIC_MAX_TOKENS: u32 = 2048;
/// Modele rozumujące zużywają część limitu na myślenie, więc zostawiamy zapas
const OPENAI_MAX_COMPLETION_TOKENS: u32 = 4096;

fn client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder().timeout(timeout).build().unwrap_or_default()
}

/// Modele 5.x mają domyślnie średni wysiłek; do tagowania i skrótów wystarczy niski
fn supports_effort(model: &str) -> bool {
    ["haiku", "sonnet", "opus", "fable", "mythos"].iter().any(|f| model.starts_with(&format!("claude-{f}-5")))
}

/// Zwraca treść odpowiedzi (JSON jako tekst).
pub async fn ollama_chat(base: &str, model: &str, system: &str, user: &str, schema: &Value) -> AiResult<String> {
    let body = json!({
        "model": model,
        "stream": false,
        "think": false, // modele z trybem „myślenia” odpowiadałyby wielokrotnie wolniej
        "keep_alive": "30m", // model zostaje w pamięci między mailami
        "options": { "temperature": 0 },
        "format": schema,
        "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }]
    });
    let res = client(OLLAMA_TIMEOUT).post(format!("{base}/api/chat")).json(&body).send().await.map_err(|e| {
        if e.is_timeout() {
            AiError::Other("Model nie odpowiedział w ciągu 2 minut".into())
        } else {
            AiError::Unavailable(Reason::Down, "Nie można połączyć się z Ollamą. Uruchom ją (ollama serve).".into())
        }
    })?;
    let status = res.status();
    if !status.is_success() {
        if status.as_u16() == 404 {
            return Err(AiError::Unavailable(Reason::NoModel, format!("Brak modelu {model}. Pobierz go poleceniem: ollama pull {model}")));
        }
        let text = res.text().await.unwrap_or_default();
        return Err(AiError::Other(format!("Ollama zwróciła błąd {}: {}", status.as_u16(), text.chars().take(200).collect::<String>())));
    }
    let data: Value = res.json().await.map_err(|e| AiError::Other(format!("Niepoprawna odpowiedź Ollamy: {e}")))?;
    Ok(data["message"]["content"].as_str().unwrap_or("").to_string())
}

/// Zainstalowane modele; None = Ollama nie odpowiada.
pub async fn list_models(base: &str) -> Option<Vec<String>> {
    let res = client(Duration::from_secs(2)).get(format!("{base}/api/tags")).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let data: Value = res.json().await.ok()?;
    Some(data["models"].as_array()?.iter().filter_map(|m| m["name"].as_str().map(str::to_string)).collect())
}

pub async fn anthropic_chat(base: &str, api_key: &str, model: &str, system: &str, user: &str, schema: &Value) -> AiResult<String> {
    let mut output_config = json!({ "format": { "type": "json_schema", "schema": strict_schema(schema) } });
    if supports_effort(model) {
        output_config["effort"] = json!("low");
    }
    let body = json!({
        "model": model,
        "max_tokens": ANTHROPIC_MAX_TOKENS,
        "system": system,
        "messages": [{ "role": "user", "content": user }],
        "output_config": output_config
    });
    let res = client(CLOUD_TIMEOUT)
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|_| AiError::Unavailable(Reason::Down, "Nie można połączyć się z Anthropic. Sprawdź internet.".into()))?;
    let status = res.status().as_u16();
    let data: Value = res.json().await.unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = data["error"]["message"].as_str().unwrap_or("").to_string();
        return Err(match status {
            401 | 403 => AiError::Unavailable(Reason::NoKey, "Anthropic odrzucił klucz API. Sprawdź go w Ustawieniach → Asystent AI.".into()),
            404 => AiError::Unavailable(Reason::NoModel, format!("Model {model} nie istnieje albo konto nie ma do niego dostępu.")),
            429 => AiError::Unavailable(Reason::Down, "Anthropic: przekroczono limit zapytań. Spróbuję ponownie przy następnej synchronizacji.".into()),
            400 if detail.to_lowercase().contains("credit balance") => AiError::Unavailable(Reason::NoKey, "Na koncie Anthropic skończyły się środki.".into()),
            _ => AiError::Other(format!("Anthropic zwrócił błąd {status}: {}", detail.chars().take(200).collect::<String>())),
        });
    }
    match data["stop_reason"].as_str() {
        Some("refusal") => return Err(AiError::Other("Model odmówił odpowiedzi na tę wiadomość".into())),
        Some("max_tokens") => return Err(AiError::Other("Odpowiedź modelu została ucięta".into())),
        _ => {}
    }
    // bloki „thinking” pomijamy; interesuje nas tylko tekst
    let text: String = data["content"]
        .as_array()
        .map(|blocks| blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect())
        .unwrap_or_default();
    Ok(text)
}

pub async fn openai_chat(base: &str, api_key: &str, model: &str, system: &str, user: &str, schema: &Value) -> AiResult<String> {
    let mut body = json!({
        "model": model,
        "max_completion_tokens": OPENAI_MAX_COMPLETION_TOKENS,
        "response_format": { "type": "json_schema", "json_schema": { "name": "reply", "strict": true, "schema": strict_schema(schema) } },
        "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }]
    });
    // modele rozumujące (o*, gpt-5) nie przyjmują temperatury; rodzina gpt-4 tak
    if model.starts_with("gpt-4") {
        body["temperature"] = json!(0);
    }
    let res = client(CLOUD_TIMEOUT)
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|_| AiError::Unavailable(Reason::Down, "Nie można połączyć się z OpenAI. Sprawdź internet.".into()))?;
    let status = res.status().as_u16();
    let data: Value = res.json().await.unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = data["error"]["message"].as_str().map_or_else(|| format!("błąd {status}"), str::to_string);
        return Err(match status {
            401 | 403 => AiError::Unavailable(Reason::NoKey, "OpenAI odrzuciło klucz API. Sprawdź go w Ustawieniach → Asystent AI.".into()),
            404 => AiError::Unavailable(Reason::NoModel, format!("Model {model} nie istnieje albo konto nie ma do niego dostępu.")),
            429 if data["error"]["code"] == "insufficient_quota" => AiError::Unavailable(Reason::NoKey, "Na koncie OpenAI skończyły się środki.".into()),
            429 => AiError::Unavailable(Reason::Down, "OpenAI: przekroczono limit zapytań. Spróbuję ponownie przy następnej synchronizacji.".into()),
            _ => AiError::Other(format!("OpenAI zwróciło błąd {status}: {}", detail.chars().take(200).collect::<String>())),
        });
    }
    let choice = &data["choices"][0];
    if choice["message"]["refusal"].as_str().is_some_and(|r| !r.is_empty()) {
        return Err(AiError::Other("Model odmówił odpowiedzi na tę wiadomość".into()));
    }
    if choice["finish_reason"] == "length" {
        return Err(AiError::Other("Odpowiedź modelu została ucięta".into()));
    }
    Ok(choice["message"]["content"].as_str().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::super::http_mock::MockHttp;
    use super::*;

    fn schema() -> Value {
        json!({"type":"object","properties":{"tags":{"type":"array","maxItems":1,"items":{"type":"string"}}},"required":["tags"]})
    }

    #[tokio::test]
    async fn ollama_sends_schema_and_returns_content() {
        let http = MockHttp::start(vec![(200, r#"{"message":{"content":"{\"tags\":[]}"}}"#.into())]).await;
        let out = ollama_chat(&http.url(), "gemma3:4b", "sys", "usr", &schema()).await.unwrap();
        assert_eq!(out, r#"{"tags":[]}"#);
        let req = &http.requests()[0];
        assert_eq!(req.path, "/api/chat");
        assert_eq!(req.body["model"], "gemma3:4b");
        assert_eq!(req.body["think"], false);
        assert_eq!(req.body["format"]["properties"]["tags"]["maxItems"], 1, "Ollama dostaje schemat z limitami");
        assert_eq!(req.body["messages"][1]["content"], "usr");
    }

    #[tokio::test]
    async fn ollama_errors_are_classified() {
        let http = MockHttp::start(vec![(404, "{}".into()), (500, "boom".into())]).await;
        let e = ollama_chat(&http.url(), "m", "s", "u", &schema()).await.unwrap_err();
        assert!(matches!(e, AiError::Unavailable(Reason::NoModel, ref m) if m.contains("ollama pull m")));
        let e = ollama_chat(&http.url(), "m", "s", "u", &schema()).await.unwrap_err();
        assert!(matches!(e, AiError::Other(ref m) if m.contains("500")));
        let down = ollama_chat("http://127.0.0.1:1", "m", "s", "u", &schema()).await.unwrap_err();
        assert!(matches!(down, AiError::Unavailable(Reason::Down, _)));
    }

    #[tokio::test]
    async fn lists_models_or_none() {
        let http = MockHttp::start(vec![(200, r#"{"models":[{"name":"gemma3:4b"},{"name":"llama3"}]}"#.into())]).await;
        assert_eq!(list_models(&http.url()).await, Some(vec!["gemma3:4b".into(), "llama3".into()]));
        assert_eq!(list_models("http://127.0.0.1:1").await, None);
    }

    #[tokio::test]
    async fn anthropic_request_shape_and_text_blocks() {
        let reply = r#"{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"{\"tags\":[\"A\"]}"}]}"#;
        let http = MockHttp::start(vec![(200, reply.into()), (200, reply.into())]).await;
        let out = anthropic_chat(&http.url(), "sk-ant-test", "claude-haiku-5-5", "sys", "usr", &schema()).await.unwrap();
        assert_eq!(out, r#"{"tags":["A"]}"#);
        let req = &http.requests()[0];
        assert_eq!(req.path, "/v1/messages");
        assert_eq!(req.header("x-api-key"), Some("sk-ant-test"));
        assert_eq!(req.header("anthropic-version"), Some("2023-06-01"));
        assert_eq!(req.body["system"], "sys");
        assert_eq!(req.body["output_config"]["effort"], "low");
        assert_eq!(req.body["output_config"]["format"]["type"], "json_schema");
        assert_eq!(req.body["output_config"]["format"]["schema"]["additionalProperties"], false);
        assert!(req.body["output_config"]["format"]["schema"]["properties"]["tags"].get("maxItems").is_none());
        assert!(req.body.get("temperature").is_none() && req.body.get("tool_choice").is_none());
        anthropic_chat(&http.url(), "k", "claude-3-haiku", "s", "u", &schema()).await.unwrap();
        assert!(http.requests()[1].body["output_config"].get("effort").is_none(), "starsze modele nie przyjmują effort");
    }

    #[tokio::test]
    async fn anthropic_errors_map_to_unavailable_reasons() {
        let credit = r#"{"error":{"message":"Your credit balance is too low"}}"#;
        let http = MockHttp::start(vec![
            (401, "{}".into()),
            (404, "{}".into()),
            (429, "{}".into()),
            (400, credit.into()),
            (200, r#"{"stop_reason":"refusal","content":[]}"#.into()),
            (200, r#"{"stop_reason":"max_tokens","content":[]}"#.into()),
            (500, r#"{"error":{"message":"oops"}}"#.into()),
        ])
        .await;
        let mut reasons = Vec::new();
        for _ in 0..7 {
            let e = anthropic_chat(&http.url(), "k", "claude-haiku-5-5", "s", "u", &schema()).await.unwrap_err();
            reasons.push(match e {
                AiError::Unavailable(r, _) => format!("{r:?}"),
                AiError::Other(m) => m,
            });
        }
        assert_eq!(reasons[..4], ["NoKey", "NoModel", "Down", "NoKey"]);
        assert!(reasons[4].contains("odmówił") && reasons[5].contains("ucięta") && reasons[6].contains("500"), "{reasons:?}");
        let down = anthropic_chat("http://127.0.0.1:1", "k", "m", "s", "u", &schema()).await.unwrap_err();
        assert!(matches!(down, AiError::Unavailable(Reason::Down, _)));
    }

    #[tokio::test]
    async fn openai_request_shape_and_errors() {
        let ok = r#"{"choices":[{"message":{"content":"{\"tags\":[]}"},"finish_reason":"stop"}]}"#;
        let http = MockHttp::start(vec![
            (200, ok.into()),
            (200, ok.into()),
            (401, "{}".into()),
            (404, "{}".into()),
            (429, r#"{"error":{"code":"insufficient_quota","message":"x"}}"#.into()),
            (429, r#"{"error":{"code":"rate_limit_exceeded"}}"#.into()),
            (200, r#"{"choices":[{"message":{"content":null,"refusal":"nie"},"finish_reason":"stop"}]}"#.into()),
            (200, r#"{"choices":[{"message":{"content":"{"},"finish_reason":"length"}]}"#.into()),
        ])
        .await;
        assert_eq!(openai_chat(&http.url(), "sk-test", "gpt-4o-mini", "sys", "usr", &schema()).await.unwrap(), r#"{"tags":[]}"#);
        let req = &http.requests()[0];
        assert_eq!(req.path, "/v1/chat/completions");
        assert_eq!(req.header("authorization"), Some("Bearer sk-test"));
        assert_eq!(req.body["temperature"], 0);
        assert_eq!(req.body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(req.body["messages"][0]["role"], "system");
        openai_chat(&http.url(), "k", "gpt-5", "s", "u", &schema()).await.unwrap();
        assert!(http.requests()[1].body.get("temperature").is_none(), "modele rozumujące nie przyjmują temperatury");

        let mut got = Vec::new();
        for _ in 0..6 {
            got.push(match openai_chat(&http.url(), "k", "gpt-4o-mini", "s", "u", &schema()).await.unwrap_err() {
                AiError::Unavailable(r, _) => format!("{r:?}"),
                AiError::Other(m) => m,
            });
        }
        assert_eq!(got[..4], ["NoKey", "NoModel", "NoKey", "Down"]);
        assert!(got[4].contains("odmówił") && got[5].contains("ucięta"), "{got:?}");
    }

    /// Prawdziwy lokalny serwer Ollamy: `cargo test -- --ignored real_ollama`
    #[tokio::test]
    #[ignore]
    async fn real_ollama_returns_schema_conforming_json() {
        let models = list_models(OLLAMA_URL).await.expect("Ollama nie odpowiada");
        let model = models.iter().find(|m| m.starts_with("gemma3")).or(models.first()).expect("brak modeli").clone();
        let schema = json!({"type":"object","properties":{"tags":{"type":"array","maxItems":1,"items":{"type":"string","enum":["Faktury","Praca"]}}},"required":["tags"]});
        let out = ollama_chat(OLLAMA_URL, &model, "Wybierz tag. Odpowiadasz wyłącznie w JSON.", "Temat: Faktura za prąd nr 12/2026, do zapłaty do 15.11", &schema).await.unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        println!("model {model} odpowiedział: {out}");
        assert!(parsed["tags"].is_array());
    }
}
