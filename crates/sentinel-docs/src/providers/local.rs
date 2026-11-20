//! The user's document library: a folder inside Sentinel's data directory
//! where the user puts documents they already have. Works offline. Files
//! are read (bounded) to identify their format and PDF title; they are
//! never moved, changed or opened by Sentinel.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use crate::classify;
use crate::model::{FileFormat, FormatBasis, ProviderInfo, SourceCategory};
use crate::provider::{Provider, ProviderError, RawCandidate, SearchContext};

pub const ID: &str = "local_library";
pub const MAX_FILES: usize = 2000;
/// Library files with this suffix are documentation catalogues (the JSON
/// format of `providers::catalogue`), read instead of treated as documents.
pub const LOCAL_CATALOGUE_SUFFIX: &str = ".sentinel-catalogue.json";
const HEAD_BYTES: usize = 256 * 1024;
const TAIL_BYTES: u64 = 64 * 1024;

pub struct LocalLibrary {
    pub dir: PathBuf,
}

impl LocalLibrary {
    fn read_sample(path: &std::path::Path, size: u64) -> std::io::Result<Vec<u8>> {
        let mut f = fs::File::open(path)?;
        let mut buf = Vec::with_capacity(HEAD_BYTES.min(size as usize));
        (&mut f).take(HEAD_BYTES as u64).read_to_end(&mut buf)?;
        if size > HEAD_BYTES as u64 + TAIL_BYTES {
            f.seek(SeekFrom::End(-(TAIL_BYTES as i64)))?;
            f.take(TAIL_BYTES).read_to_end(&mut buf)?;
        }
        Ok(buf)
    }
}

impl Provider for LocalLibrary {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID.into(),
            name: "Document library".into(),
            category: SourceCategory::UserLibrary,
            online: false,
            enabled: true,
            configured: true,
            description: format!(
                "Documents you placed in {}. Read only; works offline.",
                self.dir.display()
            ),
        }
    }

    fn search(&self, ctx: &SearchContext) -> Result<Vec<RawCandidate>, ProviderError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(ProviderError::Io(format!(
                    "the library folder could not be read: {e}"
                )));
            }
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort();
        let mut out = Vec::new();
        for name in names.into_iter().take(MAX_FILES) {
            if ctx.cancel.is_cancelled() {
                return Err(ProviderError::Net(crate::net::NetError::Cancelled));
            }
            let path = self.dir.join(&name);
            // Never follow links out of the library; only regular files.
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !meta.file_type().is_file() {
                continue;
            }
            // A catalogue the user keeps locally: its entries are remote
            // documents, matched and trusted exactly like a web catalogue's.
            if name.ends_with(LOCAL_CATALOGUE_SUFFIX) {
                if meta.len() <= crate::providers::catalogue::MAX_CATALOGUE_BYTES
                    && let Ok(bytes) = fs::read(&path)
                {
                    let ours = ctx.identity.manufacturer.as_ref().map(|m| m.key.as_str());
                    match crate::providers::catalogue::parse(&bytes, ours) {
                        Ok((found, _)) => out.extend(found),
                        Err(e) => {
                            tracing::warn!(error = %e, "a local catalogue in the library is not valid")
                        }
                    }
                }
                continue;
            }
            let ext = classify::extension(&name);
            if !ext.as_deref().is_some_and(classify::acceptable_extension) {
                continue;
            }
            let Ok(sample) = Self::read_sample(&path, meta.len()) else {
                continue;
            };
            let Ok(format) = classify::format_from_content(&sample, ext.as_deref()) else {
                continue;
            };
            let document_title = if format == FileFormat::Pdf {
                classify::pdf_title(&sample)
            } else {
                None
            };
            let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
            out.push(RawCandidate {
                title: document_title
                    .clone()
                    .unwrap_or_else(|| stem.replace(['_', '-'], " ")),
                filename: Some(name.clone()),
                size_bytes: Some(meta.len()),
                document_title,
                library_file: Some((name, meta.len())),
                format: Some((format, FormatBasis::Content)),
                ..Default::default()
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::dell;
    use crate::net::Cancel;

    #[test]
    fn reads_documents_skips_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        fs::write(
            d.join("inspiron-15-3535-service-manual.pdf"),
            b"%PDF-1.4\n<< /Title (Inspiron 15 3535 Service Manual) >>",
        )
        .unwrap();
        fs::write(d.join("0N90HF.brd"), b"\x00\x01binary boardview").unwrap();
        fs::write(d.join("fake.pdf"), b"MZ\x90 executable").unwrap();
        fs::write(d.join("notes.txt"), b"text").unwrap();
        fs::write(d.join(".hidden.pdf"), b"%PDF-1.4").unwrap();
        fs::create_dir(d.join("sub.pdf")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.join("link.pdf")).unwrap();
        let id = dell();
        let lib = LocalLibrary {
            dir: d.to_path_buf(),
        };
        let found = lib
            .search(&SearchContext {
                identity: &id,
                queries: &[],
                client: None,
                cancel: &Cancel::new(),
            })
            .unwrap();
        let names: Vec<_> = found.iter().map(|c| c.filename.clone().unwrap()).collect();
        assert_eq!(names, ["0N90HF.brd", "inspiron-15-3535-service-manual.pdf"]);
        assert_eq!(
            found[1].document_title.as_deref(),
            Some("Inspiron 15 3535 Service Manual")
        );
        assert_eq!(
            found[0].format,
            Some((FileFormat::Boardview, FormatBasis::Content))
        );
        // A missing library is simply empty.
        let none = LocalLibrary {
            dir: d.join("absent"),
        };
        assert!(
            none.search(&SearchContext {
                identity: &id,
                queries: &[],
                client: None,
                cancel: &Cancel::new()
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn local_catalogues_list_remote_documents() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("mine.sentinel-catalogue.json"),
            r#"{"format":"sentinel-document-catalogue","version":1,"documents":[
              {"title":"Inspiron 15 3535 Owner's Manual","url":"https://dl.dell.com/m.pdf","manufacturer":"Dell","models":["Inspiron 15 3535"]},
              {"title":"bad","url":"https://10.0.0.1/x.pdf"}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("broken.sentinel-catalogue.json"),
            b"{not json",
        )
        .unwrap();
        let id = dell();
        let found = LocalLibrary {
            dir: dir.path().to_path_buf(),
        }
        .search(&SearchContext {
            identity: &id,
            queries: &[],
            client: None,
            cancel: &Cancel::new(),
        })
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].document_url.as_deref(),
            Some("https://dl.dell.com/m.pdf")
        );
        assert!(
            found[0].library_file.is_none(),
            "an entry is a remote document, not a library file"
        );
    }
}
