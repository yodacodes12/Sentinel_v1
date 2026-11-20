//! The single backend interface used by every Sentinel front end.
//!
//! The CLI and the desktop shell both call [`Sentinel`]; neither talks to the
//! socket, the configuration file or the daemon binary on its own. That is
//! what keeps their behaviour identical.
//!
//! Process policy: this crate launches Sentinel's own binaries and, when the
//! user explicitly asks to open an installed utility, the desktop
//! environment's launcher (`gio launch <desktop file>`). It stops the daemon
//! by asking it over IPC. It never sends signals and never runs a command
//! line taken from a desktop file, a package or the daemon.

pub mod launcher;

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use sentinel_config::{ConfigPatch, ConfigStore, LoadStatus, PowerMode, SentinelConfig};
use sentinel_core::instance::is_locked;
use sentinel_core::{APP_VERSION, AppPaths, MonitoringCommand, Result, SentinelError};
use sentinel_inventory::cache::{self, Cacheable};
use sentinel_inventory::{
    ComponentIndex, DiscoveryJob, DiscoveryStatus, HardwareCapabilities, InventoryOrigin,
    JobProgress, SystemInventory, SystemUtility, UtilityCatalog,
};
use sentinel_ipc::{Client, DaemonStatus, MetricsAnswer, MonitoringStatus, ProcessAnswer, Request};
use sentinel_storage::Storage;
use sentinel_telemetry::model::{
    Alert, HistoryQuery, MonitoringReport, SamplingInfo, SamplingProfile, Series,
};
use serde::{Deserialize, Serialize};

/// How long `start` waits for the daemon to answer, and `stop` for it to exit.
pub const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

pub struct Sentinel {
    paths: AppPaths,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StartOutcome {
    Started(DaemonStatus),
    AlreadyRunning(DaemonStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    Stopped,
    NotRunning,
}

/// Where a configuration value was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    /// The running daemon's live configuration.
    Daemon,
    /// The file on disk, because no daemon is running.
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConfigView {
    pub config: SentinelConfig,
    pub path: String,
    pub source: ConfigSource,
    /// Set when the file on disk could not be used as-is.
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcState {
    Connected,
    NotRunning,
    Error,
}

/// Everything `sentinel status` and the desktop header show, gathered once.
/// Every field is a real observation; nothing is estimated or assumed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Overview {
    /// Version of the client that produced this overview.
    pub client_version: String,
    pub running: bool,
    pub ipc: IpcState,
    /// Human-readable reason when `ipc` is `error`.
    pub ipc_detail: Option<String>,
    pub socket_path: String,
    /// Present only when the daemon answered.
    pub daemon: Option<DaemonStatus>,
    pub operating_mode: PowerMode,
    pub config_path: String,
    pub log_path: String,
}

impl Sentinel {
    pub fn from_env() -> Result<Self> {
        sentinel_core::platform::ensure_supported_platform()?;
        Ok(Self::new(AppPaths::from_env()?))
    }

    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn connect(&self) -> Result<Client> {
        Client::connect(&self.paths.checked_socket_path()?)
    }

    /// The daemon's status, or [`SentinelError::DaemonUnavailable`].
    pub fn status(&self) -> Result<DaemonStatus> {
        self.connect()?.status()
    }

    /// `Ok(None)` when no daemon is running; other failures stay errors.
    pub fn probe(&self) -> Result<Option<DaemonStatus>> {
        match self.status() {
            Ok(status) => Ok(Some(status)),
            Err(SentinelError::DaemonUnavailable) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// A complete, never-failing snapshot for display.
    pub fn overview(&self) -> Overview {
        let (ipc, ipc_detail, daemon) = match self.status() {
            Ok(status) => (IpcState::Connected, None, Some(status)),
            Err(SentinelError::DaemonUnavailable) => (IpcState::NotRunning, None, None),
            Err(error) => (IpcState::Error, Some(error.to_string()), None),
        };
        let operating_mode = match &daemon {
            Some(status) => status.operating_mode,
            None => ConfigStore::read(&self.paths.config_file())
                .map(|store| store.config().power_mode)
                .unwrap_or_default(),
        };
        Overview {
            client_version: APP_VERSION.to_string(),
            running: daemon.is_some(),
            ipc,
            ipc_detail,
            socket_path: self.paths.socket_path().display().to_string(),
            daemon,
            operating_mode,
            config_path: self.paths.config_file().display().to_string(),
            log_path: self.paths.daemon_log_file().display().to_string(),
        }
    }

    /// Start the daemon unless one is already running.
    pub fn start(&self) -> Result<StartOutcome> {
        sentinel_core::platform::ensure_not_root(sentinel_core::platform::current_uid())?;
        if let Some(status) = self.probe()? {
            return Ok(StartOutcome::AlreadyRunning(status));
        }

        let binary = launcher::locate(launcher::DAEMON_BINARY, launcher::DAEMON_ENV)?;
        let mut child = launcher::spawn_detached(&binary, &[])?;

        let deadline = Instant::now() + LIFECYCLE_TIMEOUT;
        loop {
            if let Some(status) = self.probe()? {
                // Only claim the start if the daemon answering is the one
                // spawned here; a concurrent start may have won the race.
                let ours = status.pid == child.id();
                launcher::reap_in_background(child);
                return Ok(if ours {
                    StartOutcome::Started(status)
                } else {
                    StartOutcome::AlreadyRunning(status)
                });
            }
            if let Some(exit) = child.try_wait().map_err(|e| SentinelError::Launch {
                what: "the Sentinel daemon".into(),
                reason: e.to_string(),
            })? {
                // Another start won the race: that daemon is the one running.
                if exit.code() == Some(i32::from(SentinelError::AlreadyRunning.exit_code()))
                    && let Some(status) = self.wait_for_daemon(deadline)?
                {
                    return Ok(StartOutcome::AlreadyRunning(status));
                }
                return Err(SentinelError::Launch {
                    what: "the Sentinel daemon".into(),
                    reason: format!(
                        "it exited during start-up ({exit}). See {} for details",
                        self.paths.daemon_log_file().display()
                    ),
                });
            }
            if Instant::now() >= deadline {
                launcher::reap_in_background(child);
                return Err(SentinelError::Timeout(
                    "waiting for the Sentinel daemon to become ready".into(),
                ));
            }
            sleep(POLL_INTERVAL);
        }
    }

    fn wait_for_daemon(&self, deadline: Instant) -> Result<Option<DaemonStatus>> {
        loop {
            if let Some(status) = self.probe()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            sleep(POLL_INTERVAL);
        }
    }

    /// Ask the daemon to shut down and wait until it has really gone.
    pub fn stop(&self) -> Result<StopOutcome> {
        let lock_file = self.paths.lock_file();
        let mut client = match self.connect() {
            Ok(client) => client,
            Err(SentinelError::DaemonUnavailable) if !is_locked(&lock_file) => {
                return Ok(StopOutcome::NotRunning);
            }
            // The lock is held but the socket is gone: a daemon is already
            // on its way out (or in). Wait for it below.
            Err(SentinelError::DaemonUnavailable) => {
                return self.wait_until_gone().map(|()| StopOutcome::Stopped);
            }
            Err(error) => return Err(error),
        };
        client.shutdown()?;
        drop(client);
        self.wait_until_gone()?;
        Ok(StopOutcome::Stopped)
    }

    fn wait_until_gone(&self) -> Result<()> {
        let lock_file = self.paths.lock_file();
        let deadline = Instant::now() + LIFECYCLE_TIMEOUT;
        // The daemon releases the lock last, when the process exits.
        while is_locked(&lock_file) {
            if Instant::now() >= deadline {
                return Err(SentinelError::Timeout(
                    "waiting for the Sentinel daemon to stop".into(),
                ));
            }
            sleep(POLL_INTERVAL);
        }
        Ok(())
    }

    /// The configuration: live from the daemon, or from disk when stopped.
    pub fn configuration(&self) -> Result<ConfigView> {
        match self.connect() {
            Ok(mut client) => {
                let (config, path) = client.configuration()?;
                Ok(ConfigView {
                    config,
                    path,
                    source: ConfigSource::Daemon,
                    warning: None,
                })
            }
            Err(SentinelError::DaemonUnavailable) => {
                Ok(file_view(ConfigStore::read(&self.paths.config_file())?))
            }
            Err(error) => Err(error),
        }
    }

    /// Change settings through the daemon when it runs (so its in-memory
    /// copy stays authoritative), otherwise directly in the file using the
    /// same validation code.
    pub fn update_configuration(&self, patch: ConfigPatch) -> Result<ConfigView> {
        match self.connect() {
            Ok(mut client) => {
                let (config, path) = client.update_configuration(patch)?;
                Ok(ConfigView {
                    config,
                    path,
                    source: ConfigSource::Daemon,
                    warning: None,
                })
            }
            Err(SentinelError::DaemonUnavailable) => {
                sentinel_core::paths::ensure_private_dir(&self.paths.config_dir)?;
                let mut store = ConfigStore::open(&self.paths.config_file())?;
                store.update(&patch)?;
                Ok(file_view(store))
            }
            Err(error) => Err(error),
        }
    }

    /// Send a monitoring command and get the state that now holds. A
    /// refusal (e.g. resume while not paused) is returned as an error.
    pub fn monitoring(&self, command: MonitoringCommand) -> Result<MonitoringStatus> {
        self.connect()?.monitoring(command)
    }
}

/// Where an inventory or catalogue shown to the user came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewSource {
    /// The running daemon (its result may itself be from the cache: see
    /// `origin`).
    Daemon,
    /// Sentinel's cache file, read directly because no daemon is running.
    CacheFile,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InventoryView {
    pub inventory: Option<SystemInventory>,
    /// Navigable index of `inventory` for the hardware explorer, built here
    /// in the client from the inventory alone. Not part of the daemon's IPC
    /// contract.
    pub components: Option<ComponentIndex>,
    pub origin: Option<InventoryOrigin>,
    pub source: ViewSource,
    /// Scan progress; `None` when the daemon is not running.
    pub progress: Option<JobProgress>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UtilitiesView {
    pub catalog: Option<UtilityCatalog>,
    pub origin: Option<InventoryOrigin>,
    pub source: ViewSource,
    pub progress: Option<JobProgress>,
    pub warning: Option<String>,
}

/// Read a cached scan without the daemon. Never creates or changes files.
fn read_cache<T: Cacheable>(paths: &AppPaths) -> (Option<T>, Option<String>) {
    match Storage::open_read_only(&paths.database_file()) {
        Ok(None) => (None, None),
        Ok(Some(storage)) => match storage.load_cache(T::KEY) {
            Ok(Some(row)) => match cache::decode::<T>(row.schema_version, &row.payload) {
                Ok(value) => (Some(value), None),
                Err(problem) => (
                    None,
                    Some(format!(
                        "The cached scan cannot be used ({problem}). Run a new scan."
                    )),
                ),
            },
            Ok(None) => (None, None),
            Err(error) => (None, Some(error.to_string())),
        },
        Err(error) => (None, Some(error.to_string())),
    }
}

impl Sentinel {
    /// The hardware inventory: from the daemon, or from the cache file when
    /// the daemon is stopped (labelled as cached).
    pub fn inventory(&self) -> Result<InventoryView> {
        match self.connect() {
            Ok(mut client) => {
                let answer = client.hardware_inventory()?;
                let inventory = answer.inventory.map(|b| *b);
                Ok(InventoryView {
                    components: inventory.as_ref().map(ComponentIndex::build),
                    inventory,
                    origin: answer.origin,
                    source: ViewSource::Daemon,
                    progress: Some(answer.progress),
                    warning: None,
                })
            }
            Err(SentinelError::DaemonUnavailable) => {
                let (inventory, warning) = read_cache::<SystemInventory>(&self.paths);
                Ok(InventoryView {
                    origin: inventory.as_ref().map(|_| InventoryOrigin::Cache),
                    components: inventory.as_ref().map(ComponentIndex::build),
                    inventory,
                    source: ViewSource::CacheFile,
                    progress: None,
                    warning,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn utilities(&self) -> Result<UtilitiesView> {
        match self.connect() {
            Ok(mut client) => {
                let answer = client.system_utilities()?;
                Ok(UtilitiesView {
                    catalog: answer.catalog.map(|b| *b),
                    origin: answer.origin,
                    source: ViewSource::Daemon,
                    progress: Some(answer.progress),
                    warning: None,
                })
            }
            Err(SentinelError::DaemonUnavailable) => {
                let (catalog, warning) = read_cache::<UtilityCatalog>(&self.paths);
                Ok(UtilitiesView {
                    origin: catalog.as_ref().map(|_| InventoryOrigin::Cache),
                    catalog,
                    source: ViewSource::CacheFile,
                    progress: None,
                    warning,
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Ask the daemon to scan. Scans need the daemon; there is no
    /// client-side fallback, so two scans can never run at once.
    pub fn run_discovery(&self, job: DiscoveryJob, force: bool) -> Result<DiscoveryStatus> {
        self.connect()?.run_discovery(job, force)
    }

    pub fn cancel_discovery(&self, job: DiscoveryJob) -> Result<DiscoveryStatus> {
        self.connect()?.cancel_discovery(job)
    }

    pub fn discovery_status(&self) -> Result<DiscoveryStatus> {
        self.connect()?.discovery_status()
    }

    pub fn capabilities(&self) -> Result<HardwareCapabilities> {
        self.connect()?.hardware_capabilities()
    }

    /// Open an installed utility for the user, through the desktop
    /// environment. Only utilities in the discovered catalogue that have a
    /// desktop entry can be opened; command-line tools are never run.
    pub fn launch_utility(&self, id: &str) -> Result<SystemUtility> {
        if !sentinel_inventory::is_valid_utility_id(id) {
            return Err(SentinelError::Launch {
                what: "the utility".into(),
                reason: "that is not a valid utility identifier".into(),
            });
        }
        let view = self.utilities()?;
        let Some(catalog) = view.catalog else {
            return Err(SentinelError::Launch {
                what: id.into(),
                reason: "no utility catalogue exists yet; scan for utilities first".into(),
            });
        };
        let Some(utility) = catalog.utilities.into_iter().find(|u| u.id == id) else {
            return Err(SentinelError::Launch {
                what: id.into(),
                reason: "it is not in the discovered utility catalogue".into(),
            });
        };
        let entry = match (
            &utility.desktop_entry,
            utility.installed && utility.launch_available,
        ) {
            (Some(entry), true) => entry.clone(),
            _ => {
                return Err(SentinelError::Launch {
                    what: utility.name.clone(),
                    reason: utility
                        .launch_note
                        .clone()
                        .unwrap_or_else(|| "it cannot be opened by Sentinel".into()),
                });
            }
        };
        launcher::open_desktop_entry(std::path::Path::new(&entry.path), &entry.id)?;
        Ok(utility)
    }
}

/// Which telemetry section to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricsSection {
    All,
    Cpu,
    Memory,
    Battery,
    Thermal,
    Gpu,
    Storage,
    Network,
}

impl MetricsSection {
    fn request(self) -> Request {
        match self {
            MetricsSection::All => Request::GetLatestMetrics,
            MetricsSection::Cpu => Request::GetCpuMetrics,
            MetricsSection::Memory => Request::GetMemoryMetrics,
            MetricsSection::Battery => Request::GetBatteryMetrics,
            MetricsSection::Thermal => Request::GetThermalMetrics,
            MetricsSection::Gpu => Request::GetGpuMetrics,
            MetricsSection::Storage => Request::GetStorageMetrics,
            MetricsSection::Network => Request::GetNetworkMetrics,
        }
    }
}

impl Sentinel {
    pub fn monitoring_report(&self) -> Result<MonitoringReport> {
        self.connect()?.monitoring_report()
    }

    /// Live telemetry from the daemon. While monitoring is off the snapshot
    /// is `None`; there is no client-side substitute.
    pub fn metrics(&self, section: MetricsSection) -> Result<MetricsAnswer> {
        self.connect()?.metrics(section.request())
    }

    pub fn processes(&self) -> Result<ProcessAnswer> {
        self.connect()?.processes()
    }

    pub fn active_alerts(&self) -> Result<Vec<Alert>> {
        self.connect()?.alerts(Request::GetActiveAlerts)
    }

    /// Alert history: from the daemon, or read-only from the history
    /// database when it is stopped.
    pub fn alert_history(&self, limit: u32) -> Result<Vec<Alert>> {
        match self.connect() {
            Ok(mut c) => c.alerts(Request::GetAlertHistory { limit }),
            Err(SentinelError::DaemonUnavailable) => {
                let path = self.paths.data_dir.join("history.db");
                Ok(sentinel_history::History::open_read_only(&path)?
                    .map(|h| h.alerts(limit))
                    .transpose()?
                    .unwrap_or_default())
            }
            Err(e) => Err(e),
        }
    }

    /// Mark an alert as seen. This never changes anything on the system.
    pub fn acknowledge_alert(&self, alert_id: u64) -> Result<Alert> {
        let mut alerts = self
            .connect()?
            .alerts(Request::AcknowledgeAlert { alert_id })?;
        alerts
            .pop()
            .ok_or_else(|| SentinelError::Protocol("empty acknowledgement".into()))
    }

    pub fn sampling(&self) -> Result<SamplingInfo> {
        self.connect()?.sampling(Request::GetSamplingProfile)
    }

    pub fn set_sampling(
        &self,
        profile: SamplingProfile,
        power_aware: Option<bool>,
    ) -> Result<SamplingInfo> {
        self.connect()?.sampling(Request::UpdateSamplingProfile {
            profile,
            power_aware,
        })
    }

    pub fn telemetry_history(&self, query: HistoryQuery) -> Result<Vec<Series>> {
        self.connect()?.history(query)
    }
}

fn file_view(store: ConfigStore) -> ConfigView {
    let warning = match store.status() {
        LoadStatus::RecoveredFromCorrupt { reason, .. } => Some(format!(
            "The configuration file could not be read ({reason}). Defaults are shown."
        )),
        LoadStatus::NewerVersion { found } => Some(format!(
            "The configuration file was written by a newer Sentinel (version {found}). \
             Defaults are shown and the file is left untouched."
        )),
        _ => None,
    };
    ConfigView {
        config: store.config().clone(),
        path: store.path().display().to_string(),
        source: ConfigSource::File,
        warning,
    }
}

// ------------------------------------------------------- Phase 6 documents

/// What `open_document` opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenTarget {
    SourcePage,
    DocumentAddress,
    LocalCopy,
}

impl Sentinel {
    pub fn documents(&self) -> Result<sentinel_docs::DocumentsView> {
        self.connect()?.documents(Request::GetDocuments)
    }

    pub fn start_document_discovery(&self, force: bool) -> Result<sentinel_docs::DocumentsView> {
        self.connect()?
            .documents(Request::StartDocumentDiscovery { force })
    }

    pub fn cancel_document_discovery(&self) -> Result<sentinel_docs::DocumentsView> {
        self.connect()?.documents(Request::CancelDocumentDiscovery)
    }

    pub fn download_document(&self, document_id: u64) -> Result<sentinel_docs::DocumentsView> {
        self.connect()?
            .documents(Request::DownloadDocument { document_id })
    }

    pub fn remove_document_file(&self, document_id: u64) -> Result<sentinel_docs::DocumentsView> {
        self.connect()?
            .documents(Request::RemoveDocumentFile { document_id })
    }

    pub fn clear_document_cache(&self) -> Result<sentinel_docs::DocumentsView> {
        self.connect()?.documents(Request::ClearDocumentCache)
    }

    /// Open a discovered document's source page, its address, or the local
    /// copy, with the desktop's default application. The address or file is
    /// taken from the daemon's record (never from the caller) and checked
    /// again here: only public https addresses, and only PDF files that are
    /// still exactly as Sentinel stored them or that sit in the library.
    pub fn open_document(&self, document_id: u64, target: OpenTarget) -> Result<String> {
        let view = self.documents()?;
        let doc = view
            .candidates
            .iter()
            .find(|c| c.id == document_id)
            .ok_or_else(|| SentinelError::Launch {
                what: format!("document {document_id}"),
                reason: "no such document".into(),
            })?;
        let refuse = |reason: String| SentinelError::Launch {
            what: doc.title.chars().take(80).collect(),
            reason,
        };
        let opened = match target {
            OpenTarget::SourcePage | OpenTarget::DocumentAddress => {
                let url = if target == OpenTarget::SourcePage {
                    doc.source
                        .page_url
                        .as_deref()
                        .or(doc.source.document_url.as_deref())
                } else {
                    doc.source.document_url.as_deref()
                };
                let url = url.ok_or_else(|| refuse("no address is known".into()))?;
                sentinel_docs::net::validate_url(url)
                    .map_err(|e| refuse(e.to_string()))?
                    .to_string()
            }
            OpenTarget::LocalCopy => {
                let path = match &doc.retrieval {
                    sentinel_docs::RetrievalStatus::Downloaded { file, .. } => {
                        sentinel_docs::download::verify_stored(Path::new(&view.download_dir), file)
                            .map_err(refuse)?
                    }
                    sentinel_docs::RetrievalStatus::InLibrary { file, .. } => {
                        checked_library_file(Path::new(&view.library_dir), file).map_err(refuse)?
                    }
                    _ => return Err(refuse("there is no local copy".into())),
                };
                let mut head = [0u8; 8];
                let n = std::fs::File::open(&path)
                    .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
                    .map_err(|e| refuse(e.to_string()))?;
                if sentinel_docs::classify::sniff(&head[..n])
                    != sentinel_docs::classify::Sniffed::Pdf
                {
                    return Err(refuse(
                        "only PDF documents are opened; other formats stay in Sentinel's folder"
                            .into(),
                    ));
                }
                path.to_str()
                    .ok_or_else(|| refuse("the path is not valid UTF-8".into()))?
                    .to_string()
            }
        };
        launcher::open_with_default_application(&opened)?;
        Ok(opened)
    }
}

/// A library file: a plain name, a regular file (not a link) directly in
/// the library folder.
fn checked_library_file(dir: &Path, name: &str) -> std::result::Result<PathBuf, String> {
    if name.is_empty() || name.contains('/') || name == ".." || name.starts_with('.') {
        return Err("not a library file name".into());
    }
    let path = dir.join(name);
    let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("the file is missing: {e}"))?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".into());
    }
    let canon = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
    if canon.parent() != std::fs::canonicalize(dir).ok().as_deref() {
        return Err("the file is outside the library folder".into());
    }
    Ok(canon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_config::Theme;
    use std::ffi::OsString;

    fn sentinel_in(dir: &tempfile::TempDir) -> Sentinel {
        let home = dir.path().to_path_buf();
        let paths = AppPaths::resolve(|key| match key {
            "HOME" => Some(home.clone().into_os_string()),
            "XDG_RUNTIME_DIR" => Some(home.join("run").into_os_string()),
            _ => None::<OsString>,
        })
        .unwrap();
        Sentinel::new(paths)
    }

    #[test]
    fn with_no_daemon_everything_reports_stopped_without_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = sentinel_in(&dir);

        assert_eq!(sentinel.probe().unwrap(), None);
        assert!(matches!(
            sentinel.status(),
            Err(SentinelError::DaemonUnavailable)
        ));
        assert_eq!(sentinel.stop().unwrap(), StopOutcome::NotRunning);
        assert!(matches!(
            sentinel.monitoring(MonitoringCommand::Start),
            Err(SentinelError::DaemonUnavailable)
        ));

        let overview = sentinel.overview();
        assert!(!overview.running);
        assert_eq!(overview.ipc, IpcState::NotRunning);
        assert_eq!(overview.daemon, None);
        assert_eq!(overview.operating_mode, PowerMode::Balanced);

        // Looking must not create anything.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn configuration_is_read_and_written_on_disk_when_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = sentinel_in(&dir);

        let view = sentinel.configuration().unwrap();
        assert_eq!(view.source, ConfigSource::File);
        assert_eq!(view.config, SentinelConfig::default());
        assert!(
            !sentinel.paths().config_file().exists(),
            "reading never writes"
        );

        let view = sentinel
            .update_configuration(ConfigPatch {
                theme: Some(Theme::Black),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(view.config.theme, Theme::Black);
        assert_eq!(sentinel.configuration().unwrap().config.theme, Theme::Black);
        assert_eq!(sentinel.overview().operating_mode, PowerMode::Balanced);
    }

    #[test]
    fn inventory_without_daemon_or_cache_is_empty_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = sentinel_in(&dir);
        let view = sentinel.inventory().unwrap();
        assert_eq!(view.inventory, None);
        assert_eq!(view.source, ViewSource::CacheFile);
        assert!(view.progress.is_none());
        assert!(sentinel.utilities().unwrap().catalog.is_none());
        assert!(matches!(
            sentinel.run_discovery(DiscoveryJob::Hardware, true),
            Err(SentinelError::DaemonUnavailable)
        ));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn launching_validates_before_anything_else() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = sentinel_in(&dir);
        for bad in ["", "htop; reboot", "../x", "a b"] {
            assert!(
                matches!(
                    sentinel.launch_utility(bad),
                    Err(SentinelError::Launch { .. })
                ),
                "{bad:?}"
            );
        }
        let err = sentinel.launch_utility("htop").unwrap_err();
        assert!(err.to_string().contains("scan"), "{err}");
    }

    #[test]
    fn corrupt_file_produces_a_warning_not_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = sentinel_in(&dir);
        std::fs::create_dir_all(&sentinel.paths().config_dir).unwrap();
        std::fs::write(sentinel.paths().config_file(), "]]] nope").unwrap();

        let view = sentinel.configuration().unwrap();
        assert_eq!(view.config, SentinelConfig::default());
        assert!(view.warning.is_some());
    }
}
