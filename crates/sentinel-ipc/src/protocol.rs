//! Message types. Changing anything here in an incompatible way requires
//! bumping [`PROTOCOL_VERSION`].

use sentinel_config::{ConfigPatch, PowerMode, SentinelConfig};
use sentinel_core::{DaemonState, MonitoringCommand, MonitoringState};
use sentinel_docs::DocumentsView;
use sentinel_inventory::{
    DiscoveryJob, DiscoveryStatus, HardwareCapabilities, InventoryOrigin, JobProgress,
    SystemInventory, UtilityCatalog,
};
use sentinel_telemetry::model::{
    Alert, HistoryQuery, MonitoringReport, ProcessList, SamplingInfo, SamplingProfile, Series,
    TelemetrySnapshot,
};
use serde::{Deserialize, Serialize};

/// Version of the wire protocol. A daemon answers a request carrying any
/// other version with [`ErrorCode::UnsupportedVersion`].
pub const PROTOCOL_VERSION: u32 = 1;

/// Upper bound for one encoded request, excluding the newline.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Upper bound for one encoded response. Inventories and utility
/// catalogues are a few hundred kilobytes; requests stay small.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Most alerts returned by one `GetAlertHistory`.
pub const MAX_ALERT_HISTORY: u32 = 1_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub version: u32,
    /// Chosen by the client and echoed in the response.
    pub id: u64,
    pub request: Request,
}

impl RequestEnvelope {
    pub fn new(id: u64, request: Request) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            request,
        }
    }
}

/// Everything a client may ask of the daemon. Unknown fields are rejected,
/// so nothing can ride along with a request unnoticed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Request {
    GetDaemonStatus,
    StartMonitoring,
    StopMonitoring,
    PauseMonitoring,
    ResumeMonitoring,
    GetApplicationVersion,
    GetConfiguration,
    UpdateConfiguration {
        patch: ConfigPatch,
    },
    /// Ask the daemon to shut itself down gracefully. This is how
    /// `sentinel stop` works; Sentinel never signals processes.
    Shutdown,
    /// Scan the hardware unless an inventory already exists.
    RunHardwareDiscovery,
    /// Scan the hardware again, replacing the current inventory.
    RefreshHardwareInventory,
    GetHardwareInventory,
    GetDiscoveryStatus,
    /// Catalogue installed utilities unless a catalogue already exists.
    DiscoverSystemUtilities,
    RefreshSystemUtilities,
    GetSystemUtilities,
    GetHardwareCapabilities,
    /// Stop a queued or running scan. Its previous result is kept.
    CancelDiscovery {
        job: DiscoveryJob,
    },
    // ----- Phase 3 - monitoring
    GetMonitoringStatus,
    GetLatestMetrics,
    GetCpuMetrics,
    GetMemoryMetrics,
    GetProcessMetrics,
    GetBatteryMetrics,
    GetThermalMetrics,
    GetGpuMetrics,
    GetStorageMetrics,
    GetNetworkMetrics,
    GetActiveAlerts,
    GetAlertHistory {
        limit: u32,
    },
    /// Mark an alert as seen. Changes nothing on the system.
    AcknowledgeAlert {
        alert_id: u64,
    },
    GetSamplingProfile,
    UpdateSamplingProfile {
        profile: SamplingProfile,
        power_aware: Option<bool>,
    },
    GetTelemetryHistory {
        query: HistoryQuery,
    },
    // ----- Phase 6 - document discovery (opt-in; reads only)
    GetDocuments,
    /// Search for motherboard documentation. Reuses cached results unless
    /// `force` is set or the cache has expired.
    StartDocumentDiscovery {
        force: bool,
    },
    CancelDocumentDiscovery,
    /// Download a discovered document into Sentinel's own folder. Only for
    /// sources whose access status permits it.
    DownloadDocument {
        document_id: u64,
    },
    /// Delete a downloaded copy from Sentinel's own folder.
    RemoveDocumentFile {
        document_id: u64,
    },
    /// Forget search results (downloaded copies are kept).
    ClearDocumentCache,
}

impl Request {
    /// Semantic checks beyond what the type system enforces.
    pub fn validate(&self) -> Result<(), IpcError> {
        match self {
            Request::GetAlertHistory { limit } if *limit == 0 || *limit > MAX_ALERT_HISTORY => {
                Err(IpcError::new(
                    ErrorCode::InvalidRequest,
                    format!("limit must be between 1 and {MAX_ALERT_HISTORY}"),
                ))
            }
            Request::GetTelemetryHistory { query }
                if query.series.is_empty() || query.series.len() > HistoryQuery::MAX_SERIES =>
            {
                Err(IpcError::new(
                    ErrorCode::InvalidRequest,
                    format!("ask for between 1 and {} series", HistoryQuery::MAX_SERIES),
                ))
            }
            Request::UpdateConfiguration { patch } if patch.is_empty() => Err(IpcError::new(
                ErrorCode::InvalidRequest,
                "UpdateConfiguration must change at least one setting",
            )),
            _ => Ok(()),
        }
    }

    pub fn monitoring_command(&self) -> Option<MonitoringCommand> {
        match self {
            Request::StartMonitoring => Some(MonitoringCommand::Start),
            Request::StopMonitoring => Some(MonitoringCommand::Stop),
            Request::PauseMonitoring => Some(MonitoringCommand::Pause),
            Request::ResumeMonitoring => Some(MonitoringCommand::Resume),
            _ => None,
        }
    }

    pub fn from_monitoring_command(command: MonitoringCommand) -> Self {
        match command {
            MonitoringCommand::Start => Request::StartMonitoring,
            MonitoringCommand::Stop => Request::StopMonitoring,
            MonitoringCommand::Pause => Request::PauseMonitoring,
            MonitoringCommand::Resume => Request::ResumeMonitoring,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub version: u32,
    /// The request's identifier, or `None` when the request was too broken
    /// to contain a readable one.
    pub id: Option<u64>,
    pub outcome: Outcome,
}

impl ResponseEnvelope {
    pub fn new(id: Option<u64>, result: Result<Response, IpcError>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            outcome: match result {
                Ok(response) => Outcome::Ok {
                    response: Box::new(response),
                },
                Err(error) => Outcome::Error { error },
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Ok { response: Box<Response> },
    Error { error: IpcError },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Response {
    DaemonStatus(DaemonStatus),
    /// The monitoring state after a monitoring request that was honoured.
    Monitoring(MonitoringStatus),
    ApplicationVersion {
        app_version: String,
        protocol_version: u32,
    },
    Configuration {
        config: SentinelConfig,
        path: String,
    },
    /// The daemon has begun shutting down. The client should wait for the
    /// socket to disappear before reporting the daemon as stopped.
    ShutdownAccepted,
    DiscoveryStatus(DiscoveryStatus),
    HardwareInventory(InventoryAnswer),
    SystemUtilities(UtilitiesAnswer),
    HardwareCapabilities(HardwareCapabilities),
    // Boxed: telemetry answers are much larger than the other responses.
    MonitoringReport(Box<MonitoringReport>),
    Metrics(Box<MetricsAnswer>),
    Processes(Box<ProcessAnswer>),
    Alerts {
        alerts: Vec<Alert>,
    },
    Sampling(SamplingInfo),
    Documents(Box<DocumentsView>),
    History {
        series: Vec<Series>,
    },
}

/// Live telemetry. `snapshot` is `None` whenever monitoring is not active
/// or paused: no measurement is ever presented while monitoring is off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsAnswer {
    pub monitoring: MonitoringReport,
    pub snapshot: Option<TelemetrySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessAnswer {
    pub monitoring: MonitoringReport,
    pub processes: Option<ProcessList>,
}

/// The current hardware inventory, where it came from, and scan progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InventoryAnswer {
    /// `None` until a scan has completed or a cache was loaded.
    pub inventory: Option<Box<SystemInventory>>,
    pub origin: Option<InventoryOrigin>,
    pub progress: JobProgress,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UtilitiesAnswer {
    pub catalog: Option<Box<UtilityCatalog>>,
    pub origin: Option<InventoryOrigin>,
    pub progress: JobProgress,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub app_version: String,
    pub protocol_version: u32,
    pub pid: u32,
    pub state: DaemonState,
    /// Unix time, seconds.
    pub started_at: u64,
    pub uptime_secs: u64,
    pub monitoring: MonitoringStatus,
    /// The stored power-mode preference. It does not drive anything yet.
    pub operating_mode: PowerMode,
    pub config_path: String,
    /// False when the local database could not be opened; the daemon keeps
    /// running without session history.
    pub storage_available: bool,
    /// True when the previous daemon session ended without a clean shutdown.
    pub recovered_from_unclean_shutdown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitoringStatus {
    pub state: MonitoringState,
    /// Whether this build contains a monitoring engine at all.
    pub engine_available: bool,
    /// Plain-language explanation shown to the user.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcError {
    pub code: ErrorCode,
    pub message: String,
}

impl IpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Not JSON, or not shaped like an envelope.
    MalformedRequest,
    UnsupportedVersion,
    /// A well-formed envelope whose request is unknown or fails validation.
    InvalidRequest,
    FrameTooLarge,
    /// The monitor could not be started (e.g. its thread failed to start).
    MonitoringUnavailable,
    InvalidStateTransition,
    ConfigError,
    /// The daemon is shutting down and no longer takes requests.
    ShuttingDown,
    /// Another document job is running.
    Busy,
    /// The referenced record does not exist.
    NotFound,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::MalformedRequest => "malformed_request",
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::FrameTooLarge => "frame_too_large",
            ErrorCode::MonitoringUnavailable => "monitoring_unavailable",
            ErrorCode::InvalidStateTransition => "invalid_state_transition",
            ErrorCode::ConfigError => "config_error",
            ErrorCode::ShuttingDown => "shutting_down",
            ErrorCode::Busy => "busy",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Internal => "internal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_strings_match_serde() {
        for code in [
            ErrorCode::MalformedRequest,
            ErrorCode::UnsupportedVersion,
            ErrorCode::InvalidRequest,
            ErrorCode::FrameTooLarge,
            ErrorCode::MonitoringUnavailable,
            ErrorCode::InvalidStateTransition,
            ErrorCode::ConfigError,
            ErrorCode::ShuttingDown,
            ErrorCode::Internal,
        ] {
            assert_eq!(
                serde_json::to_string(&code).unwrap(),
                format!("\"{}\"", code.as_str())
            );
        }
    }

    #[test]
    fn monitoring_requests_round_trip_through_commands() {
        for command in [
            MonitoringCommand::Start,
            MonitoringCommand::Stop,
            MonitoringCommand::Pause,
            MonitoringCommand::Resume,
        ] {
            let request = Request::from_monitoring_command(command);
            assert_eq!(request.monitoring_command(), Some(command));
        }
        assert_eq!(Request::GetDaemonStatus.monitoring_command(), None);
    }

    #[test]
    fn empty_configuration_update_fails_validation() {
        let request = Request::UpdateConfiguration {
            patch: ConfigPatch::default(),
        };
        assert_eq!(
            request.validate().unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(Request::Shutdown.validate().is_ok());
    }
}
