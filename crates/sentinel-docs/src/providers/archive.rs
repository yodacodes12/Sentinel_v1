//! The Internet Archive's public search API (`advancedsearch.php`, JSON).
//! A documented, unauthenticated API; requests are rate limited and
//! robots.txt is honoured by the client.
//!
//! Anyone can upload to the Archive and Sentinel cannot verify who
//! published an item or whether it may be shared, so results are kept as
//! references to the item page only: nothing is downloaded from it.

use serde::Deserialize;

use crate::model::{ProviderInfo, QueryPurpose, SourceCategory};
use crate::net::NetError;
use crate::provider::{Provider, ProviderError, RawCandidate, SearchContext};

pub const ID: &str = "internet_archive";
pub const ENDPOINT: &str = "https://archive.org/advancedsearch.php";
pub const ROWS: u32 = 10;
pub const MAX_QUERIES: usize = 4;
const MAX_BODY: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Answer {
    response: Docs,
}

#[derive(Deserialize)]
struct Docs {
    docs: Vec<Doc>,
}

#[derive(Deserialize)]
struct Doc {
    identifier: String,
    #[serde(default)]
    title: Option<serde_json::Value>,
    #[serde(default)]
    description: Option<serde_json::Value>,
}

fn text(v: &Option<serde_json::Value>) -> Option<String> {
    match v.as_ref()? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(a) => a.iter().find_map(|x| x.as_str().map(str::to_string)),
        _ => None,
    }
}

fn valid_identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// The search address for one query: the query's words must occur in the
/// item, and only text items are searched.
pub fn search_url(query: &str) -> String {
    let q = format!("({query}) AND mediatype:(texts)");
    let mut url = url::Url::parse(ENDPOINT).expect("valid endpoint");
    url.query_pairs_mut()
        .append_pair("q", &q)
        .append_pair("fl[]", "identifier")
        .append_pair("fl[]", "title")
        .append_pair("fl[]", "description")
        .append_pair("rows", &ROWS.to_string())
        .append_pair("page", "1")
        .append_pair("output", "json");
    url.to_string()
}

pub fn parse(bytes: &[u8], query: &str) -> Result<Vec<RawCandidate>, ProviderError> {
    let a: Answer = serde_json::from_slice(bytes)
        .map_err(|e| ProviderError::InvalidResponse(format!("unexpected search answer: {e}")))?;
    Ok(a.response
        .docs
        .into_iter()
        .filter(|d| valid_identifier(&d.identifier))
        .map(|d| RawCandidate {
            title: text(&d.title)
                .unwrap_or_else(|| d.identifier.clone())
                .chars()
                .take(300)
                .collect(),
            page_url: Some(format!("https://archive.org/details/{}", d.identifier)),
            description: text(&d.description).map(|s| s.chars().take(500).collect()),
            publisher: Some("Internet Archive".into()),
            query: Some(query.to_string()),
            ..Default::default()
        })
        .collect())
}

pub struct InternetArchive;

impl Provider for InternetArchive {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID.into(),
            name: "Internet Archive".into(),
            category: SourceCategory::Community,
            online: true,
            enabled: true,
            configured: true,
            description: "Public search of archive.org text items. Community uploads: references only, never downloaded.".into(),
        }
    }

    fn search(&self, ctx: &SearchContext) -> Result<Vec<RawCandidate>, ProviderError> {
        let client = ctx
            .client
            .ok_or_else(|| ProviderError::NotConfigured("online discovery is off".into()))?;
        // The search requires every word, so identifier-only queries are
        // used; matching then judges each result.
        let queries: Vec<_> = ctx
            .queries
            .iter()
            .filter(|q| q.purpose == QueryPurpose::Identity)
            .take(MAX_QUERIES)
            .collect();
        let mut out = Vec::new();
        for q in queries {
            match client.fetch_bytes(
                &search_url(&q.text),
                "application/json",
                MAX_BODY,
                ctx.cancel,
            ) {
                Ok((body, _)) => out.extend(parse(&body, &q.text)?),
                // Slow down for the rest of the session: stop here.
                Err(NetError::RateLimited) if !out.is_empty() => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::dell;
    use crate::net::Cancel;
    use crate::net::tests::{Fake, client};
    use crate::query::generate;
    use std::sync::Arc;

    #[test]
    fn query_address_is_encoded() {
        let u = search_url("Dell Inspiron 15 3535 schematic");
        assert!(u.starts_with("https://archive.org/advancedsearch.php?q=%28Dell+Inspiron+15+3535+schematic%29+AND+mediatype%3A%28texts%29"));
        assert!(u.contains("rows=10") && u.contains("output=json"));
    }

    #[test]
    fn parses_and_rejects_odd_identifiers() {
        let body = br#"{"response":{"numFound":3,"docs":[
          {"identifier":"inspiron-15-3535-sm","title":"Inspiron 15 3535 service manual","description":["Scanned"]},
          {"identifier":"../etc","title":"x"},
          {"identifier":"plain"}]}}"#;
        let found = parse(body, "q").unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0].page_url.as_deref(),
            Some("https://archive.org/details/inspiron-15-3535-sm")
        );
        assert_eq!(found[0].description.as_deref(), Some("Scanned"));
        assert!(
            found.iter().all(|c| c.document_url.is_none()),
            "references only"
        );
        assert!(parse(b"{\"error\":\"x\"}", "q").is_err());
    }

    #[test]
    fn timeouts_invalid_answers_and_rate_limits() {
        let id = dell();
        let queries = generate(&id);
        let fake = Arc::new(Fake::default());
        let urls: Vec<String> = queries
            .iter()
            .filter(|q| q.purpose == QueryPurpose::Identity)
            .map(|q| search_url(&q.text))
            .collect();
        fake.fail(&urls[0], NetError::Timeout);
        let c = client(fake.clone());
        let cancel = Cancel::new();
        let ctx = SearchContext {
            identity: &id,
            queries: &queries,
            client: Some(&c),
            cancel: &cancel,
        };
        assert_eq!(
            InternetArchive.search(&ctx).unwrap_err(),
            ProviderError::Net(NetError::Timeout)
        );
        fake.ok(&urls[0], b"not json", "text/html");
        assert!(matches!(
            InternetArchive.search(&ctx),
            Err(ProviderError::InvalidResponse(_))
        ));
        fake.ok(&urls[0], br#"{"response":{"docs":[{"identifier":"a","title":"Dell Inspiron 15 3535 schematic"}]}}"#, "application/json");
        let second = urls[1].clone();
        fake.status(&second, 429, None);
        let found = InternetArchive.search(&ctx).unwrap();
        assert_eq!(found.len(), 1, "results before the rate limit are kept");
        assert!(fake.hits("https://archive.org/advancedsearch.php") <= MAX_QUERIES + 1);
    }

    /// Real network, never in routine runs:
    /// `cargo test -p sentinel-docs -- --ignored live_internet_archive`.
    /// Uses a generic public model, not the machine running the test.
    #[test]
    #[ignore = "contacts archive.org"]
    fn live_internet_archive_search() {
        use crate::identity::tests::{board, inventory};
        use crate::net::{HttpsTransport, Limits};
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
        let id = crate::model::SearchIdentity::from_inventory(&inv).unwrap();
        let queries = generate(&id);
        let c = crate::net::Client::new(
            Arc::new(HttpsTransport::new(Duration::from_secs(20))),
            Limits {
                timeout: Duration::from_secs(20),
                min_interval: Duration::from_millis(1500),
            },
        );
        let cancel = Cancel::new();
        let found = InternetArchive
            .search(&SearchContext {
                identity: &id,
                queries: &queries,
                client: Some(&c),
                cancel: &cancel,
            })
            .unwrap();
        eprintln!("live: {} requests, {} results", c.requests(), found.len());
        for f in found.iter().take(5) {
            let a = crate::matching::assess(
                &id,
                &crate::matching::CandidateText {
                    title: f.title.clone(),
                    description: f.description.clone(),
                    host: Some("archive.org".into()),
                    ..Default::default()
                },
            );
            eprintln!("  {:?}  {}", a.class, f.title);
        }
    }
}
