//! Jedna tura rozmowy: nagranie → rozpoznanie mowy → model z narzędziami → odczytanie odpowiedzi.
//! Uruchamiana z paska menu. W jednej chwili działa tylko jedna tura.

use crate::{alerts, audio, llm, stt, tools, tts};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Jak długo czeka na „tak” / „nie” po podglądzie
pub const CONFIRM_WINDOW: Duration = Duration::from_secs(60);

pub struct VoiceState {
    busy: AtomicBool,
    /// historia rozmowy z modelem (żeby „tak” odnosiło się do poprzedniego pytania)
    history: Mutex<Vec<Value>>,
    /// zmiana czekająca na zgodę (jedna naraz)
    pending: Mutex<Option<Pending>>,
    /// czy nasłuch ciągły jest włączony
    listening: AtomicBool,
    /// czy pętla nasłuchu już działa (żeby nie uruchomić dwóch naraz)
    loop_running: AtomicBool,
}

struct Pending {
    preview: llm::Preview,
    since: Instant,
}

impl VoiceState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            busy: AtomicBool::new(false),
            history: Mutex::new(Vec::new()),
            pending: Mutex::new(None),
            listening: AtomicBool::new(false),
            loop_running: AtomicBool::new(false),
        })
    }
}

#[derive(Debug, PartialEq)]
enum Intent {
    Yes,
    No,
    Other,
}

/// Rozpoznaje krótkie „tak” / „nie” bez udziału modelu
fn classify(text: &str) -> Intent {
    let cleaned: String = text.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    const NO: [&str; 5] = ["nie", "anuluj", "anuluję", "odrzuć", "stop"];
    const YES: [&str; 5] = ["tak", "potwierdzam", "zgoda", "zgadzam", "ok"];
    if words.iter().any(|w| NO.contains(w)) {
        Intent::No
    } else if words.len() <= 4 && words.iter().any(|w| YES.contains(w)) {
        Intent::Yes
    } else {
        Intent::Other
    }
}

/// Obsługuje odpowiedź na czekający podgląd. `Some(tekst)` gdy odpowiedź dotyczyła zgody, `None` gdy to zwykłe pytanie.
fn handle_confirmation(state: &VoiceState, heard: &str) -> Option<String> {
    let mut guard = state.pending.lock().ok()?;
    let intent = classify(heard);
    if intent == Intent::Other {
        // nie kasujemy oczekującej zmiany przy zwykłym pytaniu, ale po czasie ją wygaszamy
        if guard.as_ref().is_some_and(|p| p.since.elapsed() > CONFIRM_WINDOW) {
            *guard = None;
        }
        return None;
    }
    let pending = guard.take()?;
    if pending.since.elapsed() > CONFIRM_WINDOW {
        return Some("Potwierdzenie wygasło, powtórz polecenie.".into());
    }
    match intent {
        Intent::No => Some("Dobrze, anulowane.".into()),
        Intent::Yes => {
            let mut args = pending.preview.args.clone();
            args["potwierdzone"] = Value::Bool(true);
            Some(match tools::run(&pending.preview.tool, &args) {
                Ok(result) => result["opis"].as_str().map_or_else(|| "Gotowe.".into(), |o| format!("Gotowe. {o}")),
                Err(e) => format!("Nie udało się: {e}"),
            })
        }
        Intent::Other => None,
    }
}

/// Uruchamia turę w tle. `on_busy` dostaje true na starcie i false na końcu, `on_text` każdą linijkę do pokazania.
pub fn spawn_turn(state: Arc<VoiceState>, on_busy: impl Fn(bool) + Send + Sync + 'static, on_text: impl Fn(&str) + Send + Sync + 'static) {
    std::thread::spawn(move || run_turn(&state, &on_busy, &on_text));
}

fn run_turn(state: &VoiceState, on_busy: &dyn Fn(bool), on_text: &dyn Fn(&str)) {
    if state.listening.load(Ordering::SeqCst) {
        on_text("Nasłuch jest włączony: mów „Klaudiusz” i polecenie");
        return;
    }
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    on_busy(true);
    if let Err(e) = turn(state, on_text) {
        on_text(&format!("Błąd: {e}"));
        eprintln!("Błąd tury: {e}");
    }
    on_busy(false);
    state.busy.store(false, Ordering::SeqCst);
}

/// Nagranie jednej wypowiedzi i rozpoznanie mowy. `None` gdy nikt nic nie powiedział.
fn capture_text() -> Result<Option<String>, String> {
    // nie nagrywamy, dopóki Klaudiusz mówi (żeby nie słyszeć własnego głosu)
    while tts::is_speaking() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let Some(samples) = audio::record_utterance()? else {
        return Ok(None);
    };
    let dir = alerts::db_path().parent().map(|p| p.to_path_buf()).unwrap_or_default();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Katalog: {e}"))?;
    let wav = dir.join("utterance.wav");
    audio::write_wav(&wav, &samples)?;
    let heard = stt::transcribe(&wav)?;
    Ok(if heard.trim().is_empty() { None } else { Some(heard) })
}

fn turn(state: &VoiceState, on_text: &dyn Fn(&str)) -> Result<(), String> {
    let Some(heard) = capture_text()? else {
        on_text("Nie słyszałem polecenia");
        return Ok(());
    };
    on_text(&format!("Ty: {heard}"));
    process(state, &heard, on_text)
}

/// Początki słowa aktywującego, bo rozpoznawanie potrafi zapisać je na kilka sposobów
const WAKE_PREFIXES: [&str; 5] = ["klaud", "klaw", "claud", "klau", "kłaud"];

/// Słowo aktywujące („Klaudiusz”, „Klaudi” i zbliżone zapisy). Zwraca resztę wypowiedzi, `None` gdy nie zwrócono się do Klaudiusza.
pub fn strip_wake_word(text: &str) -> Option<String> {
    let text = strip_noise_tags(text);
    let words: Vec<&str> = text.split_whitespace().collect();
    let idx = words.iter().position(|w| {
        let clean: String = w.to_lowercase().chars().filter(|c| c.is_alphabetic()).collect();
        WAKE_PREFIXES.iter().any(|p| clean.starts_with(p)) || is_close_to_wake_word(&clean)
    })?;
    let rest: Vec<&str> = words.iter().enumerate().filter(|(i, _)| *i != idx).map(|(_, w)| *w).collect();
    Some(rest.join(" "))
}

#[cfg(test)]
mod noise_tests {
    use super::*;

    #[test]
    fn whisper_noise_tags_are_ignored() {
        assert_eq!(strip_wake_word("[oklaski]"), None);
        assert_eq!(strip_wake_word("(muzyka) Klaudiusz ile mam gotówki").as_deref(), Some("ile mam gotówki"));
        assert_eq!(strip_noise_tags("[BLANK_AUDIO] tak"), " tak");
    }
}

/// Whisper czasem przekręca imię („Klaudiusz” → „Klaudios”, „Klodiusz”, „Kladiusz”). Liczymy odległość edycyjną.
fn is_close_to_wake_word(word: &str) -> bool {
    word.chars().count() >= 5 && (edit_distance(word, "klaudiusz") <= 3 || edit_distance(word, "klaudi") <= 2)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == *cb { 0 } else { 1 };
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Whisper dopisuje do ciszy i szumu znaczniki w nawiasach ([oklaski], (muzyka), [BLANK_AUDIO]). To nie mowa, więc je usuwamy.
fn strip_noise_tags(text: &str) -> String {
    let mut out = String::new();
    let mut depth_close: Option<char> = None;
    for c in text.chars() {
        match depth_close {
            Some(close) => {
                if c == close {
                    depth_close = None;
                }
            }
            None => match c {
                '[' => depth_close = Some(']'),
                '(' => depth_close = Some(')'),
                _ => out.push(c),
            },
        }
    }
    out
}

/// Dziennik rozpoznań (do diagnozy nasłuchu): `~/Library/Application Support/app.klaudiusz.assistant/listen.log`
pub fn log(line: &str) {
    use std::io::Write;
    let path = alerts::db_path().parent().map(|p| p.join("listen.log"));
    if let Some(path) = path {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let stamp = chrono::Local::now().format("%H:%M:%S");
            let _ = writeln!(f, "{stamp} {line}");
        }
    }
}

/// Pętla nasłuchu: czeka na słowo aktywujące, potem wykonuje polecenie. Działa, dopóki nasłuch jest włączony.
fn listen_loop(state: &VoiceState, on_busy: &dyn Fn(bool), on_text: &dyn Fn(&str)) {
    while state.listening.load(Ordering::SeqCst) {
        let heard = match capture_text() {
            Ok(Some(h)) => h,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("Nasłuch: {e}");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        log(&format!("usłyszano: {heard}"));
        let Some(rest) = strip_wake_word(&heard) else { continue };
        log(&format!("hasło wykryte, polecenie: {rest:?}"));
        if state.busy.swap(true, Ordering::SeqCst) {
            continue;
        }
        on_busy(true);
        let result = (|| -> Result<(), String> {
            let command = if rest.trim().is_empty() {
                // samo hasło: potwierdzamy i czekamy na polecenie
                tts::speak("Słucham")?;
                match capture_text()? {
                    Some(next) => next,
                    None => return Ok(()),
                }
            } else {
                rest
            };
            on_text(&format!("Ty: {command}"));
            process(state, &command, on_text)
        })();
        if let Err(e) = result {
            on_text(&format!("Błąd: {e}"));
        }
        on_busy(false);
        state.busy.store(false, Ordering::SeqCst);
    }
    state.loop_running.store(false, Ordering::SeqCst);
}

/// Włącza albo wyłącza nasłuch ciągły. Pętla kończy się po bieżącej wypowiedzi.
pub fn set_listening(state: Arc<VoiceState>, on: bool, on_busy: Arc<dyn Fn(bool) + Send + Sync>, on_text: Arc<dyn Fn(&str) + Send + Sync>) {
    state.listening.store(on, Ordering::SeqCst);
    if on && !state.loop_running.swap(true, Ordering::SeqCst) {
        std::thread::spawn(move || listen_loop(&state, on_busy.as_ref(), on_text.as_ref()));
    }
}

/// „cisza” / „wycisz” wycisza, „odcisz” albo „włącz głos” przywraca. Tylko krótkie polecenia.
pub fn mute_command(text: &str) -> Option<bool> {
    let cleaned: String = text.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() > 4 {
        return None;
    }
    if words.iter().any(|w| *w == "odcisz" || (*w == "włącz" && words.contains(&"głos"))) {
        Some(false)
    } else if words.iter().any(|w| *w == "cisza" || w.starts_with("wycisz")) {
        Some(true)
    } else {
        None
    }
}

/// Potwierdzenie albo odpowiedź modelu dla rozpoznanego tekstu
fn process(state: &VoiceState, heard: &str, on_text: &dyn Fn(&str)) -> Result<(), String> {
    if let Some(mute) = mute_command(heard) {
        tts::set_muted(mute);
        on_text(if mute { "Głos wyciszony" } else { "Głos włączony" });
        if !mute {
            tts::speak("Głos włączony")?;
        }
        return Ok(());
    }
    if let Some(answer) = handle_confirmation(state, heard) {
        on_text(&format!("Klaudiusz: {answer}"));
        tts::speak(&answer)?;
        return Ok(());
    }

    let started = Instant::now();
    let reply = {
        let mut history = state.history.lock().map_err(|_| "Historia rozmowy jest niedostępna")?;
        llm::reply(&mut history, heard)?
    };
    // czas modelu (do diagnozy opóźnień)
    log(&format!(
        "model: {:.1} s (rundy {:?}), narzędzia: {:?}",
        started.elapsed().as_secs_f32(),
        reply.rounds_s,
        reply.tools_used
    ));
    // gdy czeka zmiana, mówimy dokładny opis z narzędzia (model potrafi pomylić datę)
    let mut answer = reply.text;
    if let Some(preview) = reply.preview {
        on_text(&format!("Czeka na potwierdzenie (60 s): {}", preview.opis));
        answer = format!("{} Powiedz tak albo nie.", preview.opis);
        if let Ok(mut guard) = state.pending.lock() {
            *guard = Some(Pending { preview, since: Instant::now() });
        }
    }
    on_text(&format!("Klaudiusz: {answer}"));
    tts::speak(&answer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_yes_and_no() {
        assert_eq!(classify("Tak."), Intent::Yes);
        assert_eq!(classify("Dobrze, potwierdzam"), Intent::Yes);
        assert_eq!(classify("potwierdzam"), Intent::Yes);
        assert_eq!(classify("nie, anuluj"), Intent::No);
        assert_eq!(classify("nie tak"), Intent::No);
        assert_eq!(classify("ile mam gotówki"), Intent::Other);
        assert_eq!(mute_command("cisza"), Some(true));
        assert_eq!(mute_command("Odcisz"), Some(false));
        assert_eq!(mute_command("włącz głos"), Some(false));
        assert_eq!(mute_command("ile mam gotówki na koncie"), None);
    }

    /// Test etapu 3 na plikach: rozpoznanie gotowego nagrania i odpowiedź modelu z narzędziami (tylko odczyt).
    /// Wymaga działającej Ollamy. Uruchom: `cargo test pipeline_on_sample -- --nocapture`.
    #[test]
    fn pipeline_on_sample() {
        let Ok(wav) = std::env::var("KLAUDIUSZ_SAMPLE_WAV") else { return };
        let heard = stt::transcribe(std::path::Path::new(&wav)).unwrap();
        println!("rozpoznano: {heard}");
        let mut history = Vec::new();
        let reply = llm::reply(&mut history, &heard).unwrap();
        println!("odpowiedź: {}", reply.text);
        assert!(!reply.text.is_empty());
    }
}

#[cfg(test)]
mod wake_tests {
    use super::strip_wake_word;

    #[test]
    fn detects_wake_word_spellings() {
        assert_eq!(strip_wake_word("Klaudia, ile mam gotówki").as_deref(), Some("ile mam gotówki"));
        assert_eq!(strip_wake_word("Claudius ile").as_deref(), Some("ile"));
    }

    #[test]
    fn detects_wake_word_and_keeps_command() {
        assert_eq!(strip_wake_word("Klaudiusz, ile mam gotówki?").as_deref(), Some("ile mam gotówki?"));
        assert_eq!(strip_wake_word("hej Klaudi dodaj zadanie").as_deref(), Some("hej dodaj zadanie"));
        assert_eq!(strip_wake_word("Klaudiusz").as_deref(), Some(""));
        assert_eq!(strip_wake_word("ile mam gotówki"), None);
    }

    #[test]
    fn detects_misspelled_wake_word_from_log() {
        assert_eq!(strip_wake_word("Klodiusz ile mam").as_deref(), Some("ile mam"));
        assert_eq!(strip_wake_word("Kladiusz ile mam").as_deref(), Some("ile mam"));
        assert_eq!(strip_wake_word("Klaudios?").as_deref(), Some(""));
        assert_eq!(strip_wake_word("Pozwolnić"), None);
    }
}
