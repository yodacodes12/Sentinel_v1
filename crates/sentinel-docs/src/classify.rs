//! Document type and file format identification. Metadata only: no
//! schematic parsing, OCR or boardview processing (Phase 7).
//!
//! A filename or `Content-Type` is a hint, never proof: the format is
//! recorded as established by content only when the bytes were checked.

use crate::model::{DocumentType, FileFormat, FormatBasis};

/// Boardview formats recognisable by extension. Their contents are not read.
pub const BOARDVIEW_EXTENSIONS: &[&str] = &[
    "brd", "bdv", "bv", "bvr", "fz", "cad", "asc", "tvw", "gr", "cst", "f2b", "bvr3",
];

const DOC_EXTENSIONS: &[&str] = &["pdf", "zip"];

/// Words that identify a document type, strongest first. Matched on whole
/// lower-case words of the title, filename and description.
const TYPE_WORDS: &[(DocumentType, &[&str])] = &[
    (
        DocumentType::Boardview,
        &["boardview", "board view", "brd file"],
    ),
    (
        DocumentType::Schematic,
        &[
            "schematic",
            "schematics",
            "circuit diagram",
            "block diagram",
        ],
    ),
    (
        DocumentType::ServiceManual,
        &[
            "service manual",
            "owner's manual",
            "owners manual",
            "hardware maintenance manual",
            "maintenance manual",
            "repair manual",
            "service guide",
            "disassembly",
        ],
    ),
    (
        DocumentType::FirmwareDocumentation,
        &["bios", "uefi", "firmware", "release notes"],
    ),
    (
        DocumentType::SpecificationSheet,
        &[
            "specifications",
            "specification",
            "spec sheet",
            "datasheet",
            "psref",
            "setup and specifications",
            "quickspecs",
        ],
    ),
    (
        DocumentType::HardwareManual,
        &[
            "hardware manual",
            "technical reference",
            "technical guide",
            "user manual",
            "user guide",
            "reference guide",
        ],
    ),
];

/// Lower-case extension of a filename or URL path, without query string.
pub fn extension(name: &str) -> Option<String> {
    let base = name.split(['?', '#']).next()?.rsplit('/').next()?;
    let (_, ext) = base.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    (!ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .then_some(ext)
}

pub fn doc_type(text: &str, ext: Option<&str>) -> DocumentType {
    if ext.is_some_and(|e| BOARDVIEW_EXTENSIONS.contains(&e)) {
        return DocumentType::Boardview;
    }
    let lower = format!(" {} ", text.to_lowercase().replace(['_', '-', '.'], " "));
    for (kind, words) in TYPE_WORDS {
        if words
            .iter()
            .any(|w| lower.contains(&format!(" {w} ")) || lower.contains(&format!(" {w}s ")))
        {
            return *kind;
        }
    }
    DocumentType::Unknown
}

/// Format from hints only (no content). Never `Content`.
pub fn format_from_hints(ext: Option<&str>, mime: Option<&str>) -> (FileFormat, FormatBasis) {
    match ext {
        Some("pdf") => return (FileFormat::Pdf, FormatBasis::Extension),
        Some("zip") => return (FileFormat::Zip, FormatBasis::Extension),
        Some(e) if BOARDVIEW_EXTENSIONS.contains(&e) => {
            return (FileFormat::Boardview, FormatBasis::Extension);
        }
        Some("htm" | "html" | "php" | "aspx") => {
            return (FileFormat::WebPage, FormatBasis::Extension);
        }
        _ => {}
    }
    let mime = mime.map(|m| {
        m.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    });
    match mime.as_deref() {
        Some("application/pdf") => (FileFormat::Pdf, FormatBasis::ContentType),
        Some("application/zip" | "application/x-zip-compressed") => {
            (FileFormat::Zip, FormatBasis::ContentType)
        }
        Some("text/html") => (FileFormat::WebPage, FormatBasis::ContentType),
        _ => (FileFormat::Unknown, FormatBasis::Unknown),
    }
}

/// What the first bytes of a file are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniffed {
    Pdf,
    Zip,
    /// A login, error or paywall page served in place of a document.
    Html,
    /// Executable or script content: never kept.
    Executable,
    Empty,
    /// Binary data with no recognised signature.
    Other,
}

fn looks_like_markup(text: &[u8]) -> bool {
    let lower: Vec<u8> = text.iter().take(64).map(u8::to_ascii_lowercase).collect();
    lower.starts_with(b"<!doctype")
        || lower.starts_with(b"<html")
        || lower.starts_with(b"<?xml")
        || lower.starts_with(b"<head")
}

pub fn sniff(head: &[u8]) -> Sniffed {
    let trimmed = {
        let start = head
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(head.len());
        &head[start..]
    };
    if head.is_empty() {
        Sniffed::Empty
    } else if head.starts_with(b"%PDF-") {
        Sniffed::Pdf
    } else if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        Sniffed::Zip
    } else if head.starts_with(b"\x7fELF")
        || head.starts_with(b"MZ")
        || head.starts_with(b"#!")
        || head.starts_with(b"\xca\xfe\xba\xbe")
        || head.starts_with(b"\xcf\xfa\xed\xfe")
    {
        Sniffed::Executable
    } else if looks_like_markup(trimmed) {
        Sniffed::Html
    } else {
        Sniffed::Other
    }
}

/// Decide a downloaded file's format from its bytes. A boardview is only
/// accepted when the extension says boardview and the bytes are not one of
/// the formats above.
pub fn format_from_content(head: &[u8], ext: Option<&str>) -> Result<FileFormat, String> {
    match sniff(head) {
        Sniffed::Pdf => Ok(FileFormat::Pdf),
        Sniffed::Zip => Ok(FileFormat::Zip),
        Sniffed::Html => Err(
            "the server returned a web page, not a document (often a login, error or paywall page)"
                .into(),
        ),
        Sniffed::Executable => Err(
            "the content is an executable or script; Sentinel never keeps executable files".into(),
        ),
        Sniffed::Empty => Err("the file is empty".into()),
        Sniffed::Other if ext.is_some_and(|e| BOARDVIEW_EXTENSIONS.contains(&e)) => {
            Ok(FileFormat::Boardview)
        }
        Sniffed::Other => Err("the content is not a recognised document format".into()),
    }
}

/// Is a file extension one Sentinel would keep at all?
pub fn acceptable_extension(ext: &str) -> bool {
    DOC_EXTENSIONS.contains(&ext) || BOARDVIEW_EXTENSIONS.contains(&ext)
}

/// A remote filename made safe to display: last path component only,
/// control and path characters removed, bounded. Never used as a path.
pub fn sanitise_filename(raw: &str) -> Option<String> {
    let decoded = url::form_urlencoded::parse(format!("x={raw}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    let last = decoded.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c))
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    if cleaned.is_empty() || cleaned == ".." {
        return None;
    }
    Some(cleaned.chars().take(120).collect())
}

/// Title text from a PDF's document information dictionary, if it is in
/// plain literal form within the scanned bytes. Bounded and best-effort;
/// streams are never decompressed.
pub fn pdf_title(bytes: &[u8]) -> Option<String> {
    let at = bytes
        .windows(7)
        .position(|w| w == b"/Title(" || w == b"/Title ")?;
    let rest = &bytes[at + 6..];
    let open = rest.iter().position(|&b| b == b'(')?;
    if open > 2 {
        return None;
    }
    let mut out = Vec::new();
    let mut depth = 0;
    let mut escaped = false;
    for &b in rest[open + 1..].iter().take(300) {
        if escaped {
            out.push(b);
            escaped = false;
            continue;
        }
        match b {
            b'\\' => escaped = true,
            b'(' => {
                depth += 1;
                out.push(b)
            }
            b')' if depth == 0 => break,
            b')' => {
                depth -= 1;
                out.push(b)
            }
            _ => out.push(b),
        }
    }
    // UTF-16 titles start with a byte-order mark; only plain text is used.
    if out.starts_with(&[0xfe, 0xff]) {
        return None;
    }
    let text: String = String::from_utf8_lossy(&out)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

// ---------------------------------------------------------------- archives

/// File types an archive may contain and still be kept (never extracted).
pub const ARCHIVE_ENTRY_EXTENSIONS: &[&str] = &[
    "pdf", "txt", "md", "csv", "xml", "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "zip",
];

/// One entry of a ZIP central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub uncompressed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipReport {
    pub entries: Vec<ZipEntry>,
    /// Entries that could escape an extraction directory or run code.
    pub unsafe_entries: Vec<String>,
    pub total_uncompressed: u64,
}

/// Read a ZIP file's central directory (names and sizes only). Sentinel
/// never extracts archives; this is how it decides whether one is kept and
/// what to tell the user about it.
pub fn inspect_zip(bytes: &[u8]) -> Result<ZipReport, String> {
    let le16 = |b: &[u8], i: usize| -> Option<u64> {
        Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as u64)
    };
    let le32 = |b: &[u8], i: usize| -> Option<u64> {
        Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as u64)
    };
    let search_from = bytes.len().saturating_sub(22 + 65_535);
    let eocd = (search_from..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| bytes[i..].starts_with(b"PK\x05\x06"))
        .ok_or("no ZIP end-of-directory record")?;
    let count = le16(bytes, eocd + 10).ok_or("truncated ZIP")? as usize;
    let mut at = le32(bytes, eocd + 16).ok_or("truncated ZIP")? as usize;
    if count > 10_000 {
        return Err("the archive lists more than 10 000 entries".into());
    }
    let mut report = ZipReport {
        entries: Vec::new(),
        unsafe_entries: Vec::new(),
        total_uncompressed: 0,
    };
    for _ in 0..count {
        if !bytes
            .get(at..)
            .is_some_and(|b| b.starts_with(b"PK\x01\x02"))
        {
            return Err("malformed ZIP central directory".into());
        }
        let size = le32(bytes, at + 24).ok_or("truncated ZIP")?;
        let name_len = le16(bytes, at + 28).ok_or("truncated ZIP")? as usize;
        let extra = le16(bytes, at + 30).ok_or("truncated ZIP")? as usize;
        let comment = le16(bytes, at + 32).ok_or("truncated ZIP")? as usize;
        let attrs = le32(bytes, at + 38).ok_or("truncated ZIP")?;
        let name_bytes = bytes
            .get(at + 46..at + 46 + name_len)
            .ok_or("truncated ZIP")?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        let unix_mode = attrs >> 16;
        let is_symlink = unix_mode & 0o170000 == 0o120000;
        // Allow-list: documents, images, text and boardviews. Anything else
        // (programs, scripts, installers, unknown types) marks the archive.
        let is_dir = name.ends_with('/');
        let base = name
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let plain_text_name = ["readme", "license", "licence", "changelog"]
            .iter()
            .any(|n| base == *n);
        let is_exec_name = !is_dir
            && !plain_text_name
            && !extension(&name).is_some_and(|e| {
                ARCHIVE_ENTRY_EXTENSIONS.contains(&e.as_str())
                    || BOARDVIEW_EXTENSIONS.contains(&e.as_str())
            });
        let escapes = name.starts_with('/')
            || name.starts_with('\\')
            || name.contains('\\')
            || name.split('/').any(|part| part == "..")
            || name.chars().nth(1) == Some(':')
            || name.chars().any(char::is_control);
        if escapes || is_symlink || is_exec_name {
            report.unsafe_entries.push(name.chars().take(120).collect());
        }
        report.total_uncompressed = report.total_uncompressed.saturating_add(size);
        report.entries.push(ZipEntry {
            name,
            uncompressed: size,
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(report)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn types_from_words_and_extensions() {
        assert_eq!(
            doc_type("Dell Inspiron 15 3535 Schematic", Some("pdf")),
            DocumentType::Schematic
        );
        assert_eq!(doc_type("0N90HF", Some("brd")), DocumentType::Boardview);
        assert_eq!(
            doc_type("inspiron-15-3535-owners-manual-en-us", Some("pdf")),
            DocumentType::ServiceManual
        );
        assert_eq!(
            doc_type("ThinkPad T480 Hardware Maintenance Manual", None),
            DocumentType::ServiceManual
        );
        assert_eq!(
            doc_type("Inspiron 3535 Setup and Specifications", None),
            DocumentType::SpecificationSheet
        );
        assert_eq!(
            doc_type("BIOS release notes 1.32.0", None),
            DocumentType::FirmwareDocumentation
        );
        assert_eq!(doc_type("Photos of my laptop", None), DocumentType::Unknown);
        // Word boundaries: "schematical" is not a schematic.
        assert_eq!(
            doc_type("schematically speaking", None),
            DocumentType::Unknown
        );
    }

    #[test]
    fn hints_are_never_content() {
        assert_eq!(
            format_from_hints(Some("pdf"), None),
            (FileFormat::Pdf, FormatBasis::Extension)
        );
        assert_eq!(
            format_from_hints(None, Some("application/pdf; charset=binary")),
            (FileFormat::Pdf, FormatBasis::ContentType)
        );
        assert_eq!(
            format_from_hints(None, None),
            (FileFormat::Unknown, FormatBasis::Unknown)
        );
        assert_eq!(
            extension("https://x.example/a/b.PDF?x=1#p"),
            Some("pdf".into())
        );
        assert_eq!(extension("https://x.example/download"), None);
    }

    #[test]
    fn content_decides_and_rejects_executables_and_pages() {
        assert_eq!(
            format_from_content(b"%PDF-1.7\n...", Some("zip")),
            Ok(FileFormat::Pdf)
        );
        assert!(
            format_from_content(b"\x7fELF\x02\x01", Some("pdf"))
                .unwrap_err()
                .contains("executable")
        );
        assert!(format_from_content(b"MZ\x90\x00", Some("pdf")).is_err());
        assert!(format_from_content(b"#!/bin/sh\nrm -rf", Some("pdf")).is_err());
        assert!(
            format_from_content(b"  <!DOCTYPE html><html>login", Some("pdf"))
                .unwrap_err()
                .contains("web page")
        );
        assert!(format_from_content(b"", Some("pdf")).is_err());
        assert_eq!(
            format_from_content(b"\x00\x01binary", Some("brd")),
            Ok(FileFormat::Boardview)
        );
        assert!(
            format_from_content(b"\x00\x01binary", Some("pdf")).is_err(),
            "a PDF name without PDF bytes is malformed"
        );
    }

    #[test]
    fn unsafe_filenames_are_neutralised() {
        assert_eq!(
            sanitise_filename("../../etc/passwd").as_deref(),
            Some("passwd")
        );
        assert_eq!(
            sanitise_filename("..%2F..%2Fhome%2Fx.pdf").as_deref(),
            Some("x.pdf")
        );
        assert_eq!(
            sanitise_filename("C:\\Windows\\evil.pdf").as_deref(),
            Some("evil.pdf")
        );
        assert_eq!(
            sanitise_filename(".hidden.pdf").as_deref(),
            Some("hidden.pdf")
        );
        assert_eq!(
            sanitise_filename("a\u{0}b\n.pdf").as_deref(),
            Some("ab.pdf")
        );
        assert_eq!(sanitise_filename(".."), None);
        assert_eq!(sanitise_filename(&"x".repeat(500)).unwrap().len(), 120);
    }

    #[test]
    fn pdf_titles_are_read_without_parsing_streams() {
        assert_eq!(
            pdf_title(
                b"%PDF-1.4 1 0 obj << /Title (Inspiron 15 3535 Service Manual) /Author (Dell) >>"
            )
            .as_deref(),
            Some("Inspiron 15 3535 Service Manual")
        );
        assert_eq!(
            pdf_title(b"<< /Title (A \\(nested\\) title) >>").as_deref(),
            Some("A (nested) title")
        );
        assert_eq!(pdf_title(b"no info here"), None);
    }

    /// A minimal stored ZIP with the given entry names.
    pub fn zip_with(names: &[&str]) -> Vec<u8> {
        let mut local = Vec::new();
        let mut central = Vec::new();
        for name in names {
            let offset = local.len() as u32;
            local.extend_from_slice(b"PK\x03\x04");
            local.extend_from_slice(&[
                20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]);
            local.extend_from_slice(&(name.len() as u16).to_le_bytes());
            local.extend_from_slice(&[0, 0]);
            local.extend_from_slice(name.as_bytes());
            central.extend_from_slice(b"PK\x01\x02");
            central
                .extend_from_slice(&[20, 3, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&5u32.to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&((0o100644u32) << 16).to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = local.len() as u32;
        let mut out = local;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn archive_paths_are_checked_without_extracting() {
        let ok = inspect_zip(&zip_with(&[
            "board/",
            "board/0N90HF.pdf",
            "board/readme.txt",
            "board/0N90HF.brd",
            "README",
        ]))
        .unwrap();
        assert_eq!(ok.entries.len(), 5);
        assert!(ok.unsafe_entries.is_empty());
        let bad = inspect_zip(&zip_with(&[
            "../../.bashrc",
            "/etc/cron.d/x",
            "a\\..\\b",
            "C:/x.pdf",
            "tool.exe",
            "setup.sh",
            "binary",
            "fine.pdf",
        ]))
        .unwrap();
        assert_eq!(bad.unsafe_entries.len(), 7);
        assert!(inspect_zip(b"PK\x03\x04 truncated").is_err());
    }
}
