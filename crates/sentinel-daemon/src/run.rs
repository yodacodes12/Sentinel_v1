//! Daemon start-up, the main loop and shutdown.

use std::sync::{Arc, Mutex};

use sentinel_config::{ConfigStore, LoadStatus};
use sentinel_core::instance::InstanceLock;
use sentinel_core::{APP_VERSION, AppPaths, DaemonState, Result, SentinelError, platform};
use sentinel_logging::LogOptions;
use sentinel_storage::Storage;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

use crate::context::{DaemonContext, unix_now};
use crate::discovery::{Coordinator, DiscoveryConfig};
use crate::monitoring::{Monitor, MonitorConfig, MonitorSettings};
use crate::server::{ServerLimits, serve};
use crate::socket::BoundSocket;
use sentinel_utilities::UtilityEnv;

pub struct DaemonOptions {
    pub paths: AppPaths,
    /// Mirror log lines to stderr (for foreground runs and systemd).
    pub log_to_stderr: bool,
    /// Read hardware from this directory instead of `/` (test fixtures).
    /// Results are labelled as not describing this machine.
    pub discovery_root: Option<std::path::PathBuf>,
}

/// Environment variable that points discovery at a fixture tree.
pub const DISCOVERY_ROOT_ENV: &str = "SENTINEL_DISCOVERY_ROOT";

/// Run the daemon until it is asked to stop. Returns once everything it
/// created at runtime (socket, lock) has been released.
pub fn run(options: DaemonOptions) -> Result<()> {
    let DaemonOptions {
        paths,
        log_to_stderr,
        discovery_root,
    } = options;

    platform::ensure_supported_platform()?;
    platform::ensure_not_root(platform::current_uid())?;
    paths.ensure_all()?;
    let socket_path = paths.checked_socket_path()?;

    // Taken before anything else is touched: a second daemon exits here
    // without disturbing the first one's socket, log or database.
    let _instance = InstanceLock::acquire(&paths.lock_file())?;

    let mut log_options = LogOptions::for_file(paths.daemon_log_file());
    log_options.stderr = log_to_stderr;
    sentinel_logging::init(&log_options)?;
    tracing::info!(
        version = APP_VERSION,
        pid = std::process::id(),
        "daemon starting"
    );

    let config = ConfigStore::open(&paths.config_file())?;
    log_config_status(&config);
    if let Some(root) = &discovery_root {
        tracing::warn!(root = %root.display(), "hardware discovery reads a test root, not this machine");
    }

    // Session history is useful but not essential: without a working
    // database the daemon still runs and says so in its status.
    let session = open_session(&paths);
    let root: std::path::PathBuf = discovery_root.unwrap_or_else(|| "/".into());
    let monitor = Monitor::new(
        MonitorConfig {
            root: root.clone(),
            history_path: Some(paths.data_dir.join("history.db")),
        },
        MonitorSettings::from_config(config.config()),
    );
    let start_monitoring = config.config().monitoring_enabled;
    let coordinator = Coordinator::new(DiscoveryConfig {
        root,
        utility_env: UtilityEnv::from_env(),
        database: session.is_ok().then(|| paths.database_file()),
    });
    if let Ok((storage, _, _)) = &session {
        coordinator.load_cache(storage);
    }
    let mut ctx = DaemonContext::new(config, monitor, coordinator);
    // Phase 6: documents live in Sentinel's own data directory. Nothing is
    // searched until a client asks; online discovery is off by default.
    let documents_dir = paths.data_dir.join("documents");
    let library_dir = documents_dir.join("library");
    if let Err(error) = sentinel_core::paths::ensure_private_dir(&documents_dir)
        .and_then(|_| sentinel_core::paths::ensure_private_dir(&library_dir))
    {
        tracing::warn!(%error, "the document folders could not be created");
    }
    ctx.documents = crate::documents::Documents::new(crate::documents::DocumentsConfig {
        database: session.is_ok().then(|| paths.database_file()),
        library_dir,
        download_dir: documents_dir.join("downloads"),
        transport: crate::documents::https_transport(),
    });
    let mut session = match session {
        Ok((storage, id, previous_unclean)) => {
            ctx.storage_available = true;
            ctx.recovered_from_unclean_shutdown = previous_unclean;
            if previous_unclean {
                tracing::warn!("the previous daemon session did not shut down cleanly");
            }
            Some((storage, id))
        }
        Err(error) => {
            tracing::error!(%error, "local storage is unavailable; continuing without it");
            None
        }
    };

    // One thread, woken only by socket activity and signals.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|e| SentinelError::Io {
            context: "starting the async runtime".into(),
            source: e,
        })?;

    let ctx = Arc::new(Mutex::new(ctx));
    let served: Result<()> = runtime.block_on(async {
        let socket = BoundSocket::bind(&socket_path)?;
        let (shutdown, _) = watch::channel(false);
        let signals = tokio::spawn(wait_for_signals(shutdown.clone())?);

        lock(&ctx).transition(DaemonState::Running)?;
        tracing::info!("daemon ready");
        // Startup policy: monitoring resumes only if the user left it on.
        if start_monitoring {
            tracing::info!("monitoring was left on by the user; starting it");
            if let Err(error) = lock(&ctx).monitoring.start() {
                tracing::error!(%error, "monitoring could not be started");
            }
        }

        serve(
            socket.listener(),
            ctx.clone(),
            shutdown,
            ServerLimits::default(),
        )
        .await;

        lock(&ctx).transition(DaemonState::ShuttingDown)?;
        tracing::info!("daemon shutting down");
        signals.abort();
        Ok(())
        // `socket` drops here: the listener closes and the file is removed.
    });
    drop(runtime);
    // Stop any scan in progress; its partial result is discarded.
    lock(&ctx).discovery.shutdown();
    // Cancel a document search or download (at most one request timeout).
    lock(&ctx).documents.shutdown();
    // Stop monitoring (history is flushed). The preference is unchanged, so
    // monitoring resumes on the next start if the user had it on.
    if let Err(error) = lock(&ctx).monitoring.stop() {
        tracing::warn!(%error, "monitoring did not stop cleanly");
    }

    if let Some((storage, id)) = session.as_mut()
        && let Err(error) = storage.end_session(*id, unix_now())
    {
        tracing::warn!(%error, "could not record the clean shutdown");
    }

    match served {
        Ok(()) => {
            lock(&ctx).transition(DaemonState::Stopped)?;
            tracing::info!("daemon stopped");
            Ok(())
        }
        Err(error) => {
            tracing::error!(%error, "daemon failed");
            Err(error)
        }
    }
}

fn lock(ctx: &Arc<Mutex<DaemonContext>>) -> std::sync::MutexGuard<'_, DaemonContext> {
    ctx.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn open_session(paths: &AppPaths) -> Result<(Storage, i64, bool)> {
    let mut storage = Storage::open(&paths.database_file())?;
    let start = storage.begin_session(std::process::id(), APP_VERSION, unix_now())?;
    Ok((storage, start.id, start.previous_unclean))
}

fn log_config_status(config: &ConfigStore) {
    match config.status() {
        LoadStatus::Loaded => tracing::info!("configuration loaded"),
        LoadStatus::Defaulted => tracing::info!("no configuration found; defaults written"),
        LoadStatus::Migrated { from } => {
            tracing::info!(from, "configuration migrated to the current version")
        }
        LoadStatus::RecoveredFromCorrupt { reason, backup } => tracing::warn!(
            reason,
            backup = backup.as_ref().map(|p| p.display().to_string()),
            "configuration was unreadable; it was preserved and defaults are in use"
        ),
        LoadStatus::NewerVersion { found } => tracing::warn!(
            found,
            "configuration was written by a newer Sentinel; using defaults and leaving it untouched"
        ),
    }
}

/// Resolve when the daemon should stop: SIGTERM or SIGINT. SIGHUP is ignored
/// so closing the terminal that started the daemon does not stop it.
fn wait_for_signals(
    shutdown: watch::Sender<bool>,
) -> Result<impl Future<Output = ()> + Send + 'static> {
    let register = |kind: SignalKind| {
        signal(kind).map_err(|e| SentinelError::Io {
            context: "installing signal handlers".into(),
            source: e,
        })
    };
    let mut terminate = register(SignalKind::terminate())?;
    let mut interrupt = register(SignalKind::interrupt())?;
    let mut hangup = register(SignalKind::hangup())?;

    Ok(async move {
        loop {
            tokio::select! {
                _ = terminate.recv() => {
                    tracing::info!("received SIGTERM");
                    break;
                }
                _ = interrupt.recv() => {
                    tracing::info!("received SIGINT");
                    break;
                }
                _ = hangup.recv() => tracing::info!("ignoring SIGHUP"),
            }
        }
        let _ = shutdown.send(true);
    })
}
