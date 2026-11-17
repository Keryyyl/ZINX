//! Rozpoznawanie mowy: whisper.cpp (`whisper-cli`) z modelem `small`.

use std::path::{Path, PathBuf};
use std::process::Command;


const WHISPER_CANDIDATES: [&str; 2] = ["/opt/homebrew/bin/whisper-cli", "/usr/local/bin/whisper-cli"];

pub fn model_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/Klaudiusz/models/ggml-small-q5_1.bin")
}

pub fn transcribe(wav: &Path) -> Result<String, String> {
    let bin = WHISPER_CANDIDATES
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or("Nie znaleziono whisper-cli (zainstaluj: brew install whisper-cpp)")?;
    let model = model_path();
    if !model.exists() {
        return Err(format!("Brak modelu mowy: {}", model.display()));
    }
    let out = Command::new(bin)
        .args(["-m"])
        .arg(&model)
        .args(["-l", "pl", "-nt", "-np", "-f"])
        .arg(wav)
        .output()
        .map_err(|e| format!("whisper-cli: {e}"))?;
    if !out.status.success() {
        return Err(format!("whisper-cli zakończył się błędem: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
