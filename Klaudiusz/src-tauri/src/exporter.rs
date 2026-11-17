//! Uruchamianie Exportera (skrypt pobierający wyciągi z Millennium do CSV dla PFMX).
//! Skrypt sam otwiera przeglądarkę do logowania; logowanie wykonujesz Ty, Klaudiusz nie zna danych logowania.

use std::path::Path;
use std::process::{Command, Stdio};

pub const SCRIPT: &str = "/Volumes/KarolDysk/ZINX/Exporter/millennium_export.py";

/// Uruchamia skrypt w tle i zapisuje jego wyjście do dziennika w katalogu aplikacji
pub fn launch() -> Result<(), String> {
    if !Path::new(SCRIPT).exists() {
        return Err(format!("Nie znaleziono skryptu Exportera: {SCRIPT}"));
    }
    let log_path = crate::alerts::db_path().parent().map(|p| p.join("exporter.log")).ok_or("Brak katalogu aplikacji")?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| format!("Dziennik Exportera: {e}"))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;
    Command::new("python3")
        .arg(SCRIPT)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .map_err(|e| format!("Nie udało się uruchomić Exportera (python3): {e}"))?;
    Ok(())
}
