//! Dostawcy asystenta AI: pobrany model (Ollama) albo zewnętrzny w chmurze.

pub const CLOUD_PROVIDERS: [&str; 2] = ["anthropic", "openai"];

pub fn is_provider(p: &str) -> bool {
    p == "ollama" || CLOUD_PROVIDERS.contains(&p)
}

pub fn is_cloud(p: &str) -> bool {
    p != "ollama"
}

/// Domyślny model: do tagowania w tle wybieramy najtańszy, bo zapytań są setki
pub fn default_model(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "claude-haiku-5-5",
        "openai" => "gpt-4o-mini",
        _ => "gemma3:4b",
    }
}
