//! Prawdziwe połączenie z serwerem IMAP (async-imap przez TLS). Cienka warstwa nad interfejsem `ImapSession`:
//! logika synchronizacji jest testowana na atrapie (`mock.rs`), a ta warstwa tylko tłumaczy polecenia.
//! Obsługujemy połączenie szyfrowane od początku (zwykle port 993); STARTTLS dojdzie osobno.

use super::folders::{pack_uids, ListedMailbox};
use super::session::{BodyPart, Fetched, Flag, Flags, ImapSession, UidMap};
use crate::error::{Error, Result};
use async_imap::imap_proto::{BodyStructure, ContentEncoding, MessageSection, SectionPath};
use async_imap::types::{Fetch, Flag as ImapFlag, NameAttribute};
use async_imap::{Client, Session};
use async_trait::async_trait;
use futures::TryStreamExt;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::{client::TlsStream, TlsConnector};

type Stream = TlsStream<TcpStream>;

pub struct RealSession {
    session: Session<Stream>,
    gmail: bool,
    has_move: bool,
}

fn net<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> Error + '_ {
    move |e| Error(format!("{what}: {e}"))
}

/// Dekoduje treść części według Content-Transfer-Encoding z jej nagłówka.
pub fn decode_transfer(mime_header: &[u8], body: &[u8]) -> Vec<u8> {
    let header = String::from_utf8_lossy(mime_header).to_lowercase();
    let encoding = header
        .lines()
        .find_map(|l| l.strip_prefix("content-transfer-encoding:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    match encoding.as_str() {
        "base64" => {
            use base64::Engine;
            let compact: Vec<u8> = body.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
            base64::engine::general_purpose::STANDARD.decode(&compact).unwrap_or_else(|_| body.to_vec())
        }
        "quoted-printable" => quoted_printable::decode(body, quoted_printable::ParseMode::Robust).unwrap_or_else(|_| body.to_vec()),
        _ => body.to_vec(),
    }
}

impl RealSession {
    async fn fetch_section(&mut self, uid: u32, part: &str, mime: bool) -> Result<Vec<u8>> {
        let numbers: Vec<u32> = part.split('.').map(|n| n.parse().map_err(|_| Error::from("Nieprawidłowy numer części"))).collect::<Result<_>>()?;
        let spec = format!("BODY.PEEK[{part}{}]", if mime { ".MIME" } else { "" });
        let fetches: Vec<Fetch> = self
            .session
            .uid_fetch(uid.to_string(), format!("(UID {spec})"))
            .await
            .map_err(net("Pobieranie załącznika"))?
            .try_collect()
            .await
            .map_err(net("Pobieranie załącznika"))?;
        let f = fetches.first().ok_or_else(|| Error::from("Wiadomości nie ma już na serwerze"))?;
        let path = SectionPath::Part(numbers, mime.then_some(MessageSection::Mime));
        f.section(&path).map(<[u8]>::to_vec).ok_or_else(|| Error::from("Serwer nie zwrócił treści załącznika"))
    }

    pub async fn connect(host: &str, port: u16, user: &str, password: &str) -> Result<Self> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let name = ServerName::try_from(host.to_string()).map_err(|_| Error::from("Nieprawidłowa nazwa serwera IMAP"))?;
        let tcp = tokio::time::timeout(std::time::Duration::from_secs(20), TcpStream::connect((host, port)))
            .await
            .map_err(|_| Error::from("Serwer IMAP nie odpowiada"))?
            .map_err(net("Nie udało się połączyć z serwerem IMAP"))?;
        let tls = connector.connect(name, tcp).await.map_err(net("Nie udało się nawiązać połączenia szyfrowanego (sprawdź port, zwykle 993)"))?;

        let mut client = Client::new(tls);
        client.read_response().await.map_err(net("Serwer IMAP nie przywitał się"))?;
        let mut session = client.login(user, password).await.map_err(|(e, _)| Error(format!("Logowanie nie powiodło się: {e}")))?;
        let caps = session.capabilities().await.map_err(net("Nie udało się odczytać możliwości serwera"))?;
        Ok(RealSession { gmail: caps.has_str("X-GM-EXT-1"), has_move: caps.has_str("MOVE"), session })
    }
}

fn special_use(attrs: &[NameAttribute<'_>]) -> Option<String> {
    attrs.iter().find_map(|a| match a {
        NameAttribute::All => Some("\\All".to_string()),
        NameAttribute::Archive => Some("\\Archive".to_string()),
        NameAttribute::Junk => Some("\\Junk".to_string()),
        NameAttribute::Trash => Some("\\Trash".to_string()),
        NameAttribute::Extension(s) => Some(s.to_string()).filter(|s| s.starts_with('\\')),
        _ => None,
    })
}

fn flags_of(f: &Fetch) -> Flags {
    let mut flags = Flags::default();
    for flag in f.flags() {
        match flag {
            ImapFlag::Seen => flags.seen = true,
            ImapFlag::Flagged => flags.flagged = true,
            _ => {}
        }
    }
    flags
}

fn param(params: &Option<Vec<(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)>>, key: &str) -> Option<String> {
    let list = params.as_ref()?;
    // RFC 2231: `filename*=utf-8''Zażółć%20plik.pdf` ma pierwszeństwo przed zwykłym `filename`
    if let Some((_, v)) = list.iter().find(|(k, _)| k.eq_ignore_ascii_case(&format!("{key}*"))) {
        if let Some(decoded) = decode_rfc2231(v) {
            return Some(decoded);
        }
    }
    list.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.to_string())
}

/// `charset'język'tekst%20z%20procentami` -> tekst (UTF-8; inne kodowania zostawiamy zwykłej nazwie)
pub fn decode_rfc2231(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, '\'');
    let (charset, _lang, text) = (parts.next()?, parts.next()?, parts.next()?);
    if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("us-ascii") {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Drzewo imap-proto -> nasze `BodyPart` z numerami części jak na serwerze ("1", "1.2").
fn convert(bs: &BodyStructure<'_>, part: Option<String>) -> BodyPart {
    let build = |common: &async_imap::imap_proto::BodyContentCommon<'_>, other: Option<&async_imap::imap_proto::BodyContentSinglePart<'_>>| BodyPart {
        part: part.clone(),
        mime: format!("{}/{}", common.ty.ty, common.ty.subtype).to_lowercase(),
        children: Vec::new(),
        filename: common
            .disposition
            .as_ref()
            .and_then(|d| param(&d.params, "filename"))
            .or_else(|| param(&common.ty.params, "name")),
        disposition: common.disposition.as_ref().map(|d| d.ty.to_lowercase()),
        id: other.and_then(|o| o.id.as_ref().map(|i| i.to_string())),
        size: other.map_or(0, |o| o.octets),
        encoding: other.map(|o| match &o.transfer_encoding {
            ContentEncoding::Base64 => "base64".to_string(),
            ContentEncoding::QuotedPrintable => "quoted-printable".to_string(),
            ContentEncoding::SevenBit => "7bit".to_string(),
            ContentEncoding::EightBit => "8bit".to_string(),
            ContentEncoding::Binary => "binary".to_string(),
            ContentEncoding::Other(s) => s.to_string(),
        }),
    };
    match bs {
        BodyStructure::Basic { common, other, .. } | BodyStructure::Text { common, other, .. } | BodyStructure::Message { common, other, .. } => {
            build(common, Some(other))
        }
        BodyStructure::Multipart { common, bodies, .. } => {
            let mut node = build(common, None);
            node.children = bodies
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let number = (i + 1).to_string();
                    convert(b, Some(match &part {
                        Some(p) => format!("{p}.{number}"),
                        None => number,
                    }))
                })
                .collect();
            node
        }
    }
}

/// Wartość do polecenia SEARCH: w cudzysłowie, bez znaków sterujących.
fn quote(value: &str) -> Result<String> {
    if value.chars().any(|c| c.is_control()) {
        return Err("Nieprawidłowy identyfikator wiadomości".into());
    }
    Ok(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
}

#[async_trait]
impl ImapSession for RealSession {
    fn is_gmail(&self) -> bool {
        self.gmail
    }

    async fn list(&mut self) -> Result<Vec<ListedMailbox>> {
        let names: Vec<_> = self
            .session
            .list(Some(""), Some("*"))
            .await
            .map_err(net("Nie udało się pobrać listy folderów"))?
            .try_collect()
            .await
            .map_err(net("Nie udało się pobrać listy folderów"))?;
        Ok(names.iter().map(|n| ListedMailbox { path: n.name().to_string(), special_use: special_use(n.attributes()) }).collect())
    }

    async fn create(&mut self, path: &str) -> Result<()> {
        self.session.create(path).await.map_err(net("Nie udało się utworzyć folderu"))
    }

    async fn select(&mut self, path: &str) -> Result<u32> {
        let mailbox = self.session.select(path).await.map_err(|e| Error(format!("Nie udało się otworzyć {path}: {e}")))?;
        Ok(mailbox.uid_validity.unwrap_or(0))
    }

    async fn search_uids_from(&mut self, from: u32) -> Result<Vec<u32>> {
        let found = self.session.uid_search(format!("UID {from}:*")).await.map_err(net("Wyszukiwanie nowych wiadomości"))?;
        let mut uids: Vec<u32> = found.into_iter().collect();
        uids.sort_unstable();
        Ok(uids)
    }

    async fn search_message_id(&mut self, message_id: &str) -> Result<Option<u32>> {
        let query = format!("HEADER Message-ID {}", quote(message_id)?);
        let found = self.session.uid_search(query).await.map_err(net("Wyszukiwanie wiadomości po Message-ID"))?;
        Ok(found.into_iter().max())
    }

    async fn fetch_full(&mut self, uids: &[u32]) -> Result<Vec<Fetched>> {
        let fetches: Vec<Fetch> = self
            .session
            .uid_fetch(pack_uids(uids), "(UID FLAGS INTERNALDATE BODY.PEEK[] BODYSTRUCTURE)")
            .await
            .map_err(net("Pobieranie wiadomości"))?
            .try_collect()
            .await
            .map_err(net("Pobieranie wiadomości"))?;
        Ok(fetches
            .iter()
            .filter_map(|f| {
                Some(Fetched {
                    uid: f.uid?,
                    flags: flags_of(f),
                    internal_date: f.internal_date().map(|d| d.timestamp_millis()),
                    source: f.body()?.to_vec(),
                    structure: f.bodystructure().map(|b| convert(b, None)),
                })
            })
            .collect())
    }

    async fn fetch_flags(&mut self, uids: &[u32]) -> Result<Vec<(u32, Flags)>> {
        let fetches: Vec<Fetch> = self
            .session
            .uid_fetch(pack_uids(uids), "(UID FLAGS)")
            .await
            .map_err(net("Pobieranie flag"))?
            .try_collect()
            .await
            .map_err(net("Pobieranie flag"))?;
        Ok(fetches.iter().filter_map(|f| Some((f.uid?, flags_of(f)))).collect())
    }

    async fn fetch_structures(&mut self, uids: &[u32]) -> Result<Vec<(u32, BodyPart)>> {
        let fetches: Vec<Fetch> = self
            .session
            .uid_fetch(pack_uids(uids), "(UID BODYSTRUCTURE)")
            .await
            .map_err(net("Pobieranie struktury wiadomości"))?
            .try_collect()
            .await
            .map_err(net("Pobieranie struktury wiadomości"))?;
        Ok(fetches.iter().filter_map(|f| Some((f.uid?, convert(f.bodystructure()?, None)))).collect())
    }

    async fn set_flag(&mut self, uids: &[u32], flag: Flag, add: bool) -> Result<()> {
        let name = match flag {
            Flag::Seen => "\\Seen",
            Flag::Flagged => "\\Flagged",
        };
        let sign = if add { '+' } else { '-' };
        let _: Vec<Fetch> = self
            .session
            .uid_store(pack_uids(uids), format!("{sign}FLAGS.SILENT ({name})"))
            .await
            .map_err(net("Zmiana flagi"))?
            .try_collect()
            .await
            .map_err(net("Zmiana flagi"))?;
        Ok(())
    }

    // async-imap nie zwraca COPYUID, więc nowych UID nie znamy: odnajdzie je następna synchronizacja po Message-ID
    async fn move_to(&mut self, uids: &[u32], target: &str) -> Result<UidMap> {
        let range = pack_uids(uids);
        if self.has_move {
            self.session.uid_mv(&range, target).await.map_err(|e| Error(format!("Nie udało się przenieść wiadomości do {target}: {e}")))?;
        } else {
            self.session.uid_copy(&range, target).await.map_err(|e| Error(format!("Nie udało się skopiować wiadomości do {target}: {e}")))?;
            self.delete(uids).await?;
        }
        Ok(None)
    }

    async fn copy_to(&mut self, uids: &[u32], target: &str) -> Result<UidMap> {
        self.session
            .uid_copy(pack_uids(uids), target)
            .await
            .map_err(|e| Error(format!("Nie udało się skopiować wiadomości do {target}: {e}")))?;
        Ok(None)
    }

    async fn delete(&mut self, uids: &[u32]) -> Result<()> {
        let range = pack_uids(uids);
        let _: Vec<Fetch> = self
            .session
            .uid_store(&range, "+FLAGS.SILENT (\\Deleted)")
            .await
            .map_err(net("Usuwanie wiadomości"))?
            .try_collect()
            .await
            .map_err(net("Usuwanie wiadomości"))?;
        let _: Vec<u32> = self.session.uid_expunge(&range).await.map_err(net("Usuwanie wiadomości"))?.try_collect().await.map_err(net("Usuwanie wiadomości"))?;
        Ok(())
    }

    async fn append(&mut self, path: &str, raw: &[u8], seen: bool) -> Result<()> {
        self.session.append(path, if seen { Some("(\\Seen)") } else { None }, None, raw).await.map_err(net("Zapis w folderze Wysłane"))
    }

    async fn fetch_part(&mut self, uid: u32, part: &str) -> Result<Vec<u8>> {
        // nagłówek części mówi, jak ją zdekodować; treść pobieramy osobno, tylko tę jedną część
        let header = self.fetch_section(uid, part, true).await?;
        let body = self.fetch_section(uid, part, false).await?;
        Ok(decode_transfer(&header, &body))
    }

    async fn logout(&mut self) -> Result<()> {
        self.session.logout().await.map_err(net("Wylogowanie"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc2231_filenames() {
        assert_eq!(decode_rfc2231("utf-8''Za%C5%BC%C3%B3%C5%82%C4%87%20plik.pdf").as_deref(), Some("Zażółć plik.pdf"));
        assert_eq!(decode_rfc2231("UTF-8'pl'a.pdf").as_deref(), Some("a.pdf"));
        assert_eq!(decode_rfc2231("iso-8859-2''x"), None);
        assert_eq!(decode_rfc2231("bez-apostrofow"), None);
        assert_eq!(decode_rfc2231("utf-8''koniec%2").as_deref(), Some("koniec%2"));
    }

    #[test]
    fn transfer_decoding() {
        assert_eq!(decode_transfer(b"Content-Transfer-Encoding: base64\r\n\r\n", b"SGVs\r\nbG8="), b"Hello");
        assert_eq!(decode_transfer(b"Content-Transfer-Encoding: quoted-printable", b"za=C5=BC=\r\n=C3=B3"), "zażó".as_bytes());
        assert_eq!(decode_transfer(b"Content-Type: text/plain", b"surowe"), b"surowe");
    }
}
