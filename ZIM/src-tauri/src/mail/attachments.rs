use super::session::BodyPart;

/// Załącznik opisany w BODYSTRUCTURE: treść pobieramy dopiero na żądanie.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentInfo {
    /// Numer części MIME na serwerze, np. "2" albo "1.2"
    pub part: String,
    pub filename: String,
    pub mime: String,
    /// Rozmiar po zdekodowaniu (przybliżony)
    pub size: u32,
    pub content_id: Option<String>,
    /// Obrazek osadzony w treści (cid:), np. logo w stopce
    pub inline: bool,
}

/// Pliki, które system może uruchomić zamiast otworzyć; przed otwarciem pytamy o zgodę.
const RISKY_EXTENSIONS: [&str; 28] = [
    "exe", "bat", "cmd", "com", "scr", "msi", "app", "command", "sh", "scpt", "jar", "js", "jse", "vbs", "vbe", "ps1", "dmg", "pkg",
    "workflow", "terminal", "hta", "lnk", "pif", "cpl", "reg", "wsf", "action", "apk",
];

fn default_name(mime: &str) -> &'static str {
    match mime {
        "message/rfc822" => "wiadomość.eml",
        "text/calendar" => "zaproszenie.ics",
        _ => "załącznik",
    }
}

/// Nazwy plików w nagłówkach bywają zakodowane (RFC 2047: `=?UTF-8?Q?...?=`); zwraca zwykły tekst.
/// Tekst bez takiego kodowania zostaje bez zmian.
pub fn decode_filename(raw: &str) -> String {
    if !raw.contains("=?") || raw.contains(['\r', '\n']) {
        return raw.to_string();
    }
    let header = format!("Subject: {raw}\r\n\r\n");
    mail_parser::MessageParser::default()
        .parse(header.as_bytes())
        .and_then(|m| m.subject().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| raw.to_string())
}

/// Zbiera załączniki z drzewa BODYSTRUCTURE (części tekstowe z treścią maila pomijamy).
pub fn collect_attachments(root: &BodyPart) -> Vec<AttachmentInfo> {
    let mut found = Vec::new();
    walk(root, &mut found);
    found
}

fn walk(node: &BodyPart, found: &mut Vec<AttachmentInfo>) {
    let mime = node.mime.to_lowercase();
    // załączona wiadomość (.eml) to jedna całość: nie wchodzimy do jej części
    if !node.children.is_empty() && mime != "message/rfc822" {
        node.children.iter().for_each(|c| walk(c, found));
        return;
    }
    if mime.starts_with("multipart/") {
        return;
    }
    let filename = node.filename.as_deref().map(decode_filename).unwrap_or_default();
    let disposition = node.disposition.as_deref().map(str::to_lowercase);
    if (mime == "text/plain" || mime == "text/html") && disposition.as_deref() != Some("attachment") && filename.is_empty() {
        return;
    }
    let is_base64 = node.encoding.as_deref().is_some_and(|e| e.eq_ignore_ascii_case("base64"));
    found.push(AttachmentInfo {
        // wiadomość bez multipart ma jedną część, którą serwer nazywa "1"
        part: node.part.clone().unwrap_or_else(|| "1".into()),
        filename: if filename.is_empty() { default_name(&mime).into() } else { filename },
        size: if is_base64 { (u64::from(node.size) * 3 / 4) as u32 } else { node.size },
        content_id: node.id.as_ref().map(|id| id.trim_start_matches('<').trim_end_matches('>').to_string()),
        inline: disposition.as_deref() == Some("inline") && node.id.is_some(),
        mime,
    });
}

/// Nazwa pliku z maila to dane od obcej osoby: bez ścieżek, znaków sterujących i ukrytych plików.
#[allow(dead_code)] // używane przez pobieranie załączników (następny krok)
pub fn safe_filename(name: &str) -> String {
    let replaced: String = name
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let cleaned = replaced.trim_start_matches(|c: char| c == '.' || c.is_whitespace()).trim().to_string();
    if cleaned.is_empty() {
        return "załącznik".into();
    }
    let chars: Vec<char> = cleaned.chars().collect();
    if chars.len() <= 150 {
        return cleaned;
    }
    let dot = chars.iter().rposition(|&c| c == '.');
    let ext: String = match dot {
        Some(d) if d > 0 && chars.len() - d <= 10 => chars[d..].iter().collect(),
        _ => String::new(),
    };
    let keep = 150 - ext.chars().count();
    format!("{}{}", chars[..keep].iter().collect::<String>(), ext)
}

#[allow(dead_code)] // używane przy otwieraniu załączników (następny krok)
pub fn is_risky(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    match trimmed.rsplit_once('.') {
        Some((_, ext)) if !ext.contains(['\\', '/']) && !ext.contains(char::is_whitespace) => RISKY_EXTENSIONS.contains(&ext),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(part: Option<&str>, mime: &str) -> BodyPart {
        BodyPart { part: part.map(str::to_string), mime: mime.into(), ..Default::default() }
    }

    #[test]
    fn skips_body_and_collects_nested_files() {
        let tree = BodyPart {
            mime: "multipart/mixed".into(),
            children: vec![
                BodyPart {
                    mime: "multipart/alternative".into(),
                    part: Some("1".into()),
                    children: vec![
                        BodyPart { size: 10, ..leaf(Some("1.1"), "text/plain") },
                        BodyPart { size: 40, ..leaf(Some("1.2"), "text/html") },
                    ],
                    ..Default::default()
                },
                BodyPart {
                    encoding: Some("base64".into()),
                    size: 4000,
                    disposition: Some("attachment".into()),
                    filename: Some("CV.pdf".into()),
                    ..leaf(Some("2"), "application/pdf")
                },
                BodyPart {
                    id: Some("<logo@firma>".into()),
                    disposition: Some("inline".into()),
                    filename: Some("logo.png".into()),
                    size: 100,
                    ..leaf(Some("3"), "image/png")
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            collect_attachments(&tree),
            [
                AttachmentInfo { part: "2".into(), filename: "CV.pdf".into(), mime: "application/pdf".into(), size: 3000, content_id: None, inline: false },
                AttachmentInfo { part: "3".into(), filename: "logo.png".into(), mime: "image/png".into(), size: 100, content_id: Some("logo@firma".into()), inline: true },
            ]
        );
    }

    #[test]
    fn plain_text_has_no_attachments_and_single_part_is_part_one() {
        assert!(collect_attachments(&BodyPart { size: 5, ..leaf(None, "text/plain") }).is_empty());
        let a = collect_attachments(&BodyPart { size: 8, disposition: Some("attachment".into()), ..leaf(None, "application/pdf") });
        assert_eq!((a[0].part.as_str(), a[0].filename.as_str()), ("1", "załącznik"));
    }

    #[test]
    fn attached_message_is_one_eml_file() {
        let tree = BodyPart {
            mime: "multipart/mixed".into(),
            children: vec![
                leaf(Some("1"), "text/plain"),
                BodyPart { size: 900, children: vec![leaf(Some("2.1"), "text/plain")], ..leaf(Some("2"), "message/rfc822") },
            ],
            ..Default::default()
        };
        let found = collect_attachments(&tree);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].part.as_str(), found[0].filename.as_str()), ("2", "wiadomość.eml"));
    }

    #[test]
    fn decodes_encoded_word_filenames() {
        let encoded = "=?UTF-8?Q?CV_Jan_Kowalski_(PL).pdf?=";
        assert_eq!(decode_filename(encoded), "CV Jan Kowalski (PL).pdf");
        assert_eq!(decode_filename("=?UTF-8?B?emHFvMOzxYLEhy50eHQ=?="), "zażółć.txt");
        // kilka słów obok siebie to jedna nazwa
        assert_eq!(decode_filename("=?UTF-8?Q?raport_?= =?UTF-8?Q?2026.xlsx?="), "raport 2026.xlsx");
        assert_eq!(decode_filename("zwykła nazwa.pdf"), "zwykła nazwa.pdf");
        assert_eq!(decode_filename("a=?b.pdf"), "a=?b.pdf");
        let decoded = safe_filename(&decode_filename(encoded));
        assert!(decoded.ends_with(".pdf") && !is_risky(&decoded), "{decoded}");
    }

    #[test]
    fn collect_decodes_names_from_the_structure() {
        let node = BodyPart {
            filename: Some("=?UTF-8?Q?Umowa_=C5=BC=C3=B3=C5=82ta.pdf?=".into()),
            disposition: Some("attachment".into()),
            ..leaf(Some("2"), "application/pdf")
        };
        assert_eq!(collect_attachments(&node)[0].filename, "Umowa żółta.pdf");
    }

    #[test]
    fn safe_filename_strips_paths_and_controls() {
        assert_eq!(safe_filename("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_filename(".bashrc"), "bashrc");
        assert_eq!(safe_filename("a\u{0}b.txt"), "ab.txt");
        assert_eq!(safe_filename("   "), "załącznik");
        let long = safe_filename(&format!("{}.pdf", "a".repeat(300)));
        assert_eq!(long.chars().count(), 150);
        assert!(long.ends_with(".pdf"));
    }

    #[test]
    fn risky_extensions() {
        assert!(is_risky("faktura.pdf.exe"));
        assert!(is_risky("Setup.DMG"));
        assert!(is_risky("faktura.exe."));
        assert!(!is_risky("CV.pdf"));
        assert!(!is_risky("Makefile"));
    }
}
