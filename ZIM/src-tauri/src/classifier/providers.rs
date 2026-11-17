//! Jedno wejście do modelu, niezależnie od dostawcy; zwraca JSON jako tekst.

use super::clients::{anthropic_chat, ollama_chat, openai_chat, ANTHROPIC_URL, OLLAMA_URL, OPENAI_URL};
use super::errors::{AiError, AiResult, Reason};
use crate::ai::is_cloud;
use crate::db::settings::AiConfig;
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

pub struct ChatRequest {
    pub config: AiConfig,
    pub system: String,
    pub user: String,
    /// JSON Schema odpowiedzi; każdy dostawca wymusza go po swojemu
    pub schema: Value,
}

pub type ChatFn = Arc<dyn Fn(ChatRequest) -> BoxFuture<'static, AiResult<String>> + Send + Sync>;

#[derive(Clone)]
pub struct Endpoints {
    pub ollama: String,
    pub anthropic: String,
    pub openai: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints { ollama: OLLAMA_URL.into(), anthropic: ANTHROPIC_URL.into(), openai: OPENAI_URL.into() }
    }
}

pub fn provider_label(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "Claude (Anthropic)",
        "openai" => "ChatGPT (OpenAI)",
        _ => "Pobrany model (Ollama)",
    }
}

/// `get_key` czyta klucz API z bezpiecznego magazynu (schowek systemowy); rdzeń nie wie, skąd on pochodzi.
pub fn create_chat(get_key: Arc<dyn Fn(&str) -> Option<String> + Send + Sync>, endpoints: Endpoints) -> ChatFn {
    Arc::new(move |req| {
        let (get_key, ep) = (get_key.clone(), endpoints.clone());
        Box::pin(async move {
            let ChatRequest { config, system, user, schema } = req;
            let (provider, model) = (config.provider.as_str(), config.model.as_str());
            if !is_cloud(provider) {
                return ollama_chat(&ep.ollama, model, &system, &user, &schema).await;
            }
            let Some(key) = get_key(provider) else {
                return Err(AiError::Unavailable(
                    Reason::NoKey,
                    format!("Brak klucza API: {}. Dodaj go w Ustawieniach → Asystent AI.", provider_label(provider)),
                ));
            };
            match provider {
                "anthropic" => anthropic_chat(&ep.anthropic, &key, model, &system, &user, &schema).await,
                _ => openai_chat(&ep.openai, &key, model, &system, &user, &schema).await,
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::http_mock::MockHttp;
    use super::*;
    use serde_json::json;

    fn request(provider: &str) -> ChatRequest {
        ChatRequest {
            config: AiConfig { enabled: true, provider: provider.into(), model: "m1".into() },
            system: "s".into(),
            user: "u".into(),
            schema: json!({"type":"object"}),
        }
    }

    #[tokio::test]
    async fn routes_each_provider_to_its_endpoint_with_its_key() {
        let ollama = MockHttp::start(vec![(200, r#"{"message":{"content":"o"}}"#.into())]).await;
        let anthropic = MockHttp::start(vec![(200, r#"{"content":[{"type":"text","text":"a"}]}"#.into())]).await;
        let openai = MockHttp::start(vec![(200, r#"{"choices":[{"message":{"content":"p"}}]}"#.into())]).await;
        let ep = Endpoints { ollama: ollama.url(), anthropic: anthropic.url(), openai: openai.url() };
        let keys: Arc<dyn Fn(&str) -> Option<String> + Send + Sync> = Arc::new(|p| Some(format!("key-{p}")));
        let chat = create_chat(keys, ep);
        assert_eq!(chat(request("ollama")).await.unwrap(), "o");
        assert_eq!(chat(request("anthropic")).await.unwrap(), "a");
        assert_eq!(chat(request("openai")).await.unwrap(), "p");
        assert_eq!(anthropic.requests()[0].header("x-api-key"), Some("key-anthropic"));
        assert_eq!(openai.requests()[0].header("authorization"), Some("Bearer key-openai"));
        assert!(ollama.requests()[0].header("x-api-key").is_none(), "lokalny model nie dostaje żadnych kluczy");
    }

    #[tokio::test]
    async fn cloud_without_a_key_is_unavailable_without_a_request() {
        let anthropic = MockHttp::start(vec![]).await;
        let ep = Endpoints { anthropic: anthropic.url(), ..Endpoints::default() };
        let chat = create_chat(Arc::new(|_| None), ep);
        let err = chat(request("anthropic")).await.unwrap_err();
        assert!(matches!(&err, AiError::Unavailable(Reason::NoKey, m) if m.contains("Claude")), "{err:?}");
        assert!(anthropic.requests().is_empty());
    }
}
