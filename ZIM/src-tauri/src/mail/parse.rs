//! Surowa wiadomość (RFC 822) -> pola zapisywane w bazie.

use crate::address::extract_email;
use mail_parser::{AddressList, MessageParser, PartKind};

const SNIPPET_WORDS: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMail {
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub from: String,
    pub from_email: Option<String>,
    pub to: String,
    pub subject: String,
    /// unix ms
    pub date: i64,
    pub snippet: String,
    pub body_text: String,
    pub body_html: Option<String>,
    /// Nagłówek List-Unsubscribe: newsletter albo powiadomienie masowe
    pub bulk: bool,
}

/// Pierwsze słowa treści, bez cytatów z poprzednich wiadomości.
pub fn make_snippet(text: &str, words: usize) -> String {
    let kept: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with('>')).collect();
    let all: Vec<&str> = kept.iter().flat_map(|l| l.split_whitespace()).collect();
    let head = all.iter().take(words).copied().collect::<Vec<_>>().join(" ");
    if all.len() > words {
        format!("{head}…")
    } else {
        head
    }
}

fn format_addresses(address: Option<AddressList>) -> String {
    let Some(address) = address else { return String::new() };
    address
        .mailboxes()
        .map(|a| {
            let addr = a.address().unwrap_or("");
            match a.name().filter(|n| !n.is_empty()) {
                Some(name) => format!("{name} <{addr}>"),
                None => addr.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn bracketed(id: &str) -> String {
    format!("<{}>", id.trim_start_matches('<').trim_end_matches('>'))
}

/// `fallback_date`: data odbioru z serwera, gdy wiadomość nie ma poprawnego nagłówka Date (unix ms).
pub fn parse_message(source: &[u8], fallback_date: Option<i64>) -> Option<ParsedMail> {
    let msg = MessageParser::default().parse(source)?;
    let body_text = msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default();
    let message_id = msg.message_id().map(bracketed);
    let first_reference = msg.references().and_then(|list| list.first()).map(str::to_string);
    let from = format_addresses(msg.from());
    let date = msg
        .date()
        .map(|d| d.to_timestamp() * 1000)
        .or(fallback_date)
        .unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64));
    Some(ParsedMail {
        thread_id: first_reference.map(|r| bracketed(&r)).or_else(|| message_id.clone()),
        message_id,
        from_email: extract_email(&from),
        from,
        to: format_addresses(msg.to()),
        subject: msg.subject().unwrap_or("").to_string(),
        date,
        snippet: make_snippet(&body_text, SNIPPET_WORDS),
        // body_html zamienia też zwykły tekst na HTML; zapisujemy tylko prawdziwą część text/html
        body_html: if msg.html_body().any(|p| matches!(p.kind(), PartKind::Html)) {
            msg.body_html(0).map(|h| h.into_owned()).filter(|h| !h.is_empty())
        } else {
            None
        },
        body_text,
        bulk: msg.headers().contains("List-Unsubscribe"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_truncates_and_skips_quotes() {
        assert_eq!(make_snippet("Cześć,\n\n  jak   się masz? Dawno nie gadaliśmy.", 4), "Cześć, jak się masz?…");
        assert_eq!(make_snippet("Jasne, pasuje.\n> W dniu wtorek Jan napisał:\n> spotkanie?", 30), "Jasne, pasuje.");
        assert_eq!(make_snippet("", 30), "");
    }

    const RAW: &str = "From: Jan Kowalski <Jan@Firma.pl>\r\nTo: me@x.pl, \"Ala\" <ala@x.pl>\r\nSubject: Faktura 12/2026\r\n\
Message-ID: <abc123@firma.pl>\r\nReferences: <root@firma.pl> <prev@firma.pl>\r\nDate: Tue, 3 Nov 2026 10:00:00 +0000\r\n\
List-Unsubscribe: <mailto:u@firma.pl>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nWitaj,\r\nw załączeniu faktura.\r\n";

    #[test]
    fn parses_headers_and_body() {
        let m = parse_message(RAW.as_bytes(), None).unwrap();
        assert_eq!(m.from, "Jan Kowalski <Jan@Firma.pl>");
        assert_eq!(m.from_email.as_deref(), Some("jan@firma.pl"));
        assert_eq!(m.to, "me@x.pl, Ala <ala@x.pl>");
        assert_eq!(m.subject, "Faktura 12/2026");
        assert_eq!(m.message_id.as_deref(), Some("<abc123@firma.pl>"));
        assert_eq!(m.thread_id.as_deref(), Some("<root@firma.pl>"));
        assert_eq!(m.date, 1_793_700_000_000);
        assert_eq!(m.snippet, "Witaj, w załączeniu faktura.");
        assert!(m.bulk);
        assert!(m.body_html.is_none());
    }

    #[test]
    fn falls_back_to_server_date_and_message_id_thread() {
        let raw = "From: a@x.pl\r\nSubject: Bez daty\r\nMessage-ID: <m1@x.pl>\r\n\r\ntreść";
        let m = parse_message(raw.as_bytes(), Some(777)).unwrap();
        assert_eq!((m.date, m.thread_id.as_deref(), m.bulk), (777, Some("<m1@x.pl>"), false));
    }
}
