//! Daty w formacie RRRR-MM-DD (dzień) i RRRR-MM (miesiąc) oraz powtarzalność tak jak w CTHX
//! (src/shared/recurrence.ts). Wszystkie daty przychodzą z frontendu, bo tam znamy strefę czasową.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub fn valid_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let nums: Option<Vec<u32>> = parts.iter().map(|p| p.parse().ok()).collect();
    matches!(nums, Some(n) if (1..=12).contains(&n[1]) && (1..=31).contains(&n[2]))
}

pub fn valid_month(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 7 && b[4] == b'-' && b[..4].iter().all(u8::is_ascii_digit) && b[5..].iter().all(u8::is_ascii_digit) && (1..=12).contains(&text[5..].parse::<u32>().unwrap_or(0))
}

/// GG:MM, godzina 00–23, minuty 00–59
pub fn valid_time(text: &str) -> bool {
    let Some((h, m)) = text.split_once(':') else { return false };
    if h.len() != 2 || m.len() != 2 {
        return false;
    }
    matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60)
}

/// Minuty od północy dla GG:MM
pub fn minutes(text: &str) -> u32 {
    let (h, m) = text.split_once(':').unwrap_or(("0", "0"));
    h.parse::<u32>().unwrap_or(0) * 60 + m.parse::<u32>().unwrap_or(0)
}

/// Liczba dni od 1970-01-01 w kalendarzu gregoriańskim (bez stref czasowych)
pub fn day_number(key: &str) -> i64 {
    let y: i64 = key[..4].parse().unwrap_or(1970);
    let m: i64 = key[5..7].parse().unwrap_or(1);
    let d: i64 = key[8..10].parse().unwrap_or(1);
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Czy wpis o dacie startu `start` i powtarzalności `recurrence` wypada w dniu `day` (bez sprawdzania wykonania).
/// Odpowiednik `occursOn` z CTHX.
pub fn occurs_on(start: &str, recurrence: &str, day: &str) -> bool {
    if day < start {
        return false;
    }
    match recurrence {
        "none" => day == start,
        "daily" => true,
        "weekly" => (day_number(day) - day_number(start)).rem_euclid(7) == 0,
        "monthly" => day[8..] == start[8..],
        "yearly" => day[5..] == start[5..],
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_numbers_are_consecutive() {
        assert_eq!(day_number("1970-01-01"), 0);
        assert_eq!(day_number("1970-01-02") - day_number("1970-01-01"), 1);
        assert_eq!(day_number("2024-03-01") - day_number("2024-02-28"), 2); // rok przestępny
        assert_eq!(day_number("2026-11-08") - day_number("2026-11-01"), 7);
    }

    #[test]
    fn one_off_runs_only_on_its_day() {
        assert!(occurs_on("2026-11-08", "none", "2026-11-08"));
        assert!(!occurs_on("2026-11-08", "none", "2026-11-09"));
        assert!(!occurs_on("2026-11-08", "none", "2026-11-07"));
    }

    #[test]
    fn repeating_items_respect_start_date() {
        assert!(!occurs_on("2026-11-08", "daily", "2026-11-07"));
        assert!(occurs_on("2026-11-08", "daily", "2026-12-25"));
        assert!(occurs_on("2026-11-02", "weekly", "2026-11-09"));
        assert!(!occurs_on("2026-11-02", "weekly", "2026-11-10"));
        assert!(occurs_on("2026-01-15", "monthly", "2026-11-15"));
        assert!(!occurs_on("2026-01-15", "monthly", "2026-11-16"));
        assert!(occurs_on("1990-11-08", "yearly", "2026-11-08"));
        assert!(!occurs_on("1990-11-08", "yearly", "2026-11-09"));
    }

    #[test]
    fn validates_dates_and_times() {
        assert!(valid_date("2026-11-08"));
        assert!(!valid_date("2026-13-08"));
        assert!(!valid_date("2026-1-08"));
        assert!(valid_time("09:05"));
        assert!(!valid_time("24:00"));
        assert!(!valid_time("9:05"));
    }
}
