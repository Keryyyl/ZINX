#![allow(dead_code)] // używane przez synchronizację (następny krok portowania)
/// "Jan Kowalski <Jan@X.pl>" -> "jan@x.pl"; adres bez nawiasów zwraca znormalizowany.
pub fn extract_email(from: &str) -> Option<String> {
    let bracketed = from.find('<').and_then(|start| {
        let rest = &from[start + 1..];
        let end = rest.find('>')?;
        let inner = &rest[..end];
        (inner.contains('@') && !inner.chars().any(char::is_whitespace)).then(|| inner.to_string())
    });
    let plain = || {
        from.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | ',' | '"' | '\''))
            .find(|w| w.contains('@') && !w.starts_with('@') && !w.ends_with('@'))
            .map(str::to_string)
    };
    bracketed.or_else(plain).map(|a| a.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts() {
        assert_eq!(extract_email("Jan Kowalski <Jan@X.pl>").as_deref(), Some("jan@x.pl"));
        assert_eq!(extract_email("jan@x.pl").as_deref(), Some("jan@x.pl"));
        assert_eq!(extract_email("brak adresu"), None);
    }
}
