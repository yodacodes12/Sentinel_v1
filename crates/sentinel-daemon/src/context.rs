//! The daemon's in-memory state.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sentinel_config::ConfigStore;
use sentinel_core::{APP_VERSION, DaemonState, Result};
use sentinel_ipc::{DaemonStatus, PROTOCOL_VERSION};

use crate::discovery::Coordinator;
use crate::monitoring::Monitor;

pub struct DaemonContext {
    state: DaemonState,
    started: Instant,
    started_at: u64,
    pub config: ConfigStore,
    pub monitoring: Monitor,
    pub discovery: Coordinator,
    pub documents: crate::documents::Documents,
    pub storage_available: bool,
    pub recovered_from_unclean_shutdown: bool,
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl DaemonContext {
    pub fn new(config: ConfigStore, monitoring: Monitor, discovery: Coordinator) -> Self {
        Self {
            state: DaemonState::Starting,
            started: Instant::now(),
            started_at: unix_now(),
            config,
            monitoring,
            discovery,
            documents: crate::documents::Documents::disabled(),
            storage_available: false,
            recovered_from_unclean_shutdown: false,
        }
    }

    pub fn state(&self) -> DaemonState {
        self.state
    }

    /// Advance the lifecycle; illegal steps are errors, never silent.
    pub fn transition(&mut self, next: DaemonState) -> Result<()> {
        self.state = self.state.transition(next)?;
        Ok(())
    }

    pub fn status(&self) -> DaemonStatus {
        DaemonStatus {
            app_version: APP_VERSION.to_string(),
            protocol_version: PROTOCOL_VERSION,
            pid: std::process::id(),
            state: self.state,
            started_at: self.started_at,
            uptime_secs: self.started.elapsed().as_secs(),
            monitoring: self.monitoring.status(),
            operating_mode: self.config.config().power_mode,
            config_path: self.config.path().display().to_string(),
            storage_available: self.storage_available,
            recovered_from_unclean_shutdown: self.recovered_from_unclean_shutdown,
        }
    }
}
