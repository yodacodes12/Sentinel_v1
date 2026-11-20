//! Documentation catalogues: JSON indexes at HTTPS addresses the user
//! configures (a manufacturer's documentation index, an organisation's
//! repair library, a community-maintained list). The format is documented
//! in docs/DOCUMENT_DISCOVERY.md.
//!
//! A catalogue's claims about a document (models, boards, revisions) are
//! used as matching evidence; its claims about who published it are not:
//! trust is decided from the document's own host.

use serde::Deserialize;

use crate::matching::Claims;
use crate::model::{ProviderInfo, SourceCategory};
use crate::net::validate_url;
use crate::provider::{Provider, ProviderError, RawCandidate, SearchContext};
use crate::vendors;

pub const ID: &str = "catalogue";
pub const FORMAT: &str = "sentinel-document-catalogue";
pub const MAX_CATALOGUE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Deserialize)]
struct Catalogue {
    format: String,
    version: u32,
    #[serde(default)]
    publisher: Option<String>,
    documents: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    title: String,
    url: Option<String>,
    page_url: Option<String>,
    #[serde(default)]
    manufacturer: Option<String>,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    boards: Vec<String>,
    #[serde(default)]
    revisions: Vec<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    description: Option<String>,
}

pub struct CatalogueProvider {
    pub urls: Vec<String>,
}

/// Parse one catalogue and keep the entries that could concern this
/// manufacturer. Entries with invalid addresses are dropped and counted.
pub fn parse(
    bytes: &[u8],
    ours: Option<&str>,
) -> Result<(Vec<RawCandidate>, usize), ProviderError> {
    let cat: Catalogue = serde_json::from_slice(bytes)
        .map_err(|e| ProviderError::InvalidResponse(format!("not a document catalogue: {e}")))?;
    if cat.format != FORMAT || cat.version != 1 {
        return Err(ProviderError::InvalidResponse(format!(
            "unsupported catalogue format {} version {}",
            cat.format, cat.version
        )));
    }
    if cat.documents.len() > MAX_ENTRIES {
        return Err(ProviderError::InvalidResponse(format!(
            "the catalogue lists more than {MAX_ENTRIES} documents"
        )));
    }
    let mut dropped = 0;
    let mut out = Vec::new();
    for e in cat.documents {
        let doc = e.url.as_deref().map(validate_url);
        let page = e.page_url.as_deref().map(validate_url);
        if matches!(doc, Some(Err(_)))
            || matches!(page, Some(Err(_)))
            || (doc.is_none() && page.is_none())
            || e.title.trim().is_empty()
        {
            dropped += 1;
            continue;
        }
        // Another recognised manufacturer's entries are of no use here.
        let theirs = e
            .manufacturer
            .as_deref()
            .and_then(vendors::vendor_for)
            .map(|v| v.key);
        if let (Some(t), Some(o)) = (theirs, ours)
            && t != o
        {
            continue;
        }
        let clip = |v: Vec<String>| {
            v.into_iter()
                .take(64)
                .map(|s| s.chars().take(80).collect())
                .collect()
        };
        out.push(RawCandidate {
            title: e.title.chars().take(300).collect(),
            document_url: doc.and_then(Result::ok).map(|u| u.to_string()),
            page_url: page.and_then(Result::ok).map(|u| u.to_string()),
            filename: e
                .filename
                .as_deref()
                .and_then(crate::classify::sanitise_filename),
            size_bytes: e.size_bytes,
            description: e.description.map(|d| d.chars().take(500).collect()),
            claims: Some(Claims {
                manufacturer: e.manufacturer,
                models: clip(e.models),
                boards: clip(e.boards),
                revisions: clip(e.revisions),
            }),
            publisher: cat.publisher.clone(),
            ..Default::default()
        });
    }
    Ok((out, dropped))
}

impl Provider for CatalogueProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID.into(),
            name: "Documentation catalogues".into(),
            category: SourceCategory::Unknown,
            online: true,
            enabled: true,
            configured: !self.urls.is_empty(),
            description: format!(
                "{} catalogue address(es) configured. Each is read once per search.",
                self.urls.len()
            ),
        }
    }

    fn search(&self, ctx: &SearchContext) -> Result<Vec<RawCandidate>, ProviderError> {
        let client = ctx
            .client
            .ok_or_else(|| ProviderError::NotConfigured("online discovery is off".into()))?;
        if self.urls.is_empty() {
            return Err(ProviderError::NotConfigured(
                "no catalogue address is configured".into(),
            ));
        }
        let ours = ctx.identity.manufacturer.as_ref().map(|m| m.key.as_str());
        let mut out = Vec::new();
        let mut last_error = None;
        for url in &self.urls {
            match client
                .fetch_bytes(url, "application/json", MAX_CATALOGUE_BYTES, ctx.cancel)
                .map_err(ProviderError::from)
                .and_then(|(b, _)| parse(&b, ours))
            {
                Ok((mut found, dropped)) => {
                    if dropped > 0 {
                        tracing::info!(
                            dropped,
                            "catalogue entries with unusable addresses were ignored"
                        );
                    }
                    out.append(&mut found);
                }
                Err(ProviderError::Net(crate::net::NetError::Cancelled)) => {
                    return Err(ProviderError::Net(crate::net::NetError::Cancelled));
                }
                Err(e) => last_error = Some(e),
            }
        }
        match (out.is_empty(), last_error) {
            (true, Some(e)) => Err(e),
            _ => Ok(out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::dell;
    use crate::net::tests::{Fake, client};
    use crate::net::{Cancel, NetError};
    use std::sync::Arc;

    pub const SAMPLE: &str = r#"{"format":"sentinel-document-catalogue","version":1,"publisher":"Example index","documents":[
      {"title":"Inspiron 15 3535 Owner's Manual","url":"https://dl.dell.com/manuals/inspiron-15-3535-owners-manual.pdf","manufacturer":"Dell","models":["Inspiron 15 3535"]},
      {"title":"Board 0N90HF schematic","page_url":"https://docs.example.org/boards/0N90HF","manufacturer":"Dell Inc.","boards":["0N90HF"],"revisions":["A02"]},
      {"title":"ThinkPad T480 HMM","url":"https://download.lenovo.com/t480.pdf","manufacturer":"Lenovo","models":["ThinkPad T480"]},
      {"title":"Local trick","url":"https://192.168.1.10/x.pdf"},
      {"title":"","url":"https://dl.dell.com/y.pdf"},
      {"title":"No address"}
    ]}"#;

    #[test]
    fn parses_filters_and_validates() {
        let (found, dropped) = parse(SAMPLE.as_bytes(), Some("dell")).unwrap();
        assert_eq!(found.len(), 2, "Lenovo entry filtered");
        assert_eq!(dropped, 3);
        assert_eq!(found[1].claims.as_ref().unwrap().boards, ["0N90HF"]);
        assert!(
            parse(
                b"{\"format\":\"other\",\"version\":1,\"documents\":[]}",
                None
            )
            .is_err()
        );
        assert!(parse(b"<html>", None).is_err());
    }

    #[test]
    fn provider_success_empty_failure_and_offline() {
        let id = dell();
        let fake = Arc::new(Fake::default());
        fake.ok(
            "https://index.example.org/docs.json",
            SAMPLE.as_bytes(),
            "application/json",
        );
        fake.ok(
            "https://empty.example.org/docs.json",
            br#"{"format":"sentinel-document-catalogue","version":1,"documents":[]}"#,
            "application/json",
        );
        fake.status("https://broken.example.org/docs.json", 500, None);
        let c = client(fake.clone());
        let cancel = Cancel::new();
        let ctx = SearchContext {
            identity: &id,
            queries: &[],
            client: Some(&c),
            cancel: &cancel,
        };
        let p = |urls: &[&str]| CatalogueProvider {
            urls: urls.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(
            p(&["https://index.example.org/docs.json"])
                .search(&ctx)
                .unwrap()
                .len(),
            2
        );
        assert!(
            p(&["https://empty.example.org/docs.json"])
                .search(&ctx)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            p(&["https://broken.example.org/docs.json"])
                .search(&ctx)
                .unwrap_err(),
            ProviderError::Net(NetError::Http(500))
        );
        // One broken catalogue does not hide another's results.
        assert_eq!(
            p(&[
                "https://broken.example.org/docs.json",
                "https://index.example.org/docs.json"
            ])
            .search(&ctx)
            .unwrap()
            .len(),
            2
        );
        let offline = SearchContext {
            identity: &id,
            queries: &[],
            client: None,
            cancel: &cancel,
        };
        assert!(matches!(
            p(&["https://index.example.org/docs.json"]).search(&offline),
            Err(ProviderError::NotConfigured(_))
        ));
        assert!(matches!(
            p(&[]).search(&ctx),
            Err(ProviderError::NotConfigured(_))
        ));
    }
}
