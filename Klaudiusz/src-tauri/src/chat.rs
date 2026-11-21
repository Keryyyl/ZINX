//! Czat w oknie: dyktowanie do pola i czat głosowy (ciągła rozmowa bez słowa aktywującego).
//! Wiadomości głosowe trafiają do okna zdarzeniem `chat-message`.

use crate::{audio, tools, tts, voice::{self, VoiceState}};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Czy czat głosowy działa (pętla w tle)
static VOICE_ON: AtomicBool = AtomicBool::new(false);

pub fn is_voice_on() -> bool {
    VOICE_ON.load(Ordering::SeqCst)
}

#[derive(Clone, Serialize)]
struct Message {
    role: &'static str,
    text: String,
}

fn emit(app: &AppHandle, role: &'static str, text: &str) {
    let _ = app.emit("chat-message", Message { role, text: text.to_string() });
}

/// Włącza albo wyłącza czat głosowy. Włączony działa jak nasłuch, ale bez słowa aktywującego.
pub fn set_voice(app: AppHandle, state: Arc<VoiceState>, on: bool) -> Result<(), String> {
    if !on {
        VOICE_ON.store(false, Ordering::SeqCst);
        audio::cancel_recording();
        return Ok(());
    }
    if voice::is_listening(&state) {
        return Err("Najpierw wyłącz „Słuchaj”".into());
    }
    if VOICE_ON.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    std::thread::spawn(move || voice_loop(&app, &state));
    Ok(())
}

/// Pętla czatu głosowego: nagrywa, wysyła rozpoznany tekst do modelu i czyta odpowiedź.
fn voice_loop(app: &AppHandle, state: &VoiceState) {
    while VOICE_ON.load(Ordering::SeqCst) {
        let heard = match voice::capture_text() {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            Err(e) => {
                emit(app, "error", &e);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        // wyłączenie w trakcie nagrywania: tekst z przerwanego nagrania odrzucamy
        if !VOICE_ON.load(Ordering::SeqCst) {
            break;
        }
        emit(app, "user", &heard);
        match voice::chat_turn(state, &heard, true) {
            Ok(answer) => {
                emit(app, "bot", &answer);
                let _ = tts::speak(&answer);
            }
            Err(e) => emit(app, "error", &e),
        }
    }
}

/// Polecenie widoczne po wpisaniu `/`: nazwa narzędzia i opis
#[derive(Serialize)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
}

/// Lista wszystkich narzędzi, które można wymusić z czatu
pub fn commands() -> Vec<Command> {
    let mut list: Vec<Command> = tools::definitions().into_iter().map(|d| Command { name: d.name, description: d.description }).collect();
    list.sort_by_key(|c| c.name);
    list
}

/// Jedno nagranie do pola tekstowego (bez wysyłania). `None` gdy nikt nic nie powiedział.
pub fn dictate() -> Result<Option<String>, String> {
    voice::capture_text()
}

#[cfg(test)]
mod command_tests {
    use crate::tools;

    #[test]
    fn command_names_ignore_case_and_separators() {
        assert_eq!(tools::find("daily-brief"), Some("Daily_Brief"));
        assert_eq!(tools::find("DAILY_BRIEF"), Some("Daily_Brief"));
        assert_eq!(tools::find("add event"), Some("Add_Event"));
        assert_eq!(tools::find("brie"), None);
    }
}
