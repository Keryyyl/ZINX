//! Daty lokalne (RRRR-MM-DD) i powtarzalność wpisów CTHX (odpowiednik `occursOn` z CTHX).

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};

pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn tomorrow() -> String {
    (Local::now().date_naive() + Duration::days(1)).format("%Y-%m-%d").to_string()
}

pub fn current_month() -> String {
    Local::now().format("%Y-%m").to_string()
}

/// Minuty od północy dla GG:MM
pub fn minutes(text: &str) -> u32 {
    let (h, m) = text.split_once(':').unwrap_or(("0", "0"));
    h.parse::<u32>().unwrap_or(0) * 60 + m.parse::<u32>().unwrap_or(0)
}

/// Liczba dni kalendarzowych (do porównań i dni tygodnia)
fn day_number(key: &str) -> i64 {
    NaiveDate::parse_from_str(key, "%Y-%m-%d").map(|d| d.num_days_from_ce() as i64).unwrap_or(0)
}

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

/// Dzień z przesunięciem o `days` względem dzisiaj (np. jutro)
pub fn day_offset(days: i64) -> String {
    let d = Local::now().date_naive() + Duration::days(days);
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeating_items_respect_start_date() {
        assert!(!occurs_on("2026-11-08", "daily", "2026-11-07"));
        assert!(occurs_on("2026-11-08", "daily", "2026-12-25"));
        assert!(occurs_on("2026-11-02", "weekly", "2026-11-09"));
        assert!(!occurs_on("2026-11-02", "weekly", "2026-11-10"));
        assert!(occurs_on("2026-01-15", "monthly", "2026-11-15"));
        assert!(occurs_on("1990-11-08", "yearly", "2026-11-08"));
        assert!(!occurs_on("2026-11-08", "none", "2026-11-09"));
    }

    #[test]
    fn relative_day_words_become_dates() {
        assert_eq!(normalize_day("jutro").unwrap(), day_offset(1));
        assert_eq!(normalize_day("tomorrow").unwrap(), day_offset(1));
        assert_eq!(normalize_day(" Dziś ").unwrap(), today());
        assert_eq!(normalize_day("2026-11-10").unwrap(), "2026-11-10");
        assert!(normalize_day("pojutrzu").is_none());
        assert!(normalize_day("2026-13-40").is_none());
    }
}


pub fn valid_date(text: &str) -> bool {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

/// Zamienia słowa względne (jutro, dziś, wczoraj…) na RRRR-MM-DD. Model czasem poda słowo zamiast daty.
/// Zwraca `None`, gdy tekst nie jest ani datą, ani znanym słowem.
pub fn normalize_day(text: &str) -> Option<String> {
    let t = text.trim().to_lowercase();
    match t.as_str() {
        "dziś" | "dzis" | "dzisiaj" | "today" => Some(today()),
        "jutro" | "tomorrow" => Some(day_offset(1)),
        "pojutrze" => Some(day_offset(2)),
        "wczoraj" | "yesterday" => Some(day_offset(-1)),
        _ if valid_date(&t) => Some(t),
        _ => None,
    }
}

/// GG:MM, godzina 00–23, minuty 00–59
pub fn valid_time(text: &str) -> bool {
    let Some((h, m)) = text.split_once(':') else { return false };
    if h.len() != 2 || m.len() != 2 {
        return false;
    }
    matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60)
}

pub fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

/// Czas lokalny „RRRR-MM-DD” + „GG:MM” jako ms od epoki
pub fn local_ms(day: &str, hm: &str) -> Result<i64, String> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| "Nieprawidłowa data".to_string())?;
    let time = chrono::NaiveTime::parse_from_str(hm, "%H:%M").map_err(|_| "Nieprawidłowa godzina".to_string())?;
    Local
        .from_local_datetime(&date.and_time(time))
        .single()
        .map(|t| t.timestamp_millis())
        .ok_or_else(|| "Ta godzina nie istnieje lokalnie (zmiana czasu)".to_string())
}

/// „RRRR-MM-DD GG:MM” w czasie lokalnym
pub fn fmt_local(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "nieznana".into())
}

/// Data po polsku do mówienia: „RRRR-MM-DD” → „10 listopada” (z rokiem, gdy to inny rok niż bieżący)
pub fn pl_date(day: &str) -> String {
    const MONTHS: [&str; 12] = ["stycznia", "lutego", "marca", "kwietnia", "maja", "czerwca", "lipca", "sierpnia", "września", "października", "listopada", "grudnia"];
    let Ok(d) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else { return day.to_string() };
    let base = format!("{} {}", d.day(), MONTHS[d.month0() as usize]);
    if d.year() == Local::now().year() { base } else { format!("{base} {}", d.year()) }
}
