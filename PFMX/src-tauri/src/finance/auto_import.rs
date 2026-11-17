//! Automatyczny odczyt eksportów z banku: skanuje folder, importuje nowe pliki `historia_*.csv`
//! i usuwa je dopiero po udanym imporcie. Pliki, których nie da się zaimportować, zostają i trafiają do raportu.

use super::import::import_millennium;
use crate::db::Db;
use crate::error::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Podfolder w „Dokumentach”, do którego bank zapisuje eksport.
pub const FOLDER: &str = "Millennium";
const PREFIX: &str = "historia_";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileOutcome {
    pub file_name: String,
    pub added: usize,
    pub skipped: usize,
    /// Czy plik został usunięty po imporcie
    pub deleted: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AutoImportReport {
    pub imported: Vec<FileOutcome>,
    pub errors: Vec<String>,
}

pub fn exports_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            p.is_file() && name.starts_with(PREFIX) && name.to_lowercase().ends_with(".csv")
        })
        .collect();
    // nazwy zawierają datę i godzinę, więc porządek alfabetyczny to kolejność czasu
    files.sort();
    files
}

/// Importuje wszystkie eksporty z folderu. Brak folderu to nie błąd: nic do odczytu.
pub fn auto_import_dir(conn: &rusqlite::Connection, dir: &Path) -> Result<AutoImportReport> {
    let mut report = AutoImportReport::default();
    for path in exports_in(dir) {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                report.errors.push(format!("{file_name}: nie udało się odczytać ({e})"));
                continue;
            }
        };
        match import_millennium(conn, &text, &file_name) {
            Ok(done) => {
                // dane są już w bazie, więc plik można usunąć; brak usunięcia nie cofa importu
                let deleted = std::fs::remove_file(&path).is_ok();
                report.imported.push(FileOutcome { file_name, added: done.added, skipped: done.skipped, deleted });
            }
            Err(e) => report.errors.push(format!("{file_name}: {}", e.0)),
        }
    }
    Ok(report)
}

/// Jedno przejście: czyta nowe eksporty, a potem zawsze wykrywa płatności cykliczne i odhacza je z transakcji.
/// Wykrywanie nie zależy od tego, czy przyszedł nowy plik, więc działa też przy każdym cyklu w tle.
pub fn import_and_detect(conn: &rusqlite::Connection, dir: &Path) -> Result<AutoImportReport> {
    let report = auto_import_dir(conn, dir)?;
    crate::finance::refresh_derived(conn)?;
    Ok(report)
}

/// Wersja dla tła: pracuje na połączeniu z bazy, a odczyt plików idzie poza wątek interfejsu.
pub async fn run_on(db: Db, dir: PathBuf) -> Result<AutoImportReport> {
    tokio::task::spawn_blocking(move || db.with(|conn| import_and_detect(conn, &dir)))
        .await
        .map_err(|e| crate::error::Error(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{finance as fin, open_memory};
    use crate::finance::millennium::testkit::export;

    const A: &str = "PL61 1090 1014 0000 0712 1981 2874";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zim-auto-{name}-{}", crate::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn imports_new_exports_and_deletes_only_those() {
        let conn = open_memory();
        fin::add_account(&conn, "Zwykłe", "bank", Some(&A.replace(' ', ""))).unwrap();
        let dir = scratch("ok");
        std::fs::write(dir.join("historia_20261104_190347.csv"), export(&[(A, "2026-10-01", "PRZELEW", "200.00"), (A, "2026-10-02", "ZAKUP", "-36.29")])).unwrap();
        std::fs::write(dir.join("notes.txt"), "zostaje").unwrap();
        std::fs::write(dir.join("._historia_20261104_190347.csv"), "appledouble").unwrap();

        let report = auto_import_dir(&conn, &dir).unwrap();
        assert_eq!(report.imported.len(), 1);
        assert_eq!((report.imported[0].added, report.imported[0].deleted), (2, true));
        assert!(report.errors.is_empty());
        assert!(!dir.join("historia_20261104_190347.csv").exists());
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("._historia_20261104_190347.csv").exists());

        // ponowne czytanie tego samego: nic nowego, ale brak pliku to nie błąd
        assert!(auto_import_dir(&conn, &dir).unwrap().imported.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn file_with_unknown_account_stays_and_is_reported() {
        let conn = open_memory();
        let dir = scratch("unknown");
        let path = dir.join("historia_1.csv");
        std::fs::write(&path, export(&[(A, "2026-10-01", "PRZELEW", "200.00")])).unwrap();
        let report = auto_import_dir(&conn, &dir).unwrap();
        assert!(report.imported.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert!(path.exists(), "plik bez konta nie może zniknąć");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detection_runs_even_without_new_files() {
        let conn = open_memory();
        let acc = fin::add_account(&conn, "Zwykłe", "bank", Some(&A.replace(' ', ""))).unwrap().id;
        for (i, (date, balance)) in [("2026-10-03", 1000), ("2026-11-03", 500)].into_iter().enumerate() {
            conn.execute(
                "INSERT INTO fin_transactions (account_id, booked_on, kind, counterparty, description, amount, currency, balance_after, source, dedup_key, imported_at)
                 VALUES (?1, ?2, 'ZAKUP', 'Spotify', '', -2300, 'PLN', ?3, 't', ?4, 0)",
                rusqlite::params![acc, date, balance, format!("sp-{i}")],
            )
            .unwrap();
        }
        let dir = scratch("nofiles");
        let report = import_and_detect(&conn, &dir).unwrap();
        assert!(report.imported.is_empty());
        assert_eq!(crate::db::recurring::list(&conn, "2026-11").unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_folder_is_not_an_error() {
        let conn = open_memory();
        let report = auto_import_dir(&conn, Path::new("/nie/ma/takiego/folderu")).unwrap();
        assert_eq!(report, AutoImportReport::default());
    }
}
