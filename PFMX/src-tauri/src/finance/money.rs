use crate::error::{Error, Result};

/// Kwota w groszach z eksportu banku: "-1177.00" -> -117700. Tylko zapis z kropką i dwoma miejscami po niej.
pub fn parse_grosze(text: &str) -> Result<i64> {
    let t = text.trim();
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let bad = || Error(format!("Nieprawidłowa kwota: {text}"));
    let (whole, cents) = body.split_once('.').ok_or_else(bad)?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || cents.len() != 2 || !digits(cents) {
        return Err(bad());
    }
    let whole: i64 = whole.parse().map_err(|_| bad())?;
    let value = whole
        .checked_mul(100)
        .and_then(|v| v.checked_add(cents.parse::<i64>().ok()?))
        .ok_or_else(bad)?;
    Ok(if negative { -value } else { value })
}

/// Do wyświetlenia i komunikatów: -117700 -> "-1177.00"
pub fn format_grosze(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", value.unsigned_abs() / 100, value.unsigned_abs() % 100)
}

/// Numer rachunku bez spacji, wielkie litery. Sprawdza tylko kształt, nie sumę kontrolną.
pub fn normalize_iban(raw: &str) -> Result<String> {
    let clean: String = raw.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    let plausible = (15..=34).contains(&clean.len())
        && clean.chars().all(|c| c.is_ascii_alphanumeric())
        && clean[..2].chars().all(|c| c.is_ascii_alphabetic());
    if !plausible {
        return Err("Nieprawidłowy numer rachunku (IBAN)".into());
    }
    Ok(clean)
}

/// Do komunikatów i logów: sam koniec numeru, nigdy cały rachunek.
pub fn mask_iban(iban: &str) -> String {
    let n = iban.len();
    if n > 4 {
        format!("…{}", &iban[n - 4..])
    } else {
        "…".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bank_amounts_exactly() {
        assert_eq!(parse_grosze("-1177.00").unwrap(), -117_700);
        assert_eq!(parse_grosze("150.00").unwrap(), 15_000);
        assert_eq!(parse_grosze("-13.99").unwrap(), -1_399);
        assert_eq!(parse_grosze("0.05").unwrap(), 5);
        assert_eq!(parse_grosze(" 2.61 ").unwrap(), 261);
    }

    #[test]
    fn rejects_other_formats() {
        for bad in ["", "-", "1,00", "1 000.00", "1.5", "1.005", "abc", "--1.00", "."] {
            assert!(parse_grosze(bad).is_err(), "{bad} powinno być odrzucone");
        }
    }

    #[test]
    fn formats_back() {
        assert_eq!(format_grosze(-117_700), "-1177.00");
        assert_eq!(format_grosze(5), "0.05");
        assert_eq!(format_grosze(0), "0.00");
    }

    #[test]
    fn iban_is_normalized_and_checked() {
        assert_eq!(normalize_iban("pl61 1090 1014 0000 0712 1981 2874").unwrap(), "PL61109010140000071219812874");
        assert!(normalize_iban("za krótki").is_err());
        assert!(normalize_iban("1234567890123456789").is_err());
    }

    #[test]
    fn masks_iban() {
        assert_eq!(mask_iban("PL61109010140000071219812874"), "…2874");
        assert_eq!(mask_iban("AB"), "…");
    }
}
