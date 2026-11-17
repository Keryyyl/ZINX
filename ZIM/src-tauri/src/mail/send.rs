//! Wysyłka: składanie wiadomości RFC 822, SMTP i kopia w „Wysłanych”.

use super::session::ImapSession;
use crate::db::accounts::Account;
use crate::error::{Error, Result};
use lettre::address::Envelope;
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Limit Gmaila i większości serwerów; większe maile i tak zostałyby odrzucone
pub const MAX_ATTACHMENTS_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct MailAttachment {
    pub filename: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutgoingMail {
    pub to: Vec<String>,
    pub subject: String,
    pub text: String,
    pub attachments: Vec<MailAttachment>,
}

pub fn is_email(value: &str) -> bool {
    let bad = |c: char| c.is_whitespace() || matches!(c, '@' | '<' | '>' | ',' | ';' | '"');
    let Some((local, domain)) = value.trim().split_once('@') else { return false };
    let Some((host, tld)) = domain.rsplit_once('.') else { return false };
    !local.is_empty() && !local.contains(bad) && !host.is_empty() && !tld.is_empty() && !domain.contains(bad)
}

/// Sprawdza wiadomość przed wysłaniem/zaplanowaniem; komunikaty trafiają wprost do interfejsu.
pub fn validate_mail(mail: &OutgoingMail) -> Result<()> {
    if mail.to.is_empty() {
        return Err("Dodaj co najmniej jednego adresata".into());
    }
    let invalid: Vec<&str> = mail.to.iter().map(String::as_str).filter(|a| !is_email(a)).collect();
    if !invalid.is_empty() {
        return Err(Error(format!("Nieprawidłowy adres: {}", invalid.join(", "))));
    }
    let size: usize = mail.attachments.iter().map(|a| a.content.len()).sum();
    if size > MAX_ATTACHMENTS_BYTES {
        return Err("Załączniki mogą mieć razem najwyżej 25 MB".into());
    }
    Ok(())
}

fn parse_address(value: &str) -> Result<Address> {
    value.trim().parse::<Address>().map_err(|_| Error(format!("Nieprawidłowy adres: {value}")))
}

/// Gotowa wiadomość RFC 822: ta sama trafia przez SMTP do adresatów i przez IMAP do „Wysłanych”.
pub fn build_message(from_email: &str, mail: &OutgoingMail) -> Result<Vec<u8>> {
    let from = Mailbox::new(None, parse_address(from_email)?);
    let domain = from_email.rsplit_once('@').map_or("localhost", |(_, d)| d);
    let mut builder = Message::builder()
        .from(from)
        .subject(mail.subject.clone())
        // Message-ID z domeną nadawcy, jak w zwykłych klientach
        .message_id(Some(format!("<{}@{}>", uuid::Uuid::new_v4(), domain)));
    for to in &mail.to {
        builder = builder.to(Mailbox::new(None, parse_address(to)?));
    }
    let text = SinglePart::plain(mail.text.clone());
    let message = if mail.attachments.is_empty() {
        builder.singlepart(text)
    } else {
        let mut parts = MultiPart::mixed().singlepart(text);
        for a in &mail.attachments {
            let content_type = ContentType::parse(mime_for(&a.filename)).unwrap_or(ContentType::parse("application/octet-stream").unwrap());
            parts = parts.singlepart(Attachment::new(a.filename.clone()).body(a.content.clone(), content_type));
        }
        builder.multipart(parts)
    };
    Ok(message.map_err(|e| Error(format!("Nie udało się złożyć wiadomości: {e}")))?.formatted())
}

fn mime_for(filename: &str) -> &'static str {
    match filename.rsplit_once('.').map(|(_, e)| e.to_lowercase()).as_deref() {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("txt") => "text/plain",
        Some("zip") => "application/zip",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    /// TLS od razu (port 465)
    Wrapper,
    /// STARTTLS wymuszony, żeby hasło nie poszło otwartym tekstem (587)
    StartTls,
    /// Tylko testy na lokalnym serwerze
    #[allow(dead_code)]
    None,
}

pub fn tls_mode_for(port: u16) -> TlsMode {
    if port == 465 {
        TlsMode::Wrapper
    } else {
        TlsMode::StartTls
    }
}

pub struct SmtpTarget<'a> {
    pub host: &'a str,
    pub port: u16,
    pub tls: TlsMode,
    pub credentials: Option<(&'a str, &'a str)>,
}

/// Wysyła gotową wiadomość do adresatów. Błąd zawiera odpowiedź serwera (np. „535 Authentication failed”).
pub async fn send_raw(target: &SmtpTarget<'_>, from_email: &str, to: &[String], raw: &[u8]) -> Result<()> {
    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(target.host).port(target.port);
    let params = || TlsParameters::new(target.host.to_string()).map_err(|e| Error(format!("TLS: {e}")));
    builder = match target.tls {
        TlsMode::Wrapper => builder.tls(Tls::Wrapper(params()?)),
        TlsMode::StartTls => builder.tls(Tls::Required(params()?)),
        TlsMode::None => builder.tls(Tls::None),
    };
    if let Some((user, pass)) = target.credentials {
        builder = builder.credentials(Credentials::new(user.to_string(), pass.to_string()));
    }
    let recipients = to.iter().map(|a| parse_address(a)).collect::<Result<Vec<_>>>()?;
    let envelope = Envelope::new(Some(parse_address(from_email)?), recipients).map_err(|e| Error(format!("Koperta: {e}")))?;
    builder
        .build()
        .send_raw(&envelope, raw)
        .await
        .map(|_| ())
        .map_err(|e| Error(format!("Wysyłka nie powiodła się: {e}")))
}

pub async fn send_mail(account: &Account, password: &str, mail: &OutgoingMail) -> Result<Vec<u8>> {
    validate_mail(mail)?;
    let raw = build_message(&account.email, mail)?;
    let target = SmtpTarget {
        host: &account.smtp_host,
        port: account.smtp_port,
        tls: tls_mode_for(account.smtp_port),
        credentials: Some((&account.username, password)),
    };
    send_raw(&target, &account.email, &mail.to, &raw).await?;
    Ok(raw)
}

/// Kopia w „Wysłanych” to wygoda, a nie warunek wysyłki. Gmail sam zapisuje to, co przeszło przez jego SMTP,
/// więc dopisanie dałoby duplikat.
pub async fn save_to_sent<S: ImapSession + ?Sized>(session: &mut S, raw: &[u8]) -> Result<()> {
    if session.is_gmail() {
        return Ok(());
    }
    let sent = session.list().await?.into_iter().find(|m| m.special_use.as_deref() == Some("\\Sent"));
    if let Some(sent) = sent {
        session.append(&sent.path, raw, true).await?;
    }
    session.logout().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::MessageParser;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    fn mail(to: &[&str]) -> OutgoingMail {
        OutgoingMail { to: to.iter().map(|s| s.to_string()).collect(), subject: "Cześć żółw".into(), text: "Treść z polskimi znakami: łódź".into(), attachments: vec![] }
    }

    #[test]
    fn validates_recipients_and_size() {
        assert!(validate_mail(&mail(&[])).is_err());
        assert_eq!(validate_mail(&mail(&["a@b.pl", "zly"])).unwrap_err().0, "Nieprawidłowy adres: zly");
        assert!(validate_mail(&mail(&["a@b.pl"])).is_ok());
        let mut big = mail(&["a@b.pl"]);
        big.attachments.push(MailAttachment { filename: "x.bin".into(), content: vec![0; MAX_ATTACHMENTS_BYTES + 1] });
        assert!(validate_mail(&big).is_err());
        for ok in ["jan@firma.pl", " jan.k+tag@sub.firma.co.uk "] {
            assert!(is_email(ok), "{ok}");
        }
        for bad in ["jan", "jan@", "@x.pl", "jan@x", "a b@x.pl", "a@b@c.pl", "a@x.pl,b@x.pl"] {
            assert!(!is_email(bad), "{bad}");
        }
    }

    #[test]
    fn builds_a_parseable_message_with_attachments() {
        let mut m = mail(&["ala@x.pl", "ola@y.pl"]);
        m.attachments.push(MailAttachment { filename: "faktura żółta.pdf".into(), content: b"%PDF-1.4 test".to_vec() });
        let raw = build_message("jan@firma.pl", &m).unwrap();
        let parsed = MessageParser::default().parse(&raw).unwrap();
        assert_eq!(parsed.subject(), Some("Cześć żółw"));
        assert_eq!(parsed.body_text(0).unwrap().trim(), "Treść z polskimi znakami: łódź");
        assert!(parsed.message_id().unwrap().ends_with("@firma.pl"));
        assert_eq!(parsed.to().unwrap().mailboxes().count(), 2);
        let att: Vec<_> = parsed.attachments().collect();
        assert_eq!(att.len(), 1);
        assert_eq!(att[0].attachment_name(), Some("faktura żółta.pdf"));
        assert_eq!(att[0].decoded().as_ref(), b"%PDF-1.4 test");
    }

    #[test]
    fn message_without_attachments_is_plain_text() {
        let raw = String::from_utf8(build_message("jan@firma.pl", &mail(&["a@b.pl"])).unwrap()).unwrap();
        assert!(raw.contains("Content-Type: text/plain"));
        assert!(!raw.contains("multipart"));
    }

    #[test]
    fn tls_mode_follows_port() {
        assert_eq!(tls_mode_for(465), TlsMode::Wrapper);
        assert_eq!(tls_mode_for(587), TlsMode::StartTls);
    }

    /// Minimalny serwer SMTP: zapisuje kopertę i treść, odpowiada kodami z RFC 5321.
    async fn fake_smtp(reject_data: bool) -> (u16, tokio::task::JoinHandle<(Vec<String>, String)>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (read, mut write) = socket.into_split();
            let mut lines = BufReader::new(read);
            write.write_all(b"220 test ESMTP\r\n").await.unwrap();
            let (mut commands, mut data) = (Vec::new(), String::new());
            let mut in_data = false;
            loop {
                let mut line = String::new();
                if lines.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                if in_data {
                    if line == ".\r\n" {
                        in_data = false;
                        let reply: &[u8] = if reject_data { b"554 5.7.1 Odrzucone\r\n" } else { b"250 OK\r\n" };
                        write.write_all(reply).await.unwrap();
                    } else {
                        data.push_str(&line);
                    }
                    continue;
                }
                let cmd = line.trim_end().to_string();
                commands.push(cmd.clone());
                let upper = cmd.to_uppercase();
                let reply: &[u8] = if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                    b"250 test\r\n"
                } else if upper == "DATA" {
                    in_data = true;
                    b"354 go\r\n"
                } else if upper == "QUIT" {
                    write.write_all(b"221 bye\r\n").await.unwrap();
                    break;
                } else {
                    b"250 OK\r\n"
                };
                write.write_all(reply).await.unwrap();
            }
            (commands, data)
        });
        (port, handle)
    }

    #[tokio::test]
    async fn delivers_over_smtp_with_envelope_and_body() {
        let (port, server) = fake_smtp(false).await;
        let m = mail(&["ala@x.pl"]);
        let raw = build_message("jan@firma.pl", &m).unwrap();
        let target = SmtpTarget { host: "127.0.0.1", port, tls: TlsMode::None, credentials: None };
        send_raw(&target, "jan@firma.pl", &m.to, &raw).await.unwrap();
        let (commands, data) = server.await.unwrap();
        assert!(commands.iter().any(|c| c.to_uppercase().starts_with("MAIL FROM:<JAN@FIRMA.PL>")), "{commands:?}");
        assert!(commands.iter().any(|c| c.to_uppercase().starts_with("RCPT TO:<ALA@X.PL>")), "{commands:?}");
        assert!(data.contains("Subject: ") && data.contains("From: jan@firma.pl"), "{data}");
    }

    #[tokio::test]
    async fn smtp_rejection_surfaces_the_server_reply() {
        let (port, _server) = fake_smtp(true).await;
        let m = mail(&["ala@x.pl"]);
        let raw = build_message("jan@firma.pl", &m).unwrap();
        let target = SmtpTarget { host: "127.0.0.1", port, tls: TlsMode::None, credentials: None };
        let err = send_raw(&target, "jan@firma.pl", &m.to, &raw).await.unwrap_err();
        assert!(err.0.contains("Odrzucone") || err.0.contains("554"), "{}", err.0);
    }

    #[tokio::test]
    async fn sent_copy_is_appended_except_on_gmail() {
        use crate::mail::mock::MockServer;
        let server = MockServer::new();
        server.0.lock().unwrap().mailboxes.insert(
            "Sent".into(),
            crate::mail::mock::MockMailbox { special_use: Some("\\Sent".into()), uid_validity: 1, next_uid: 1, ..Default::default() },
        );
        save_to_sent(&mut server.session(), "From: a@x.pl\r\n\r\ntreść".as_bytes()).await.unwrap();
        assert_eq!(server.uids("Sent").len(), 1);
        assert!(server.flags("Sent", 1).seen);
        server.0.lock().unwrap().gmail = true;
        save_to_sent(&mut server.session(), "From: a@x.pl\r\n\r\ninna".as_bytes()).await.unwrap();
        assert_eq!(server.uids("Sent").len(), 1, "Gmail sam zapisuje w Wysłanych");
    }
}
