//! Coordinates document discovery and downloads (Phase 6).
//!
//! Like scans, a job runs on one worker thread that exists only while there
//! is work. Nothing runs by itself: searches and downloads happen only when
//! a client asks. With online discovery off (the default) no network request
//! is ever made; only the user's document library is read.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use sentinel_config::DocumentSettings;
use sentinel_docs::engine;
use sentinel_docs::net::{Cancel, Client, HttpsTransport, Limits, Transport};
use sentinel_docs::provider::Provider;
use sentinel_docs::providers::{
    archive::InternetArchive, catalogue::CatalogueProvider, local::LocalLibrary,
};
use sentinel_docs::{
    AccessStatus, DocumentCandidate, DocumentProgress, DocumentsView, JobKind, MatchClass,
    ProviderInfo, RetrievalStatus, SearchIdentity, SessionRecord, SessionStatus, SourceTrust,
    Stage,
};
use sentinel_inventory::SystemInventory;
use sentinel_ipc::{ErrorCode, IpcError};
use sentinel_storage::Storage;

use crate::context::unix_now;

const SESSIONS_SHOWN: u32 = 10;
const AUTO_DOWNLOADS: usize = 3;

/// Builds the HTTPS transport; replaced by a fake in tests.
pub type TransportFactory = Arc<dyn Fn(Duration) -> Arc<dyn Transport> + Send + Sync>;

#[derive(Clone)]
pub struct DocumentsConfig {
    /// `None` when the database is unavailable: discovery still works but
    /// nothing is remembered.
    pub database: Option<PathBuf>,
    /// The user's library (read only).
    pub library_dir: PathBuf,
    /// Where downloads are stored.
    pub download_dir: PathBuf,
    pub transport: TransportFactory,
}

#[derive(Default)]
struct State {
    progress: DocumentProgress,
    cancel: Option<Cancel>,
    notice: Option<String>,
    shutting_down: bool,
    /// Results of the last session when there is no database.
    memory: Vec<DocumentCandidate>,
}

pub struct Documents {
    config: Option<DocumentsConfig>,
    state: Arc<Mutex<State>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

fn lock(s: &Mutex<State>) -> MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

fn busy() -> IpcError {
    IpcError::new(
        ErrorCode::Busy,
        "a document search or download is already running",
    )
}

fn not_found() -> IpcError {
    IpcError::new(ErrorCode::NotFound, "no such document")
}

fn refused(message: impl Into<String>) -> IpcError {
    IpcError::new(ErrorCode::InvalidRequest, message)
}

pub fn https_transport() -> TransportFactory {
    Arc::new(|timeout| Arc::new(HttpsTransport::new(timeout)) as Arc<dyn Transport>)
}

impl Documents {
    /// No document support (contexts built in tests that do not need it).
    pub fn disabled() -> Self {
        Self {
            config: None,
            state: Arc::new(Mutex::new(State::default())),
            worker: Mutex::new(None),
        }
    }

    pub fn new(config: DocumentsConfig) -> Self {
        let docs = Self {
            config: Some(config),
            state: Arc::new(Mutex::new(State::default())),
            worker: Mutex::new(None),
        };
        docs.recover_interrupted();
        docs
    }

    fn config(&self) -> Result<&DocumentsConfig, IpcError> {
        self.config.as_ref().ok_or_else(|| {
            IpcError::new(
                ErrorCode::Internal,
                "document discovery is not available in this daemon",
            )
        })
    }

    fn storage(&self) -> Option<Storage> {
        let path = self.config.as_ref()?.database.as_ref()?;
        Storage::open(path)
            .map_err(|e| tracing::warn!(error = %e, "document database unavailable"))
            .ok()
    }

    /// Sessions a previous daemon left running are recorded as interrupted.
    fn recover_interrupted(&self) {
        let Some(mut storage) = self.storage() else {
            return;
        };
        let now = unix_now();
        match storage.interrupted_doc_sessions(now) {
            Ok(rows) => {
                for row in rows {
                    if let Ok(mut s) = serde_json::from_str::<SessionRecord>(&row.payload) {
                        s.status = SessionStatus::Interrupted;
                        s.finished_at = Some(now);
                        s.error = Some("Sentinel stopped while this search was running.".into());
                        if let Ok(p) = serde_json::to_string(&s) {
                            let _ = storage.finish_doc_session(row.id, now, "interrupted", &p);
                        }
                    }
                    tracing::info!(
                        session = row.id,
                        "an interrupted document search was recorded"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not check for interrupted document searches")
            }
        }
    }

    fn providers(config: &DocumentsConfig, settings: &DocumentSettings) -> Vec<Arc<dyn Provider>> {
        let mut out: Vec<Arc<dyn Provider>> = vec![Arc::new(LocalLibrary {
            dir: config.library_dir.clone(),
        })];
        out.push(Arc::new(CatalogueProvider {
            urls: settings.catalogue_urls.clone(),
        }));
        if settings.internet_archive {
            out.push(Arc::new(InternetArchive));
        }
        out
    }

    fn client(config: &DocumentsConfig, settings: &DocumentSettings) -> Option<Arc<Client>> {
        settings.online_discovery.then(|| {
            let timeout = Duration::from_secs(settings.request_timeout_s.into());
            Arc::new(Client::new(
                (config.transport)(timeout),
                Limits {
                    timeout,
                    min_interval: Duration::from_millis(settings.min_request_interval_ms.into()),
                },
            ))
        })
    }

    /// The documents page: identity, progress, results and history.
    pub fn view(
        &self,
        inventory: Option<&SystemInventory>,
        settings: &DocumentSettings,
    ) -> DocumentsView {
        let config = self.config.as_ref();
        let (identity, problem) = identity_of(inventory);
        // Progress first, results second: a job saves its results before it
        // reports itself finished, so a view that says "not running" always
        // contains that job's results. (Reading in the other order could
        // return "finished" with the previous, empty result list.)
        let (progress, notice, memory) = {
            let state = lock(&self.state);
            (
                state.progress.clone(),
                state.notice.clone(),
                state.memory.clone(),
            )
        };
        let storage = self.storage();
        let (mut candidates, sessions, last) = match (&identity, &storage) {
            (Some(id), Some(s)) => (
                s.doc_candidates(&id.identity_hash)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|r| decode(&r.payload, r.id))
                    .collect::<Vec<_>>(),
                s.recent_doc_sessions(&id.identity_hash, SESSIONS_SHOWN)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|r| {
                        serde_json::from_str::<SessionRecord>(&r.payload)
                            .ok()
                            .map(|mut x| {
                                x.id = r.id;
                                x
                            })
                    })
                    .collect(),
                s.doc_last_search(&id.identity_hash).ok().flatten(),
            ),
            _ => (Vec::new(), Vec::new(), None),
        };
        if storage.is_none() {
            candidates = memory;
        }
        sort(&mut candidates);
        let providers = config
            .map(|c| {
                Self::providers(c, settings)
                    .iter()
                    .map(|p| {
                        let mut i: ProviderInfo = p.info();
                        i.enabled = !i.online || settings.online_discovery;
                        i
                    })
                    .collect()
            })
            .unwrap_or_default();
        DocumentsView {
            progress,
            identity,
            identity_problem: problem,
            online: settings.online_discovery,
            auto_download: settings.auto_download,
            providers,
            candidates,
            sessions,
            last_search_at: last,
            cache_expires_at: last.map(|t| t + u64::from(settings.cache_days) * 86_400),
            storage_available: storage.is_some(),
            library_dir: config
                .map(|c| c.library_dir.display().to_string())
                .unwrap_or_default(),
            download_dir: config
                .map(|c| c.download_dir.display().to_string())
                .unwrap_or_default(),
            notice,
        }
    }

    fn begin(&self, kind: JobKind, document_id: Option<u64>) -> Result<Cancel, IpcError> {
        let mut state = lock(&self.state);
        if state.shutting_down {
            return Err(IpcError::new(
                ErrorCode::ShuttingDown,
                "the daemon is shutting down",
            ));
        }
        if state.progress.running {
            return Err(busy());
        }
        let cancel = Cancel::new();
        state.cancel = Some(cancel.clone());
        state.notice = None;
        state.progress = DocumentProgress {
            running: true,
            kind: Some(kind),
            stage: None,
            started_at: Some(unix_now()),
            session_id: None,
            document_id,
            cancel_requested: false,
        };
        Ok(cancel)
    }

    fn spawn(&self, job: impl FnOnce() + Send + 'static) -> Result<(), IpcError> {
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = worker.take() {
            let _ = old.join();
        }
        let state = self.state.clone();
        match std::thread::Builder::new()
            .name("sentinel-documents".into())
            .spawn(job)
        {
            Ok(h) => {
                *worker = Some(h);
                Ok(())
            }
            Err(e) => {
                let mut s = lock(&state);
                s.progress = DocumentProgress {
                    running: false,
                    ..s.progress.clone()
                };
                Err(IpcError::new(
                    ErrorCode::Internal,
                    format!("the document worker could not start: {e}"),
                ))
            }
        }
    }

    /// Start a search unless recent results exist (or `force`).
    pub fn start(
        &self,
        inventory: Option<&SystemInventory>,
        settings: &DocumentSettings,
        force: bool,
    ) -> Result<(), IpcError> {
        let config = self.config()?.clone();
        let (identity, problem) = identity_of(inventory);
        let identity = identity.ok_or_else(|| refused(problem.unwrap_or_default()))?;
        if !force
            && let Some(last) = self
                .storage()
                .and_then(|s| s.doc_last_search(&identity.identity_hash).ok().flatten())
            && unix_now() < last + u64::from(settings.cache_days) * 86_400
        {
            lock(&self.state).notice = Some("Results of the last search are still current, so no new search was made. Use Refresh to search again.".into());
            return Ok(());
        }
        let cancel = self.begin(JobKind::Search, None)?;
        let settings = settings.clone();
        let state = self.state.clone();
        self.spawn(move || search_job(&config, &settings, &identity, &cancel, &state))
    }

    pub fn cancel(&self) {
        let mut state = lock(&self.state);
        if let Some(c) = &state.cancel
            && state.progress.running
        {
            c.cancel();
            state.progress.cancel_requested = true;
            tracing::info!("document job cancellation requested");
        }
    }

    /// Download one document (user-requested), if its source permits it.
    pub fn download(&self, document_id: u64, settings: &DocumentSettings) -> Result<(), IpcError> {
        let config = self.config()?.clone();
        let storage = self.storage().ok_or_else(|| {
            IpcError::new(
                ErrorCode::Internal,
                "downloads need Sentinel's database, which is unavailable",
            )
        })?;
        let row = storage
            .doc_candidate(document_id)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
            .ok_or_else(not_found)?;
        let candidate = decode(&row.payload, row.id).ok_or_else(not_found)?;
        check_downloadable(&candidate, settings)?;
        drop(storage);
        let cancel = self.begin(JobKind::Download, Some(document_id))?;
        let settings = settings.clone();
        let state = self.state.clone();
        self.spawn(move || {
            let notice = download_one(&config, &settings, candidate, &cancel);
            let mut s = lock(&state);
            s.notice = Some(notice);
            s.progress = DocumentProgress::default();
            s.cancel = None;
        })
    }

    /// Delete a downloaded copy (only Sentinel's own stored file).
    pub fn remove_file(&self, document_id: u64) -> Result<(), IpcError> {
        let config = self.config()?;
        if lock(&self.state).progress.running {
            return Err(busy());
        }
        let mut storage = self
            .storage()
            .ok_or_else(|| IpcError::new(ErrorCode::Internal, "the database is unavailable"))?;
        let row = storage
            .doc_candidate(document_id)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
            .ok_or_else(not_found)?;
        let mut candidate = decode(&row.payload, row.id).ok_or_else(not_found)?;
        let RetrievalStatus::Downloaded { file, .. } = &candidate.retrieval else {
            return Err(refused("this document has no downloaded copy"));
        };
        let file = file.clone();
        candidate.retrieval = RetrievalStatus::NotDownloaded;
        let payload = serde_json::to_string(&candidate)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        storage
            .set_doc_candidate_file(document_id, None, None, &payload)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        // Another candidate may point at the same (de-duplicated) file.
        if storage.doc_file_references(&file).unwrap_or(1) == 0 {
            remove_stored(&config.download_dir, &file);
        }
        lock(&self.state).notice = Some("The downloaded copy was removed.".into());
        Ok(())
    }

    pub fn clear_cache(&self, inventory: Option<&SystemInventory>) -> Result<(), IpcError> {
        if lock(&self.state).progress.running {
            return Err(busy());
        }
        let (identity, problem) = identity_of(inventory);
        let identity = identity.ok_or_else(|| refused(problem.unwrap_or_default()))?;
        let mut state = lock(&self.state);
        state.memory.clear();
        if let Some(mut s) = self.storage() {
            let n = s
                .clear_doc_cache(&identity.identity_hash)
                .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
            state.notice = Some(format!(
                "{n} cached result(s) forgotten. Downloaded copies were kept."
            ));
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        {
            let mut state = lock(&self.state);
            state.shutting_down = true;
            if let Some(c) = &state.cancel {
                c.cancel();
            }
        }
        if let Some(h) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = h.join();
        }
    }
}

fn identity_of(inventory: Option<&SystemInventory>) -> (Option<SearchIdentity>, Option<String>) {
    match inventory {
        None => (
            None,
            Some("Run a hardware scan first: discovery needs the motherboard identity.".into()),
        ),
        Some(inv) => match SearchIdentity::from_inventory(inv) {
            Ok(id) => (Some(id), None),
            Err(why) => (None, Some(why)),
        },
    }
}

fn decode(payload: &str, id: u64) -> Option<DocumentCandidate> {
    let mut c: DocumentCandidate = serde_json::from_str(payload).ok()?;
    c.id = id;
    Some(c)
}

fn sort(c: &mut [DocumentCandidate]) {
    c.sort_by(|a, b| {
        (a.assessment.class, a.doc_type, a.title.clone()).cmp(&(
            b.assessment.class,
            b.doc_type,
            b.title.clone(),
        ))
    });
}

fn check_downloadable(c: &DocumentCandidate, settings: &DocumentSettings) -> Result<(), IpcError> {
    if !settings.online_discovery {
        return Err(refused(
            "online discovery is off; turn it on in Settings to download",
        ));
    }
    match &c.source.access {
        AccessStatus::Downloadable => {}
        AccessStatus::ReferenceOnly { reason } => {
            return Err(refused(format!(
                "this document is kept as a reference only: {reason}"
            )));
        }
        AccessStatus::LocalFile => return Err(refused("this document is already in your library")),
    }
    if matches!(c.retrieval, RetrievalStatus::Downloaded { .. }) {
        return Err(refused("this document is already downloaded"));
    }
    if c.source.document_url.is_none() {
        return Err(refused("no document address is known"));
    }
    Ok(())
}

fn remove_stored(dir: &Path, file: &str) {
    if let Err(e) = sentinel_docs::download::remove_stored(dir, file) {
        tracing::warn!(error = %e, "a downloaded document was not deleted");
    }
}

fn set_stage(state: &Mutex<State>, stage: Stage) {
    lock(state).progress.stage = Some(stage);
}

fn search_job(
    config: &DocumentsConfig,
    settings: &DocumentSettings,
    identity: &SearchIdentity,
    cancel: &Cancel,
    state: &Mutex<State>,
) {
    let started = unix_now();
    let mut storage = config.database.as_ref().and_then(|p| Storage::open(p).ok());
    let online = settings.online_discovery;
    let mut record = SessionRecord {
        id: 0,
        identity_hash: identity.identity_hash.clone(),
        started_at: started,
        finished_at: None,
        status: SessionStatus::Running,
        online,
        queries: vec![],
        providers: vec![],
        candidates: 0,
        error: None,
    };
    if let Some(s) = storage.as_mut() {
        let _ = s.save_doc_identity(
            &identity.identity_hash,
            &serde_json::to_string(identity).unwrap_or_default(),
            started,
        );
        match s.begin_doc_session(
            &identity.identity_hash,
            started,
            &serde_json::to_string(&record).unwrap_or_default(),
        ) {
            Ok(id) => {
                record.id = id;
                lock(state).progress.session_id = Some(id);
            }
            Err(e) => tracing::warn!(error = %e, "could not record the document search"),
        }
    }
    tracing::info!(online, "document search started");
    let providers = Documents::providers(config, settings);
    let client = Documents::client(config, settings);
    let result = engine::run(
        identity,
        &providers,
        client.clone(),
        cancel,
        started,
        &|stage| set_stage(state, stage),
    );

    set_stage(state, Stage::SavingResults);
    let finished = unix_now();
    let mut kept = Vec::new();
    if result.status != SessionStatus::Cancelled {
        for mut c in result.candidates {
            if let Some(s) = storage.as_mut() {
                // Keep the first discovery time and an existing download.
                if let Ok(Some(first)) =
                    s.doc_candidate_discovered_at(&identity.identity_hash, &c.key)
                {
                    c.discovered_at = first;
                }
                if let Ok(rows) = s.doc_candidates(&identity.identity_hash)
                    && let Some(old) = rows.iter().find(|r| {
                        r.local_file.is_some()
                            && decode(&r.payload, r.id).is_some_and(|o| o.key == c.key)
                    })
                    && let Some(prev) = decode(&old.payload, old.id)
                {
                    c.retrieval = prev.retrieval;
                }
                c.verified_at = finished;
                match serde_json::to_string(&c)
                    .map_err(|e| e.to_string())
                    .and_then(|p| {
                        s.upsert_doc_candidate(
                            &identity.identity_hash,
                            &c.key,
                            &p,
                            c.discovered_at,
                            finished,
                        )
                        .map_err(|e| e.to_string())
                    }) {
                    Ok(id) => c.id = id,
                    Err(e) => tracing::warn!(error = %e, "a document result could not be saved"),
                }
            }
            kept.push(c);
        }
    }
    record.status = result.status;
    record.finished_at = Some(finished);
    record.queries = result.queries;
    record.providers = result.providers;
    record.candidates = kept.len() as u32;
    record.error = result.error;
    if let Some(s) = storage.as_mut() {
        let status = serde_json::to_value(record.status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "failed".into());
        let _ = s.finish_doc_session(
            record.id,
            finished,
            &status,
            &serde_json::to_string(&record).unwrap_or_default(),
        );
        if matches!(
            record.status,
            SessionStatus::Completed | SessionStatus::Partial
        ) {
            let _ = s.set_doc_last_search(&identity.identity_hash, Some(finished));
        }
    }
    let exact = kept
        .iter()
        .filter(|c| c.assessment.class == MatchClass::Exact)
        .count();
    let high = kept
        .iter()
        .filter(|c| c.assessment.class == MatchClass::HighConfidence)
        .count();
    let mut notice = match record.status {
        SessionStatus::Cancelled => {
            "The search was cancelled; previous results were kept.".to_string()
        }
        SessionStatus::Failed => format!(
            "The search failed: {}",
            record.error.clone().unwrap_or_default()
        ),
        _ => format!(
            "{} document(s) found: {exact} exact, {high} high-confidence. {}",
            kept.len(),
            if online {
                ""
            } else {
                "Online discovery is off, so only your document library was searched."
            }
        ),
    };

    // Opt-in automatic download: manufacturer's own site, exact or high
    // confidence matches, at most a few per search.
    if settings.auto_download
        && online
        && matches!(
            record.status,
            SessionStatus::Completed | SessionStatus::Partial
        )
        && !cancel.is_cancelled()
    {
        let picks: Vec<_> = kept
            .iter()
            .filter(|c| {
                matches!(
                    c.assessment.class,
                    MatchClass::Exact | MatchClass::HighConfidence
                ) && c.source.trust == SourceTrust::OfficialThisManufacturer
                    && check_downloadable(c, settings).is_ok()
            })
            .take(AUTO_DOWNLOADS)
            .cloned()
            .collect();
        for c in picks {
            let n = download_one(config, settings, c, cancel);
            notice.push(' ');
            notice.push_str(&n);
        }
    }
    let mut s = lock(state);
    if storage.is_none() && record.status != SessionStatus::Cancelled {
        s.memory = kept;
    }
    s.notice = Some(notice.trim().to_string());
    s.progress = DocumentProgress::default();
    s.cancel = None;
    tracing::info!(status = ?record.status, candidates = record.candidates, "document search finished");
}

fn download_one(
    config: &DocumentsConfig,
    settings: &DocumentSettings,
    mut c: DocumentCandidate,
    cancel: &Cancel,
) -> String {
    let Some(url) = c.source.document_url.clone() else {
        return "No document address is known.".into();
    };
    let timeout = Duration::from_secs(settings.request_timeout_s.into());
    let client = Client::new(
        (config.transport)(timeout),
        Limits {
            timeout,
            min_interval: Duration::from_millis(settings.min_request_interval_ms.into()),
        },
    );
    let max = u64::from(settings.max_download_mb) * 1024 * 1024;
    let now = unix_now();
    let (notice, file) = match sentinel_docs::download::download(
        &client,
        &url,
        &config.download_dir,
        max,
        c.extension.as_deref(),
        cancel,
    ) {
        Ok(d) => {
            c.format = d.format;
            c.format_basis = sentinel_docs::FormatBasis::Content;
            c.size_bytes = Some(d.size_bytes);
            if let Some(z) = &d.zip {
                c.metadata
                    .insert("archive_entries".into(), z.entries.len().to_string());
                c.metadata.insert(
                    "archive_note".into(),
                    "Not extracted. Sentinel never extracts archives.".into(),
                );
            }
            c.retrieval = RetrievalStatus::Downloaded {
                file: d.file.clone(),
                sha256: d.sha256.clone(),
                size_bytes: d.size_bytes,
                at: now,
            };
            (
                format!("Downloaded “{}”.", c.title),
                Some((d.file, d.sha256)),
            )
        }
        Err(e) => {
            c.retrieval = RetrievalStatus::Failed {
                reason: e.to_string(),
                at: now,
            };
            (format!("“{}” was not downloaded: {e}", c.title), None)
        }
    };
    if let Some(mut s) = config.database.as_ref().and_then(|p| Storage::open(p).ok())
        && let Ok(payload) = serde_json::to_string(&c)
    {
        let (f, h) = file
            .as_ref()
            .map(|(f, h)| (Some(f.as_str()), Some(h.as_str())))
            .unwrap_or((None, None));
        let _ = s.set_doc_candidate_file(c.id, f, h, &payload);
    }
    tracing::info!(
        document = c.id,
        ok = file.is_some(),
        "document download finished"
    );
    notice
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_docs::net::{Head, NetError, copy_bounded};
    use sentinel_inventory::{BoardIdentity, Confidence, Field};
    use std::collections::HashMap;
    use std::io::Write;
    use std::time::Instant;

    /// Answers by URL; can be slowed down to test cancellation.
    #[derive(Default)]
    struct FakeWeb {
        pages: Mutex<HashMap<String, (u16, Vec<u8>)>>,
        delay: Mutex<Duration>,
        requests: Mutex<Vec<String>>,
    }

    impl Transport for FakeWeb {
        fn get(
            &self,
            url: &url::Url,
            _a: &str,
            max: u64,
            sink: &mut dyn Write,
            cancel: &Cancel,
        ) -> Result<Head, NetError> {
            self.requests.lock().unwrap().push(url.to_string());
            let end = Instant::now() + *self.delay.lock().unwrap();
            while Instant::now() < end {
                if cancel.is_cancelled() {
                    return Err(NetError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let page = self.pages.lock().unwrap().get(url.as_str()).cloned();
            match page {
                Some((status, body)) => {
                    if (200..300).contains(&status) {
                        copy_bounded(&mut body.as_slice(), sink, max, cancel)?;
                    }
                    Ok(Head {
                        status,
                        content_type: None,
                        content_length: Some(body.len() as u64),
                        location: None,
                    })
                }
                None => Ok(Head {
                    status: 404,
                    ..Default::default()
                }),
            }
        }
    }

    const CATALOGUE: &str = "https://index.example.org/docs.json";

    fn web() -> Arc<FakeWeb> {
        let w = Arc::new(FakeWeb::default());
        let cat = r#"{"format":"sentinel-document-catalogue","version":1,"documents":[
            {"title":"Inspiron 15 3535 Owner's Manual","url":"https://dl.dell.com/manuals/3535.pdf","manufacturer":"Dell","models":["Inspiron 15 3535"]},
            {"title":"Inspiron 15 3535 schematic scan","page_url":"https://archive.org/details/i3535"}]}"#;
        w.pages
            .lock()
            .unwrap()
            .insert(CATALOGUE.into(), (200, cat.as_bytes().to_vec()));
        w.pages.lock().unwrap().insert(
            "https://dl.dell.com/manuals/3535.pdf".into(),
            (200, b"%PDF-1.7\nmanual".to_vec()),
        );
        w
    }

    fn inventory() -> SystemInventory {
        let mut inv: SystemInventory = serde_json::from_str(include_str!(
            "../../sentinel-docs/tests/empty_inventory.json"
        ))
        .unwrap();
        let f = |v: &str| {
            Field::detected(
                v.to_string(),
                "/sys/class/dmi/id/x",
                Confidence::FirmwareReported,
            )
        };
        let (bm, bn, sm) = (f("Dell Inc."), f("0N90HF"), f("Inspiron 15 3535"));
        inv.motherboard = Some(BoardIdentity {
            discovered_at: 1,
            identification: BoardIdentity::assess(&bm, &bn, &sm),
            system_manufacturer: f("Dell Inc."),
            system_model: sm,
            system_family: f("Inspiron"),
            system_sku: f("0C3E"),
            board_manufacturer: bm,
            board_name: bn,
            board_revision: f("A02"),
            bios_vendor: f("Dell Inc."),
            bios_version: f("1.32.0"),
            bios_release_date: f("09/02/2026"),
        });
        inv
    }

    struct Fixture {
        dir: tempfile::TempDir,
        web: Arc<FakeWeb>,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("library")).unwrap();
            Self { dir, web: web() }
        }
        fn docs(&self) -> Documents {
            let web = self.web.clone();
            Documents::new(DocumentsConfig {
                database: Some(self.dir.path().join("sentinel.db")),
                library_dir: self.dir.path().join("library"),
                download_dir: self.dir.path().join("downloads"),
                transport: Arc::new(move |_| web.clone() as Arc<dyn Transport>),
            })
        }
    }

    fn online() -> DocumentSettings {
        DocumentSettings {
            online_discovery: true,
            internet_archive: false,
            catalogue_urls: vec![CATALOGUE.into()],
            min_request_interval_ms: 250,
            ..Default::default()
        }
    }

    fn wait(d: &Documents) {
        let end = Instant::now() + Duration::from_secs(10);
        while lock(&d.state).progress.running {
            assert!(Instant::now() < end, "job did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn offline_searches_only_the_library_and_reuses_the_cache() {
        let fx = Fixture::new();
        std::fs::write(
            fx.dir.path().join("library/manual.pdf"),
            b"%PDF-1.4 << /Title (Dell Inspiron 15 3535 Service Manual) >>",
        )
        .unwrap();
        let d = fx.docs();
        let inv = inventory();
        let settings = DocumentSettings::default();
        d.start(Some(&inv), &settings, false).unwrap();
        wait(&d);
        let v = d.view(Some(&inv), &settings);
        assert!(!v.online);
        assert_eq!(v.candidates.len(), 1);
        let c = &v.candidates[0];
        assert_eq!(c.assessment.class, MatchClass::HighConfidence);
        assert!(matches!(c.retrieval, RetrievalStatus::InLibrary { .. }));
        assert_eq!(v.sessions.len(), 1);
        assert_eq!(v.sessions[0].status, SessionStatus::Completed);
        assert!(v.providers.iter().filter(|p| p.online).all(|p| !p.enabled));
        assert!(
            fx.web.requests.lock().unwrap().is_empty(),
            "offline: no request at all"
        );
        // Unchanged identity, fresh results: no new search.
        d.start(Some(&inv), &settings, false).unwrap();
        assert!(!lock(&d.state).progress.running);
        assert!(
            d.view(Some(&inv), &settings)
                .notice
                .unwrap()
                .contains("still current")
        );
        d.start(Some(&inv), &settings, true).unwrap();
        wait(&d);
        assert_eq!(d.view(Some(&inv), &settings).sessions.len(), 2);
        // Results survive a restart.
        drop(d);
        let again = fx.docs();
        assert_eq!(again.view(Some(&inv), &settings).candidates.len(), 1);
    }

    #[test]
    fn online_catalogue_download_and_removal() {
        let fx = Fixture::new();
        let d = fx.docs();
        let inv = inventory();
        let s = online();
        d.start(Some(&inv), &s, false).unwrap();
        wait(&d);
        let v = d.view(Some(&inv), &s);
        assert_eq!(v.candidates.len(), 2);
        let manual = v
            .candidates
            .iter()
            .find(|c| c.source.domain.as_deref() == Some("dl.dell.com"))
            .unwrap();
        assert_eq!(manual.source.trust, SourceTrust::OfficialThisManufacturer);
        assert_eq!(manual.assessment.class, MatchClass::HighConfidence);
        let scan = v
            .candidates
            .iter()
            .find(|c| c.source.trust == SourceTrust::Community)
            .unwrap();
        // References are never downloaded; offline nothing is.
        assert_eq!(
            d.download(scan.id, &s).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            d.download(manual.id, &DocumentSettings::default())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(d.download(999, &s).unwrap_err().code, ErrorCode::NotFound);
        d.download(manual.id, &s).unwrap();
        wait(&d);
        let v = d.view(Some(&inv), &s);
        let manual = v.candidates.iter().find(|c| c.id == manual.id).unwrap();
        let RetrievalStatus::Downloaded { file, .. } = &manual.retrieval else {
            panic!("{:?} {:?}", manual.retrieval, v.notice)
        };
        assert!(fx.dir.path().join("downloads").join(file).is_file());
        // A new search keeps the download.
        d.start(Some(&inv), &s, true).unwrap();
        wait(&d);
        let kept = d
            .view(Some(&inv), &s)
            .candidates
            .into_iter()
            .find(|c| c.key == manual.key)
            .unwrap();
        assert!(matches!(kept.retrieval, RetrievalStatus::Downloaded { .. }));
        // Clearing the cache keeps downloaded documents; removing deletes the copy.
        d.clear_cache(Some(&inv)).unwrap();
        assert_eq!(d.view(Some(&inv), &s).candidates.len(), 1);
        d.remove_file(manual.id).unwrap();
        assert!(!fx.dir.path().join("downloads").join(file).exists());
        assert_eq!(
            d.remove_file(manual.id).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn cancellation_busy_and_missing_identity() {
        let fx = Fixture::new();
        *fx.web.delay.lock().unwrap() = Duration::from_secs(5);
        let d = fx.docs();
        let inv = inventory();
        let s = online();
        assert_eq!(
            d.start(None, &s, false).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        d.start(Some(&inv), &s, false).unwrap();
        assert_eq!(
            d.start(Some(&inv), &s, true).unwrap_err().code,
            ErrorCode::Busy
        );
        let t0 = Instant::now();
        d.cancel();
        wait(&d);
        assert!(t0.elapsed() < Duration::from_secs(3));
        let v = d.view(Some(&inv), &s);
        assert_eq!(v.sessions[0].status, SessionStatus::Cancelled);
        assert!(v.notice.unwrap().contains("cancelled"));
        assert!(
            v.last_search_at.is_none(),
            "a cancelled search does not count as a cached result"
        );
    }

    #[test]
    fn interrupted_sessions_are_recorded_after_a_restart() {
        let fx = Fixture::new();
        {
            let mut st = Storage::open(&fx.dir.path().join("sentinel.db")).unwrap();
            let rec = SessionRecord {
                id: 0,
                identity_hash: "h".into(),
                started_at: 1,
                finished_at: None,
                status: SessionStatus::Running,
                online: true,
                queries: vec![],
                providers: vec![],
                candidates: 0,
                error: None,
            };
            st.begin_doc_session("h", 1, &serde_json::to_string(&rec).unwrap())
                .unwrap();
        }
        let _d = fx.docs();
        let st = Storage::open(&fx.dir.path().join("sentinel.db")).unwrap();
        let rows = st.recent_doc_sessions("h", 5).unwrap();
        let rec: SessionRecord = serde_json::from_str(&rows[0].payload).unwrap();
        assert_eq!(rec.status, SessionStatus::Interrupted);
        assert!(rec.error.unwrap().contains("stopped"));
    }

    #[test]
    fn a_finished_view_always_contains_the_results() {
        let fx = Fixture::new();
        std::fs::write(
            fx.dir.path().join("library/manual.pdf"),
            b"%PDF-1.4 << /Title (Dell Inspiron 15 3535 Service Manual) >>",
        )
        .unwrap();
        let d = fx.docs();
        let inv = inventory();
        let settings = DocumentSettings::default();
        for _ in 0..100 {
            d.clear_cache(Some(&inv)).unwrap();
            d.start(Some(&inv), &settings, true).unwrap();
            loop {
                let v = d.view(Some(&inv), &settings);
                if !v.progress.running {
                    assert_eq!(v.candidates.len(), 1, "a finished view lost the results");
                    break;
                }
            }
            wait(&d);
        }
    }
}
