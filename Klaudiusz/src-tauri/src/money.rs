//! Formatowanie wartości dla modelu i dla głosu (tekst po polsku).

/// Kwota w groszach na tekst: „1 234,56 zł”
pub fn pln(grosze: i64) -> String {
    let sign = if grosze < 0 { "-" } else { "" };
    let abs = grosze.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(' ');
        }
        grouped.push(ch);
    }
    format!("{sign}{grouped},{:02} zł", abs % 100)
}
