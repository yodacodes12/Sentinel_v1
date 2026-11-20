//! The complete set of operations the frontend can perform.
//!
//! Each command is a thin adapter over `sentinel-client`. There is no
//! command that takes a path, a program or a shell string, and the Tauri
//! capability file grants the webview nothing beyond window controls. The
//! only free-form input is a utility ID, which is validated and then looked
//! up in the catalogue the daemon discovered.

use sentinel_client::{
    ConfigView, InventoryView, MetricsSection, Overview, Sentinel, UtilitiesView,
};
use sentinel_config::ConfigPatch;
use sentinel_core::{MonitoringCommand, SentinelError};
use sentinel_inventory::{DiscoveryJob, DiscoveryStatus, HardwareCapabilities, SystemUtility};
use sentinel_ipc::{MetricsAnswer, MonitoringStatus, ProcessAnswer};
use sentinel_telemetry::model::{
    Alert, HistoryQuery, MonitoringReport, SamplingInfo, SamplingProfile, Series,
};
use serde::Serialize;

/// What the frontend receives when a command fails: a stable category and a
/// message written for the user. Never a stack trace.
#[derive(Debug, Serialize)]
pub struct CommandError {
    kind: String,
    message: String,
}

impl From<SentinelError> for CommandError {
    fn from(error: SentinelError) -> Self {
        // Full detail goes to the local log; the UI gets the readable part.
        tracing::warn!(kind = error.kind(), %error, "command failed");
        let kind = match &error {
            // Let the UI distinguish daemon refusals such as
            // `monitoring_unavailable` from transport problems.
            SentinelError::Daemon { code, .. } => code.clone(),
            other => other.kind().to_string(),
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

/// Run backend work off the UI thread. The client is blocking by design, so
/// it must not run on the thread that keeps the window responsive.
async fn backend<T, F>(work: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce(&Sentinel) -> sentinel_core::Result<T> + Send + 'static,
{
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let sentinel = Sentinel::from_env()?;
        work(&sentinel)
    })
    .await;

    match joined {
        Ok(result) => result.map_err(CommandError::from),
        Err(error) => {
            tracing::error!(%error, "backend task failed");
            Err(CommandError {
                kind: "internal".into(),
                message: "Sentinel could not complete the request.".into(),
            })
        }
    }
}

#[tauri::command]
pub async fn get_overview() -> Result<Overview, CommandError> {
    backend(|sentinel| Ok(sentinel.overview())).await
}

/// Start the daemon, then report what is actually running.
#[tauri::command]
pub async fn start_daemon() -> Result<Overview, CommandError> {
    backend(|sentinel| {
        sentinel.start()?;
        Ok(sentinel.overview())
    })
    .await
}

/// Stop the daemon, then report what is actually running.
#[tauri::command]
pub async fn stop_daemon() -> Result<Overview, CommandError> {
    backend(|sentinel| {
        sentinel.stop()?;
        Ok(sentinel.overview())
    })
    .await
}

#[tauri::command]
pub async fn get_configuration() -> Result<ConfigView, CommandError> {
    backend(|sentinel| sentinel.configuration()).await
}

#[tauri::command]
pub async fn update_configuration(patch: ConfigPatch) -> Result<ConfigView, CommandError> {
    backend(move |sentinel| sentinel.update_configuration(patch)).await
}

/// Forward a monitoring command. The answer is the state that now holds; a
/// refusal from the daemon (e.g. resume while not paused) is an error.
#[tauri::command]
pub async fn monitoring_command(
    command: MonitoringCommand,
) -> Result<MonitoringStatus, CommandError> {
    backend(move |sentinel| sentinel.monitoring(command)).await
}

#[tauri::command]
pub async fn get_inventory() -> Result<InventoryView, CommandError> {
    backend(|sentinel| sentinel.inventory()).await
}

#[tauri::command]
pub async fn get_utilities() -> Result<UtilitiesView, CommandError> {
    backend(|sentinel| sentinel.utilities()).await
}

#[tauri::command]
pub async fn get_discovery_status() -> Result<DiscoveryStatus, CommandError> {
    backend(|sentinel| sentinel.discovery_status()).await
}

/// Ask the daemon to scan. Returns at once; the UI polls the status.
#[tauri::command]
pub async fn run_discovery(
    job: DiscoveryJob,
    force: bool,
) -> Result<DiscoveryStatus, CommandError> {
    backend(move |sentinel| sentinel.run_discovery(job, force)).await
}

#[tauri::command]
pub async fn cancel_discovery(job: DiscoveryJob) -> Result<DiscoveryStatus, CommandError> {
    backend(move |sentinel| sentinel.cancel_discovery(job)).await
}

#[tauri::command]
pub async fn get_capabilities() -> Result<HardwareCapabilities, CommandError> {
    backend(|sentinel| sentinel.capabilities()).await
}

/// Open an installed utility because the user clicked "Open".
#[tauri::command]
pub async fn launch_utility(id: String) -> Result<SystemUtility, CommandError> {
    backend(move |sentinel| sentinel.launch_utility(&id)).await
}

#[tauri::command]
pub async fn get_monitoring_status() -> Result<MonitoringReport, CommandError> {
    backend(|sentinel| sentinel.monitoring_report()).await
}

/// Live telemetry; `snapshot` is null whenever monitoring is off.
#[tauri::command]
pub async fn get_metrics(section: MetricsSection) -> Result<MetricsAnswer, CommandError> {
    backend(move |sentinel| sentinel.metrics(section)).await
}

#[tauri::command]
pub async fn get_processes() -> Result<ProcessAnswer, CommandError> {
    backend(|sentinel| sentinel.processes()).await
}

#[tauri::command]
pub async fn get_active_alerts() -> Result<Vec<Alert>, CommandError> {
    backend(|sentinel| sentinel.active_alerts()).await
}

#[tauri::command]
pub async fn get_alert_history(limit: u32) -> Result<Vec<Alert>, CommandError> {
    backend(move |sentinel| sentinel.alert_history(limit)).await
}

/// Marks an alert as seen. Nothing on the system changes.
#[tauri::command]
pub async fn acknowledge_alert(alert_id: u64) -> Result<Alert, CommandError> {
    backend(move |sentinel| sentinel.acknowledge_alert(alert_id)).await
}

#[tauri::command]
pub async fn get_sampling() -> Result<SamplingInfo, CommandError> {
    backend(|sentinel| sentinel.sampling()).await
}

#[tauri::command]
pub async fn set_sampling(
    profile: SamplingProfile,
    power_aware: Option<bool>,
) -> Result<SamplingInfo, CommandError> {
    backend(move |sentinel| sentinel.set_sampling(profile, power_aware)).await
}

#[tauri::command]
pub async fn get_telemetry_history(query: HistoryQuery) -> Result<Vec<Series>, CommandError> {
    backend(move |sentinel| sentinel.telemetry_history(query)).await
}

// ----- Phase 6: document discovery. Documents are referred to by the
// numeric ID the daemon assigned; addresses and paths never come from the
// webview.

#[tauri::command]
pub async fn get_documents() -> Result<sentinel_docs::DocumentsView, CommandError> {
    backend(|sentinel| sentinel.documents()).await
}

#[tauri::command]
pub async fn start_document_discovery(
    force: bool,
) -> Result<sentinel_docs::DocumentsView, CommandError> {
    backend(move |sentinel| sentinel.start_document_discovery(force)).await
}

#[tauri::command]
pub async fn cancel_document_discovery() -> Result<sentinel_docs::DocumentsView, CommandError> {
    backend(|sentinel| sentinel.cancel_document_discovery()).await
}

#[tauri::command]
pub async fn download_document(
    document_id: u64,
) -> Result<sentinel_docs::DocumentsView, CommandError> {
    backend(move |sentinel| sentinel.download_document(document_id)).await
}

#[tauri::command]
pub async fn remove_document_file(
    document_id: u64,
) -> Result<sentinel_docs::DocumentsView, CommandError> {
    backend(move |sentinel| sentinel.remove_document_file(document_id)).await
}

#[tauri::command]
pub async fn clear_document_cache() -> Result<sentinel_docs::DocumentsView, CommandError> {
    backend(|sentinel| sentinel.clear_document_cache()).await
}

/// Open a document's source page, address or checked local PDF in the
/// desktop's default application.
#[tauri::command]
pub async fn open_document(
    document_id: u64,
    target: sentinel_client::OpenTarget,
) -> Result<String, CommandError> {
    backend(move |sentinel| sentinel.open_document(document_id, target)).await
}
