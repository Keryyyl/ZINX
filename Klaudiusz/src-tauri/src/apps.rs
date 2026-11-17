//! Otwieranie aplikacji ZINX-a (tylko z tej listy).

use std::process::Command;

/// Aplikacje, które Klaudiusz może otworzyć
pub const OPENABLE: [&str; 4] = ["ZIM", "PFMX", "CTHX", "EINPX"];

pub fn open(app: &str) -> Result<(), String> {
    let name = OPENABLE
        .iter()
        .find(|a| a.eq_ignore_ascii_case(app.trim()))
        .ok_or_else(|| format!("Nie mogę otworzyć „{app}”. Dostępne: {}", OPENABLE.join(", ")))?;
    let status = Command::new("open").arg("-a").arg(name).status().map_err(|e| format!("Otwieranie: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Nie udało się otworzyć {name}. Sprawdź, czy jest w Aplikacjach."))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_apps_outside_the_list() {
        assert!(super::open("Safari").is_err());
        assert!(super::open("; rm -rf ~").is_err());
    }
}
