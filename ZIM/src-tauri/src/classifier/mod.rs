//! Asystent AI: tagowanie wiadomości i skróty. Treść maila jest daną, nie poleceniem: model nie wykonuje
//! żadnych akcji, może tylko wybrać tagi z listy albo streścić tekst.

pub mod clients;
pub mod errors;
pub mod llm;
pub mod ollama_server;
pub mod providers;
pub mod queue;
pub mod schema;
pub mod summary;

#[cfg(test)]
pub mod http_mock;
