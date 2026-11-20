//! User-requested downloads into Sentinel's own document directory, and the
//! checks made before a stored file is opened.
//!
//! Every remote file is untrusted:
//! - the address goes through [`Client::fetch`] (HTTPS, public hosts,
//!   re-validated redirects, robots.txt, rate limit);
//! - the body is streamed to a private temporary file, capped at the
//!   configured size, and deleted on any failure or cancellation;
//! - the format is decided from the bytes; web pages, executables and
//!   scripts are rejected whatever the name or `Content-Type` says;
//! - a ZIP archive is inspected (names only) and refused if any entry could
//!   escape a folder or run code; archives are never extracted;
//! - the stored name is `<sha256>.<ext>`, never the remote name, and an
//!   existing file is never overwritten (an identical file is reused);
//! - nothing downloaded is executed or opened automatically.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::classify::{self, ZipReport};
use crate::model::FileFormat;
use crate::net::{Cancel, Client, NetError};

pub const ACCEPT: &str = "application/pdf, application/zip, application/octet-stream;q=0.8";
const HEAD_KEEP: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub file: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub format: FileFormat,
    pub zip: Option<ZipReport>,
    /// An identical file was already stored; it was reused.
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Net(#[from] NetError),
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("could not store the file: {0}")]
    Storage(String),
}

fn ext_for(format: FileFormat, hinted: Option<&str>) -> &'static str {
    match format {
        FileFormat::Pdf => "pdf",
        FileFormat::Zip => "zip",
        FileFormat::Boardview => classify::BOARDVIEW_EXTENSIONS
            .iter()
            .find(|e| Some(**e) == hinted)
            .copied()
            .unwrap_or("bin"),
        _ => "bin",
    }
}

/// Hashes and keeps the first bytes while writing to the temporary file.
struct Tee<'a> {
    file: &'a mut fs::File,
    hasher: Sha256,
    head: Vec<u8>,
    total: u64,
}

impl Write for Tee<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hasher.update(&buf[..n]);
        if self.head.len() < HEAD_KEEP {
            let take = (HEAD_KEEP - self.head.len()).min(n);
            self.head.extend_from_slice(&buf[..take]);
        }
        self.total += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

pub fn ensure_dir(dir: &Path) -> Result<(), DownloadError> {
    fs::create_dir_all(dir).map_err(|e| DownloadError::Storage(e.to_string()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| DownloadError::Storage(e.to_string()))
}

/// Download `url` into `dir`. `hinted_ext` is the extension the source
/// suggested (used only to name a recognised boardview).
pub fn download(
    client: &Client,
    url: &str,
    dir: &Path,
    max_bytes: u64,
    hinted_ext: Option<&str>,
    cancel: &Cancel,
) -> Result<Downloaded, DownloadError> {
    ensure_dir(dir)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let partial = dir.join(format!(".partial-{}-{nanos}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&partial)
            .map_err(|e| DownloadError::Storage(e.to_string()))?;
        let mut tee = Tee {
            file: &mut file,
            hasher: Sha256::new(),
            head: Vec::new(),
            total: 0,
        };
        client.fetch(url, ACCEPT, max_bytes, &mut tee, cancel)?;
        tee.flush()
            .map_err(|e| DownloadError::Storage(e.to_string()))?;
        let (sha, size, head) = (
            format!("{:x}", tee.hasher.finalize_reset()),
            tee.total,
            std::mem::take(&mut tee.head),
        );
        drop(tee);
        file.sync_all()
            .map_err(|e| DownloadError::Storage(e.to_string()))?;
        let format =
            classify::format_from_content(&head, hinted_ext).map_err(DownloadError::Rejected)?;
        let zip = if format == FileFormat::Zip {
            let mut bytes = Vec::new();
            fs::File::open(&partial)
                .and_then(|f| f.take(max_bytes).read_to_end(&mut bytes))
                .map_err(|e| DownloadError::Storage(e.to_string()))?;
            let report = classify::inspect_zip(&bytes)
                .map_err(|e| DownloadError::Rejected(format!("malformed archive: {e}")))?;
            if !report.unsafe_entries.is_empty() {
                return Err(DownloadError::Rejected(format!(
                    "the archive contains entries that could escape a folder or run code: {}",
                    report.unsafe_entries.join(", ")
                )));
            }
            Some(report)
        } else {
            None
        };
        let name = format!("{sha}.{}", ext_for(format, hinted_ext));
        let target = dir.join(&name);
        // hard_link refuses to replace an existing file: no overwrite race.
        let duplicate = match fs::hard_link(&partial, &target) {
            Ok(()) => false,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                verify_stored(dir, &name).map_err(|why| {
                    DownloadError::Storage(format!("a different file already has this name: {why}"))
                })?;
                true
            }
            Err(e) => return Err(DownloadError::Storage(e.to_string())),
        };
        Ok(Downloaded {
            file: name,
            sha256: sha,
            size_bytes: size,
            format,
            zip,
            duplicate,
        })
    })();
    let _ = fs::remove_file(&partial);
    result
}

/// Check a stored file before it is opened or reported: the name has the
/// form Sentinel gives it, it is a regular file directly inside `dir` (no
/// link), its bytes still have its format, and its hash still matches.
/// `<64 lower-case hex>.<known extension>`: the only names Sentinel gives.
pub fn is_stored_name(name: &str) -> bool {
    name.split_once('.').is_some_and(|(hash, ext)| {
        hash.len() == 64
            && hash
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
            && (classify::acceptable_extension(ext) || ext == "bin")
    })
}

/// Delete one of Sentinel's stored documents: only a name Sentinel gave,
/// only a regular file (never a link), only directly inside `dir`.
pub fn remove_stored(dir: &Path, name: &str) -> Result<(), String> {
    if !is_stored_name(name) {
        return Err("not a stored document name".into());
    }
    let path = dir.join(name);
    let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".into());
    }
    fs::remove_file(&path).map_err(|e| e.to_string())
}

pub fn verify_stored(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if !is_stored_name(name) {
        return Err("not a stored document name".into());
    }
    let (hash, ext) = name.split_once('.').ok_or("not a stored document name")?;
    let path = dir.join(name);
    let meta = fs::symlink_metadata(&path).map_err(|e| format!("the file is missing: {e}"))?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".into());
    }
    let canon_dir = fs::canonicalize(dir).map_err(|e| e.to_string())?;
    let canon = fs::canonicalize(&path).map_err(|e| e.to_string())?;
    if canon.parent() != Some(canon_dir.as_path()) {
        return Err("the file is outside Sentinel's document folder".into());
    }
    let mut f = fs::File::open(&canon).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut head = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if head.len() < HEAD_KEEP {
            head.extend_from_slice(&buf[..n.min(HEAD_KEEP - head.len())]);
        }
        hasher.update(&buf[..n]);
    }
    if format!("{:x}", hasher.finalize()) != hash {
        return Err("the file's content changed since it was downloaded".into());
    }
    let expected = match ext {
        "pdf" => Some(FileFormat::Pdf),
        "zip" => Some(FileFormat::Zip),
        _ => None,
    };
    let found = classify::format_from_content(&head, Some(ext))?;
    if expected.is_some_and(|e| e != found) {
        return Err("the file's content does not match its type".into());
    }
    Ok(canon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::tests::zip_with;
    use crate::net::tests::{Fake, client};
    use std::sync::Arc;

    fn setup() -> (tempfile::TempDir, Arc<Fake>, Client) {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let c = client(fake.clone());
        (dir, fake, c)
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.starts_with(".partial"))
            .collect()
    }

    #[test]
    fn a_pdf_is_stored_under_its_hash_and_deduplicated() {
        let (dir, fake, c) = setup();
        let pdf = b"%PDF-1.7\nsome document\n%%EOF";
        fake.ok(
            "https://dl.dell.com/x/../manual.pdf",
            pdf,
            "application/pdf",
        );
        fake.ok("https://dl.dell.com/manual.pdf", pdf, "application/pdf");
        let d = download(
            &c,
            "https://dl.dell.com/manual.pdf",
            dir.path(),
            1000,
            Some("pdf"),
            &Cancel::new(),
        )
        .unwrap();
        assert_eq!(d.format, FileFormat::Pdf);
        assert_eq!(d.size_bytes, pdf.len() as u64);
        assert_eq!(d.file, format!("{}.pdf", d.sha256));
        assert!(!d.duplicate);
        let again = download(
            &c,
            "https://dl.dell.com/manual.pdf",
            dir.path(),
            1000,
            Some("pdf"),
            &Cancel::new(),
        )
        .unwrap();
        assert!(again.duplicate);
        assert_eq!(again.file, d.file);
        assert!(verify_stored(dir.path(), &d.file).is_ok());
        assert!(leftovers(dir.path()).is_empty());
        let mode = fs::metadata(dir.path().join(&d.file))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn oversized_html_executables_and_malformed_files_are_rejected() {
        let (dir, fake, c) = setup();
        fake.ok(
            "https://dl.dell.com/big.pdf",
            &vec![b'%'; 2000],
            "application/pdf",
        );
        fake.ok(
            "https://dl.dell.com/login.pdf",
            b"<!DOCTYPE html><title>Sign in</title>",
            "application/pdf",
        );
        fake.ok(
            "https://dl.dell.com/tool.pdf",
            b"\x7fELF\x02\x01\x01",
            "application/pdf",
        );
        fake.ok(
            "https://dl.dell.com/broken.pdf",
            b"garbage bytes",
            "application/pdf",
        );
        let cancel = Cancel::new();
        assert_eq!(
            download(
                &c,
                "https://dl.dell.com/big.pdf",
                dir.path(),
                1000,
                Some("pdf"),
                &cancel
            )
            .unwrap_err(),
            DownloadError::Net(NetError::TooLarge(1000))
        );
        assert!(
            matches!(download(&c, "https://dl.dell.com/login.pdf", dir.path(), 1000, Some("pdf"), &cancel), Err(DownloadError::Rejected(m)) if m.contains("web page"))
        );
        assert!(
            matches!(download(&c, "https://dl.dell.com/tool.pdf", dir.path(), 1000, Some("pdf"), &cancel), Err(DownloadError::Rejected(m)) if m.contains("executable"))
        );
        assert!(matches!(
            download(
                &c,
                "https://dl.dell.com/broken.pdf",
                dir.path(),
                1000,
                Some("pdf"),
                &cancel
            ),
            Err(DownloadError::Rejected(_))
        ));
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            0,
            "nothing kept, no partial files"
        );
    }

    #[test]
    fn invalid_private_and_redirected_destinations() {
        let (dir, fake, c) = setup();
        let cancel = Cancel::new();
        for bad in [
            "not a url",
            "http://dl.dell.com/x.pdf",
            "https://127.0.0.1/x.pdf",
            "https://[::1]/x.pdf",
            "https://router.lan/x.pdf",
            "file:///etc/shadow",
        ] {
            assert!(
                download(&c, bad, dir.path(), 1000, None, &cancel).is_err(),
                "{bad}"
            );
        }
        fake.status(
            "https://dl.dell.com/redirect.pdf",
            302,
            Some("https://10.0.0.1/internal.pdf"),
        );
        assert!(matches!(
            download(
                &c,
                "https://dl.dell.com/redirect.pdf",
                dir.path(),
                1000,
                None,
                &cancel
            ),
            Err(DownloadError::Net(NetError::Blocked(_)))
        ));
        assert_eq!(fake.hits("https://10.0.0.1"), 0);
    }

    #[test]
    fn interrupted_downloads_leave_nothing_behind() {
        let (dir, fake, c) = setup();
        fake.fail(
            "https://dl.dell.com/cut.pdf",
            NetError::Unreachable("connection reset".into()),
        );
        assert!(
            download(
                &c,
                "https://dl.dell.com/cut.pdf",
                dir.path(),
                1000,
                None,
                &Cancel::new()
            )
            .is_err()
        );
        let cancel = Cancel::new();
        cancel.cancel();
        fake.ok("https://dl.dell.com/ok.pdf", b"%PDF-1.4", "application/pdf");
        assert_eq!(
            download(
                &c,
                "https://dl.dell.com/ok.pdf",
                dir.path(),
                1000,
                None,
                &cancel
            )
            .unwrap_err(),
            DownloadError::Net(NetError::Cancelled)
        );
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn archives_with_unsafe_paths_are_refused_safe_ones_kept_unextracted() {
        let (dir, fake, c) = setup();
        fake.ok(
            "https://dl.dell.com/evil.zip",
            &zip_with(&["../../.config/autostart/x.desktop"]),
            "application/zip",
        );
        fake.ok(
            "https://dl.dell.com/good.zip",
            &zip_with(&["0N90HF/schematic.pdf"]),
            "application/zip",
        );
        let cancel = Cancel::new();
        assert!(
            matches!(download(&c, "https://dl.dell.com/evil.zip", dir.path(), 10_000, Some("zip"), &cancel), Err(DownloadError::Rejected(m)) if m.contains("escape"))
        );
        let ok = download(
            &c,
            "https://dl.dell.com/good.zip",
            dir.path(),
            10_000,
            Some("zip"),
            &cancel,
        )
        .unwrap();
        assert_eq!(ok.zip.unwrap().entries.len(), 1);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "the archive itself only; nothing extracted");
    }

    #[test]
    fn stored_files_are_verified_before_use() {
        let (dir, fake, c) = setup();
        fake.ok(
            "https://dl.dell.com/m.pdf",
            b"%PDF-1.4 x",
            "application/pdf",
        );
        let d = download(
            &c,
            "https://dl.dell.com/m.pdf",
            dir.path(),
            1000,
            None,
            &Cancel::new(),
        )
        .unwrap();
        assert!(verify_stored(dir.path(), "../../etc/passwd").is_err());
        assert!(verify_stored(dir.path(), "x.pdf").is_err());
        // Tampered content is detected.
        fs::write(dir.path().join(&d.file), b"%PDF-1.4 changed").unwrap();
        assert!(
            verify_stored(dir.path(), &d.file)
                .unwrap_err()
                .contains("changed")
        );
        // A link in place of the file is refused.
        fs::remove_file(dir.path().join(&d.file)).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", dir.path().join(&d.file)).unwrap();
        assert!(verify_stored(dir.path(), &d.file).is_err());
        // Removal refuses links and foreign names.
        assert!(remove_stored(dir.path(), &d.file).is_err());
        assert!(remove_stored(dir.path(), "../x.pdf").is_err());
        assert!(!is_stored_name(&format!("{}.exe", "a".repeat(64))));
    }

    /// Real network, never in routine runs:
    /// `cargo test -p sentinel-docs -- --ignored live_official`.
    /// A generic public model (ThinkPad T480), not the machine running it:
    /// a catalogue entry for the manufacturer's own manual is matched,
    /// trusted from its host, downloaded through every check and verified.
    #[test]
    #[ignore = "downloads a public manual from lenovo.com"]
    fn live_official_manufacturer_download() {
        use crate::identity::tests::{board, inventory};
        use crate::model::{AccessStatus, MatchClass, SearchIdentity, SourceTrust};
        use crate::net::{HttpsTransport, Limits};
        use crate::provider::{Provider, SearchContext};
        use std::time::Duration;
        let mut inv = inventory(board(
            Some("LENOVO"),
            Some("20L5CTO1WW"),
            Some("ThinkPad T480"),
            Some("LENOVO"),
            Some("20L5CTO1WW"),
            None,
        ));
        inv.system.as_mut().unwrap().product_version = sentinel_inventory::Field::detected(
            "ThinkPad T480".to_string(),
            "dmi",
            sentinel_inventory::Confidence::FirmwareReported,
        );
        let id = SearchIdentity::from_inventory(&inv).unwrap();
        let url = "https://psref.lenovo.com/syspool/Sys/PDF/ThinkPad/ThinkPad_T480/ThinkPad_T480_Spec.PDF";
        let catalogue = format!(
            r#"{{"format":"sentinel-document-catalogue","version":1,"documents":[{{"title":"ThinkPad T480 PSREF Specifications","url":"{url}","manufacturer":"Lenovo","models":["ThinkPad T480"]}}]}}"#
        );
        let (raw, dropped) =
            crate::providers::catalogue::parse(catalogue.as_bytes(), Some("lenovo")).unwrap();
        assert_eq!((raw.len(), dropped), (1, 0));
        struct Fixed(Vec<crate::provider::RawCandidate>);
        impl Provider for Fixed {
            fn info(&self) -> crate::model::ProviderInfo {
                crate::model::ProviderInfo {
                    id: "catalogue".into(),
                    name: "catalogue".into(),
                    category: crate::model::SourceCategory::Unknown,
                    online: true,
                    enabled: true,
                    configured: true,
                    description: String::new(),
                }
            }
            fn search(
                &self,
                _: &SearchContext,
            ) -> Result<Vec<crate::provider::RawCandidate>, crate::provider::ProviderError>
            {
                Ok(self.0.clone())
            }
        }
        let client = Arc::new(Client::new(
            Arc::new(HttpsTransport::new(Duration::from_secs(60))),
            Limits {
                timeout: Duration::from_secs(60),
                min_interval: Duration::from_millis(1500),
            },
        ));
        let p: Vec<Arc<dyn Provider>> = vec![Arc::new(Fixed(raw))];
        let r = crate::engine::run(&id, &p, Some(client.clone()), &Cancel::new(), 1, &|_| {});
        let c = &r.candidates[0];
        eprintln!(
            "live: class {:?}, trust {:?}, access {:?}",
            c.assessment.class, c.source.trust, c.source.access
        );
        assert_eq!(c.source.trust, SourceTrust::OfficialThisManufacturer);
        assert_eq!(c.assessment.class, MatchClass::HighConfidence);
        assert_eq!(c.source.access, AccessStatus::Downloadable);
        assert_eq!(c.doc_type, crate::model::DocumentType::SpecificationSheet);
        let dir = tempfile::tempdir().unwrap();
        let d = download(
            &client,
            url,
            dir.path(),
            100 * 1024 * 1024,
            Some("pdf"),
            &Cancel::new(),
        )
        .unwrap();
        eprintln!(
            "live: downloaded {} bytes, {:?}, sha256 {}, {} requests",
            d.size_bytes,
            d.format,
            d.sha256,
            client.requests()
        );
        assert_eq!(d.format, FileFormat::Pdf);
        assert!(verify_stored(dir.path(), &d.file).is_ok());
        assert!(
            download(
                &client,
                url,
                dir.path(),
                100 * 1024 * 1024,
                Some("pdf"),
                &Cancel::new()
            )
            .unwrap()
            .duplicate
        );
        assert!(matches!(
            download(&client, url, dir.path(), 1024, Some("pdf"), &Cancel::new()),
            Err(DownloadError::Net(NetError::TooLarge(1024)))
        ));
    }
}
