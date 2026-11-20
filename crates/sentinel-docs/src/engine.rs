//! One discovery session: identity → queries → providers (bounded
//! parallelism) → candidates (de-duplicated) → document types → hardware
//! matching and source trust. Persistence is the caller's (the daemon's).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::classify;
use crate::matching::{self, CandidateText};
use crate::model::{
    AccessStatus, DocumentCandidate, FileFormat, FormatBasis, MatchClass, ProviderOutcome,
    ProviderState, RetrievalStatus, SearchIdentity, SearchQuery, SessionStatus, SourceInfo,
    SourceTrust, Stage,
};
use crate::net::{Cancel, Client, NetError};
use crate::provider::{Provider, ProviderError, RawCandidate, SearchContext};
use crate::query;
use crate::trust;

/// Providers searched at the same time.
pub const MAX_PARALLEL_PROVIDERS: usize = 2;
/// Candidates kept from one session.
pub const MAX_CANDIDATES: usize = 500;

pub struct SessionResult {
    pub status: SessionStatus,
    pub queries: Vec<SearchQuery>,
    pub providers: Vec<ProviderOutcome>,
    pub candidates: Vec<DocumentCandidate>,
    pub error: Option<String>,
}

/// Normalised key: scheme and host lower case, no fragment, no trailing slash.
pub fn url_key(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut u) => {
            u.set_fragment(None);
            let mut s = u.to_string();
            while s.ends_with('/') {
                s.pop();
            }
            s
        }
        Err(_) => raw.to_string(),
    }
}

pub fn run(
    identity: &SearchIdentity,
    providers: &[Arc<dyn Provider>],
    client: Option<Arc<Client>>,
    cancel: &Cancel,
    now: u64,
    progress: &(dyn Fn(Stage) + Sync),
) -> SessionResult {
    progress(Stage::PreparingIdentity);
    progress(Stage::GeneratingQueries);
    let queries = query::generate(identity);

    progress(Stage::SearchingProviders);
    let outcomes: Mutex<Vec<(usize, ProviderOutcome, Vec<RawCandidate>)>> = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    std::thread::scope(|scope| {
        for _ in 0..MAX_PARALLEL_PROVIDERS.min(providers.len()) {
            scope.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().unwrap_or_else(|p| p.into_inner());
                        if *n >= providers.len() {
                            return;
                        }
                        *n += 1;
                        *n - 1
                    };
                    let p = &providers[i];
                    let info = p.info();
                    let started = Instant::now();
                    let before = client.as_ref().map(|c| c.requests()).unwrap_or(0);
                    let mut outcome = ProviderOutcome { provider_id: info.id.clone(), provider_name: info.name.clone(), state: ProviderState::Ok, results: 0, requests: 0, duration_ms: 0, error: None };
                    let mut found = Vec::new();
                    if cancel.is_cancelled() {
                        outcome.state = ProviderState::Cancelled;
                    } else if info.online && client.is_none() {
                        outcome.state = ProviderState::SkippedOffline;
                    } else if info.online && queries.is_empty() && info.id != crate::providers::catalogue::ID {
                        outcome.state = ProviderState::NotConfigured;
                        outcome.error = Some("The motherboard identity is too incomplete for an online search that would not return unrelated hardware.".into());
                    } else if !info.configured {
                        outcome.state = ProviderState::NotConfigured;
                    } else {
                        let ctx = SearchContext { identity, queries: &queries, client: client.as_deref(), cancel };
                        match p.search(&ctx) {
                            Ok(r) => {
                                outcome.state = if r.is_empty() { ProviderState::Empty } else { ProviderState::Ok };
                                outcome.results = r.len() as u32;
                                found = r;
                            }
                            Err(ProviderError::Net(NetError::Cancelled)) => outcome.state = ProviderState::Cancelled,
                            Err(ProviderError::NotConfigured(why)) => {
                                outcome.state = ProviderState::NotConfigured;
                                outcome.error = Some(why);
                            }
                            Err(e) => {
                                outcome.state = ProviderState::Failed;
                                outcome.error = Some(e.to_string());
                            }
                        }
                    }
                    outcome.duration_ms = started.elapsed().as_millis() as u64;
                    // Approximate when providers run in parallel on one client.
                    outcome.requests = client.as_ref().map(|c| c.requests()).unwrap_or(0).saturating_sub(before);
                    tracing::info!(provider = %outcome.provider_id, state = ?outcome.state, results = outcome.results, "document provider finished");
                    outcomes.lock().unwrap_or_else(|p| p.into_inner()).push((i, outcome, found));
                }
            });
        }
    });
    let mut outcomes = outcomes.into_inner().unwrap_or_else(|p| p.into_inner());
    outcomes.sort_by_key(|(i, ..)| *i);

    progress(Stage::CollectingCandidates);
    let mut by_key: BTreeMap<String, (usize, RawCandidate, Vec<String>)> = BTreeMap::new();
    for (i, _, found) in &outcomes {
        for raw in found {
            let key = match (&raw.library_file, &raw.document_url, &raw.page_url) {
                (Some((f, _)), _, _) => format!("library:{f}"),
                (_, Some(u), _) => url_key(u),
                (_, None, Some(u)) => url_key(u),
                _ => continue,
            };
            let entry = by_key
                .entry(key)
                .or_insert_with(|| (*i, raw.clone(), Vec::new()));
            if let Some(q) = &raw.query
                && !entry.2.contains(q)
            {
                entry.2.push(q.clone());
            }
        }
    }

    progress(Stage::IdentifyingTypes);
    progress(Stage::VerifyingMatches);
    let mut candidates: Vec<DocumentCandidate> = by_key
        .into_iter()
        .map(|(key, (i, raw, queries))| {
            build(identity, providers[i].as_ref(), key, raw, queries, now)
        })
        .collect();
    candidates.sort_by(|a, b| {
        (
            a.assessment.class,
            trust_rank(a.source.trust),
            a.doc_type,
            a.title.clone(),
        )
            .cmp(&(
                b.assessment.class,
                trust_rank(b.source.trust),
                b.doc_type,
                b.title.clone(),
            ))
    });
    candidates.truncate(MAX_CANDIDATES);

    let providers: Vec<ProviderOutcome> = outcomes.into_iter().map(|(_, o, _)| o).collect();
    let failed = providers
        .iter()
        .filter(|o| o.state == ProviderState::Failed)
        .count();
    let searched = providers
        .iter()
        .filter(|o| matches!(o.state, ProviderState::Ok | ProviderState::Empty))
        .count();
    let status = if cancel.is_cancelled() {
        SessionStatus::Cancelled
    } else if failed > 0 && searched == 0 {
        SessionStatus::Failed
    } else if failed > 0 {
        SessionStatus::Partial
    } else {
        SessionStatus::Completed
    };
    let error = (failed > 0).then(|| {
        providers
            .iter()
            .filter_map(|o| {
                o.error
                    .as_ref()
                    .filter(|_| o.state == ProviderState::Failed)
                    .map(|e| format!("{}: {e}", o.provider_name))
            })
            .collect::<Vec<_>>()
            .join("; ")
    });
    SessionResult {
        status,
        queries,
        providers,
        candidates,
        error,
    }
}

fn trust_rank(t: SourceTrust) -> u8 {
    match t {
        SourceTrust::OfficialThisManufacturer => 0,
        SourceTrust::UserProvided => 1,
        SourceTrust::Community => 2,
        SourceTrust::Unverified => 3,
        SourceTrust::OfficialOtherManufacturer => 4,
    }
}

fn host_of(u: Option<&str>) -> (Option<String>, bool) {
    match u.and_then(|u| url::Url::parse(u).ok()) {
        Some(u) => (
            u.host_str().map(str::to_ascii_lowercase),
            u.scheme() == "https",
        ),
        None => (None, false),
    }
}

fn build(
    identity: &SearchIdentity,
    provider: &dyn Provider,
    key: String,
    raw: RawCandidate,
    queries: Vec<String>,
    now: u64,
) -> DocumentCandidate {
    let info = provider.info();
    let doc_url = raw.document_url.clone();
    let (host, https) = host_of(doc_url.as_deref().or(raw.page_url.as_deref()));
    let filename = raw.filename.clone().or_else(|| {
        doc_url
            .as_deref()
            .and_then(|u| u.split(['?', '#']).next())
            .and_then(|u| u.rsplit('/').next())
            .and_then(classify::sanitise_filename)
    });
    let ext = filename
        .as_deref()
        .and_then(classify::extension)
        .or_else(|| doc_url.as_deref().and_then(classify::extension));
    let (format, basis) = raw.format.unwrap_or_else(|| {
        if doc_url.is_none() {
            (FileFormat::WebPage, FormatBasis::Unknown)
        } else {
            classify::format_from_hints(ext.as_deref(), raw.mime_type.as_deref())
        }
    });
    let type_text = [
        raw.document_title.as_deref(),
        Some(raw.title.as_str()),
        filename.as_deref(),
        raw.description.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let doc_type = classify::doc_type(&type_text, ext.as_deref());

    let t = if raw.library_file.is_some() {
        trust::user_library()
    } else {
        trust::assess_host(host.as_deref(), identity, https)
    };
    let url_path = doc_url
        .as_deref()
        .or(raw.page_url.as_deref())
        .and_then(|u| url::Url::parse(u).ok())
        .map(|u| crate::net::path_and_query(&u));
    let assessment = matching::assess(
        identity,
        &CandidateText {
            title: raw.title.clone(),
            filename: filename.clone(),
            url_path,
            description: raw.description.clone(),
            document_title: raw.document_title.clone(),
            host: if raw.library_file.is_some() {
                None
            } else {
                host.clone()
            },
            claims: raw.claims.clone(),
        },
    );
    let mut access = t.access.clone();
    if matches!(access, AccessStatus::Downloadable) && doc_url.is_none() {
        access = AccessStatus::ReferenceOnly {
            reason: "Only a page about the document is known, not the file itself.".into(),
        };
    }
    if matches!(access, AccessStatus::Downloadable) && matches!(format, FileFormat::WebPage) {
        access = AccessStatus::ReferenceOnly {
            reason: "The address is a web page, not a document file.".into(),
        };
    }
    if matches!(access, AccessStatus::Downloadable) && assessment.class == MatchClass::Mismatch {
        access = AccessStatus::ReferenceOnly {
            reason: "The document is for different hardware.".into(),
        };
    }
    let retrieval = match (&raw.library_file, &access) {
        (Some((f, size)), _) => RetrievalStatus::InLibrary {
            file: f.clone(),
            size_bytes: *size,
        },
        (None, AccessStatus::ReferenceOnly { reason }) => RetrievalStatus::ReferenceOnly {
            reason: reason.clone(),
        },
        _ => RetrievalStatus::NotDownloaded,
    };
    let mut metadata = BTreeMap::new();
    if let Some(d) = &raw.description {
        metadata.insert("description".into(), d.clone());
    }
    if let Some(d) = &raw.document_title {
        metadata.insert("document_title".into(), d.clone());
    }
    if let Some(p) = &raw.publisher {
        metadata.insert("listed_by".into(), p.clone());
    }
    if !queries.is_empty() {
        metadata.insert("found_by_queries".into(), queries.join(" | "));
    }
    DocumentCandidate {
        id: 0,
        key,
        title: raw.title,
        filename,
        extension: ext,
        mime_type: raw.mime_type,
        size_bytes: raw.size_bytes,
        doc_type,
        format,
        format_basis: basis,
        source: SourceInfo {
            provider_id: info.id,
            provider_name: info.name,
            source_name: t.source_name,
            domain: host,
            page_url: raw.page_url,
            document_url: doc_url,
            category: t.category,
            trust: t.trust,
            trust_reason: t.reason,
            access,
            usage_note: t.usage_note,
        },
        assessment,
        retrieval,
        discovered_at: now,
        verified_at: now,
        metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::dell;
    use crate::model::{ProviderInfo, SourceCategory};
    use crate::net::Limits;
    use crate::net::tests::Fake;
    use std::time::Duration;

    struct Scripted {
        id: &'static str,
        online: bool,
        result: Result<Vec<RawCandidate>, ProviderError>,
        delay_ms: u64,
    }

    impl Provider for Scripted {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: self.id.into(),
                name: self.id.into(),
                category: SourceCategory::Unknown,
                online: self.online,
                enabled: true,
                configured: true,
                description: String::new(),
            }
        }
        fn search(&self, ctx: &SearchContext) -> Result<Vec<RawCandidate>, ProviderError> {
            let end = Instant::now() + Duration::from_millis(self.delay_ms);
            while Instant::now() < end {
                if ctx.cancel.is_cancelled() {
                    return Err(ProviderError::Net(NetError::Cancelled));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            self.result.clone()
        }
    }

    fn raw(title: &str, url: &str) -> RawCandidate {
        RawCandidate {
            title: title.into(),
            document_url: Some(url.into()),
            query: Some("q".into()),
            ..Default::default()
        }
    }

    fn client() -> Arc<Client> {
        Arc::new(Client::new(
            Arc::new(Fake::default()),
            Limits {
                timeout: Duration::from_secs(1),
                min_interval: Duration::ZERO,
            },
        ))
    }

    fn arc(p: Scripted) -> Arc<dyn Provider> {
        Arc::new(p)
    }

    #[test]
    fn one_failing_provider_does_not_stop_the_others() {
        let providers = vec![
            arc(Scripted {
                id: "good",
                online: true,
                result: Ok(vec![raw(
                    "Dell Inspiron 15 3535 Owner's Manual",
                    "https://dl.dell.com/m/3535.pdf",
                )]),
                delay_ms: 0,
            }),
            arc(Scripted {
                id: "bad",
                online: true,
                result: Err(ProviderError::Net(NetError::Http(503))),
                delay_ms: 0,
            }),
            arc(Scripted {
                id: "empty",
                online: true,
                result: Ok(vec![]),
                delay_ms: 0,
            }),
        ];
        let r = run(
            &dell(),
            &providers,
            Some(client()),
            &Cancel::new(),
            100,
            &|_| {},
        );
        assert_eq!(r.status, SessionStatus::Partial);
        assert_eq!(
            r.providers.iter().map(|p| p.state).collect::<Vec<_>>(),
            [
                ProviderState::Ok,
                ProviderState::Failed,
                ProviderState::Empty
            ]
        );
        assert_eq!(r.candidates.len(), 1);
        let c = &r.candidates[0];
        assert_eq!(c.source.trust, SourceTrust::OfficialThisManufacturer);
        assert_eq!(c.assessment.class, MatchClass::HighConfidence);
        assert_eq!(c.doc_type, crate::model::DocumentType::ServiceManual);
        assert_eq!(
            (c.format, c.format_basis),
            (FileFormat::Pdf, FormatBasis::Extension)
        );
        assert_eq!(c.retrieval, RetrievalStatus::NotDownloaded);
        assert!(r.error.unwrap().contains("503"));
    }

    #[test]
    fn duplicates_are_merged() {
        let p = vec![
            arc(Scripted {
                id: "a",
                online: true,
                result: Ok(vec![
                    raw(
                        "Dell Inspiron 15 3535 schematic",
                        "https://Docs.Example.org/x.pdf#p2",
                    ),
                    raw("same", "https://docs.example.org/x.pdf"),
                ]),
                delay_ms: 0,
            }),
            arc(Scripted {
                id: "b",
                online: true,
                result: Ok(vec![raw("again", "https://docs.example.org/x.pdf/")]),
                delay_ms: 0,
            }),
        ];
        let r = run(&dell(), &p, Some(client()), &Cancel::new(), 1, &|_| {});
        assert_eq!(r.candidates.len(), 1);
        assert_eq!(r.candidates[0].title, "Dell Inspiron 15 3535 schematic");
    }

    #[test]
    fn offline_skips_online_providers_and_keeps_local_ones() {
        let p = vec![
            arc(Scripted {
                id: "web",
                online: true,
                result: Ok(vec![raw("x", "https://a.example.org/x.pdf")]),
                delay_ms: 0,
            }),
            arc(Scripted {
                id: "local",
                online: false,
                result: Ok(vec![RawCandidate {
                    title: "Inspiron 15 3535 schematic".into(),
                    library_file: Some(("a.pdf".into(), 9)),
                    ..Default::default()
                }]),
                delay_ms: 0,
            }),
        ];
        let r = run(&dell(), &p, None, &Cancel::new(), 1, &|_| {});
        assert_eq!(r.providers[0].state, ProviderState::SkippedOffline);
        assert_eq!(r.candidates.len(), 1);
        assert_eq!(r.candidates[0].source.trust, SourceTrust::UserProvided);
        assert!(matches!(
            r.candidates[0].retrieval,
            RetrievalStatus::InLibrary { .. }
        ));
        assert_eq!(r.status, SessionStatus::Completed);
    }

    #[test]
    fn cancellation_stops_a_running_session() {
        let p = vec![arc(Scripted {
            id: "slow",
            online: true,
            result: Ok(vec![]),
            delay_ms: 5_000,
        })];
        let cancel = Cancel::new();
        let c2 = cancel.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            c2.cancel();
        });
        let started = Instant::now();
        let r = run(&dell(), &p, Some(client()), &cancel, 1, &|_| {});
        t.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(r.status, SessionStatus::Cancelled);
        assert_eq!(r.providers[0].state, ProviderState::Cancelled);
    }

    #[test]
    fn stages_are_reported_in_order() {
        let seen = Mutex::new(Vec::new());
        run(&dell(), &[], None, &Cancel::new(), 1, &|s| {
            seen.lock().unwrap().push(s)
        });
        assert_eq!(
            seen.into_inner().unwrap(),
            [
                Stage::PreparingIdentity,
                Stage::GeneratingQueries,
                Stage::SearchingProviders,
                Stage::CollectingCandidates,
                Stage::IdentifyingTypes,
                Stage::VerifyingMatches
            ]
        );
    }

    #[test]
    fn references_and_mismatches_are_never_downloadable() {
        let p = vec![arc(Scripted {
            id: "a",
            online: true,
            result: Ok(vec![
                RawCandidate {
                    title: "Dell Inspiron 15 3535 schematic".into(),
                    page_url: Some("https://archive.org/details/x".into()),
                    ..Default::default()
                },
                raw(
                    "Dell Latitude 5420 schematic",
                    "https://dl.dell.com/5420.pdf",
                ),
            ]),
            delay_ms: 0,
        })];
        let r = run(&dell(), &p, Some(client()), &Cancel::new(), 1, &|_| {});
        for c in &r.candidates {
            assert!(
                matches!(c.retrieval, RetrievalStatus::ReferenceOnly { .. }),
                "{}",
                c.title
            );
        }
        assert_eq!(
            r.candidates.last().unwrap().assessment.class,
            MatchClass::Mismatch,
            "mismatches sort last"
        );
    }

    /// The frontend's documents fixture, produced by the real engine and
    /// matcher so the TypeScript mirror stays in step. Regenerate with
    /// `SENTINEL_UPDATE_FIXTURES=1 cargo test -p sentinel-docs`.
    #[test]
    fn the_frontend_documents_fixture_is_current() {
        use crate::model::{DocumentProgress, DocumentsView, SessionRecord};
        let lib = |title: &str, file: &str| RawCandidate {
            title: title.into(),
            library_file: Some((file.into(), 2048)),
            format: Some((FileFormat::Pdf, FormatBasis::Content)),
            ..Default::default()
        };
        let providers = vec![
            arc(Scripted {
                id: "local_library",
                online: false,
                result: Ok(vec![lib(
                    "Dell Inspiron 15 3535 schematic 0N90HF rev A02",
                    "0N90HF-A02.pdf",
                )]),
                delay_ms: 0,
            }),
            arc(Scripted {
                id: "catalogue",
                online: true,
                result: Ok(vec![
                    raw(
                        "Inspiron 15 3535 Owner's Manual",
                        "https://dl.dell.com/manuals/inspiron-15-3535-owners-manual.pdf",
                    ),
                    raw(
                        "Dell Inspiron 15 3530 schematic",
                        "https://docs.example.org/3530.pdf",
                    ),
                    raw(
                        "Dell Inspiron 15 3535 0N90HF schematic rev A00",
                        "https://docs.example.org/0N90HF-A00.pdf",
                    ),
                    raw("3535 board notes", "https://docs.example.org/notes.pdf"),
                    raw(
                        "Laptop schematic collection",
                        "https://docs.example.org/collection.zip",
                    ),
                    raw(
                        "Lenovo ThinkPad T480 Hardware Maintenance Manual",
                        "https://download.lenovo.com/t480_hmm.pdf",
                    ),
                ]),
                delay_ms: 0,
            }),
            arc(Scripted {
                id: "internet_archive",
                online: true,
                result: Ok(vec![RawCandidate {
                    title: "Inspiron 15 3535 service manual scan".into(),
                    page_url: Some("https://archive.org/details/inspiron-15-3535-sm".into()),
                    publisher: Some("Internet Archive".into()),
                    ..Default::default()
                }]),
                delay_ms: 0,
            }),
            arc(Scripted {
                id: "broken",
                online: true,
                result: Err(ProviderError::Net(NetError::Timeout)),
                delay_ms: 0,
            }),
        ];
        let identity = dell();
        let r = run(
            &identity,
            &providers,
            Some(client()),
            &Cancel::new(),
            1_800_000_000,
            &|_| {},
        );
        let mut candidates = r.candidates;
        for (i, c) in candidates.iter_mut().enumerate() {
            c.id = i as u64 + 1;
        }
        let mut providers_out = r.providers;
        for p in &mut providers_out {
            p.duration_ms = 0;
        }
        let view = DocumentsView {
            progress: DocumentProgress::default(),
            identity: Some(identity.clone()),
            identity_problem: None,
            online: true,
            auto_download: false,
            providers: providers.iter().map(|p| p.info()).collect(),
            candidates,
            sessions: vec![SessionRecord {
                id: 1,
                identity_hash: identity.identity_hash.clone(),
                started_at: 1_800_000_000,
                finished_at: Some(1_800_000_009),
                status: r.status,
                online: true,
                queries: r.queries,
                providers: providers_out,
                candidates: 8,
                error: r.error,
            }],
            last_search_at: Some(1_800_000_009),
            cache_expires_at: Some(1_800_000_009 + 14 * 86_400),
            storage_available: true,
            library_dir: "/home/user/.local/share/sentinel/documents/library".into(),
            download_dir: "/home/user/.local/share/sentinel/documents/downloads".into(),
            notice: Some("8 document(s) found: 1 exact, 2 high-confidence.".into()),
        };
        let json = serde_json::to_string_pretty(&view).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/src/test-fixtures/documents-view.json");
        if std::env::var_os("SENTINEL_UPDATE_FIXTURES").is_some() {
            std::fs::write(&path, &json).unwrap();
        }
        assert!(
            std::fs::read_to_string(&path).unwrap_or_default() == json,
            "{} is out of date; regenerate with SENTINEL_UPDATE_FIXTURES=1 cargo test -p sentinel-docs",
            path.display()
        );
    }
}
