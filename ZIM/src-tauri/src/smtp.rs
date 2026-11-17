/// Serwery SMTP popularnych dostawców, po hoście IMAP (te same hasła aplikacji działają w obu).
const KNOWN: [(&str, &str, u16); 9] = [
    ("imap.gmail.com", "smtp.gmail.com", 465),
    ("imap.mail.me.com", "smtp.mail.me.com", 587),
    ("imap.wp.pl", "smtp.wp.pl", 465),
    ("poczta.o2.pl", "poczta.o2.pl", 465),
    ("imap.poczta.onet.pl", "smtp.poczta.onet.pl", 465),
    ("poczta.interia.pl", "poczta.interia.pl", 465),
    ("imap.mail.yahoo.com", "smtp.mail.yahoo.com", 465),
    ("imap.fastmail.com", "smtp.fastmail.com", 465),
    ("outlook.office365.com", "smtp.office365.com", 587),
];

/// Najlepsze przypuszczenie: znany dostawca, a w innym wypadku imap.x -> smtp.x na porcie 465 (TLS).
pub fn guess_smtp(imap_host: &str) -> (String, u16) {
    let host = imap_host.trim().to_lowercase();
    if let Some((_, smtp, port)) = KNOWN.iter().find(|(imap, _, _)| *imap == host) {
        return ((*smtp).to_string(), *port);
    }
    let smtp = match host.strip_prefix("imap.") {
        Some(rest) => format!("smtp.{rest}"),
        None => host,
    };
    (smtp, 465)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_guessed() {
        assert_eq!(guess_smtp("IMAP.gmail.com"), ("smtp.gmail.com".into(), 465));
        assert_eq!(guess_smtp("imap.firma.pl"), ("smtp.firma.pl".into(), 465));
        assert_eq!(guess_smtp("mail.firma.pl"), ("mail.firma.pl".into(), 465));
    }
}
