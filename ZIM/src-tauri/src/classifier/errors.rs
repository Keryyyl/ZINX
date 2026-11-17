use crate::error::Error;

/// Dostawca modelu nie może teraz odpowiedzieć (nie wina wiadomości, więc nie liczymy próby):
/// - Down: brak połączenia, serwer Ollamy wyłączony albo limit zapytań
/// - NoModel: wybrany model nie istnieje u dostawcy
/// - NoKey: brak klucza API, klucz odrzucony albo wyczerpane środki
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Down,
    NoModel,
    NoKey,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    Unavailable(Reason, String),
    Other(String),
}

impl AiError {
    pub fn message(&self) -> &str {
        match self {
            AiError::Unavailable(_, m) | AiError::Other(m) => m,
        }
    }
}

impl From<AiError> for Error {
    fn from(e: AiError) -> Self {
        Error(e.message().to_string())
    }
}

impl From<Error> for AiError {
    fn from(e: Error) -> Self {
        AiError::Other(e.0)
    }
}

pub type AiResult<T> = std::result::Result<T, AiError>;
