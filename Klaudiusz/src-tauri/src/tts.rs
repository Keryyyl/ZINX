//! Odczytywanie odpowiedzi na głos: wbudowane `say` z polskim głosem.
//! Wyciszenie zapisuje się w bazie Klaudiusza, więc działa też po ponownym uruchomieniu.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const VOICE: &str = "Zosia";
const MAX_CHARS: usize = 600;
/// Ile czekamy po wypowiedzi, zanim mikrofon znów zacznie nagrywać (żeby nie złapać echa głośników)
const ECHO_TAIL: Duration = Duration::from_millis(900);

static MUTED: AtomicBool = AtomicBool::new(false);
static SPEAKING: AtomicBool = AtomicBool::new(false);

/// Wczytuje zapisane wyciszenie (wołane przy starcie)
pub fn load_mute() {
    if let Ok(conn) = crate::journal::open() {
        MUTED.store(crate::journal::get_setting(&conn, "muted").as_deref() == Some("1"), Ordering::SeqCst);
    }
}

pub fn is_muted() -> bool {
    MUTED.load(Ordering::SeqCst)
}

pub fn set_muted(on: bool) {
    MUTED.store(on, Ordering::SeqCst);
    if let Ok(conn) = crate::journal::open() {
        let _ = crate::journal::set_setting(&conn, "muted", if on { "1" } else { "0" });
    }
}

/// Czy Klaudiusz właśnie mówi (mikrofon wtedy nie nagrywa)
pub fn is_speaking() -> bool {
    SPEAKING.load(Ordering::SeqCst)
}

pub fn speak(text: &str) -> Result<(), String> {
    if is_muted() {
        return Ok(());
    }
    let short: String = text.chars().take(MAX_CHARS).collect();
    SPEAKING.store(true, Ordering::SeqCst);
    let status = Command::new("say").args(["-v", VOICE]).arg(short).status();
    std::thread::sleep(ECHO_TAIL);
    SPEAKING.store(false, Ordering::SeqCst);
    let status = status.map_err(|e| format!("say: {e}"))?;
    if status.success() { Ok(()) } else { Err("say zakończył się błędem".into()) }
}
