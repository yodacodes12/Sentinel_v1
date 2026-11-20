//! Request handling, free of any I/O on the socket so it is tested directly.

use sentinel_core::{APP_VERSION, DaemonState, SentinelError};
use sentinel_inventory::DiscoveryJob;
use sentinel_ipc::{
    ErrorCode, InventoryAnswer, IpcError, PROTOCOL_VERSION, Request, Response, UtilitiesAnswer,
};

use crate::context::DaemonContext;

/// What the server must do after sending the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Begin graceful shutdown once the response has been written.
    Shutdown,
}

pub fn handle(ctx: &mut DaemonContext, request: Request) -> (Result<Response, IpcError>, Effect) {
    if !ctx.state().accepts_requests() {
        let message = match ctx.state() {
            DaemonState::Starting => "the daemon is still starting",
            _ => "the daemon is shutting down",
        };
        return (
            Err(IpcError::new(ErrorCode::ShuttingDown, message)),
            Effect::None,
        );
    }

    // Defence in depth: the codec validates on decode, but the handler must
    // not rely on every caller having gone through it.
    if let Err(error) = request.validate() {
        return (Err(error), Effect::None);
    }

    if let Some(command) = request.monitoring_command() {
        let result = crate::telemetry_requests::monitoring_command(ctx, command);
        if let Err(error) = &result {
            tracing::info!(
                ?command,
                code = error.code.as_str(),
                "monitoring request refused"
            );
        }
        return (result, Effect::None);
    }

    match request {
        Request::GetDaemonStatus => (Ok(Response::DaemonStatus(ctx.status())), Effect::None),
        Request::GetApplicationVersion => (
            Ok(Response::ApplicationVersion {
                app_version: APP_VERSION.to_string(),
                protocol_version: PROTOCOL_VERSION,
            }),
            Effect::None,
        ),
        Request::GetConfiguration => (Ok(configuration(ctx)), Effect::None),
        Request::UpdateConfiguration { patch } => match ctx.config.update(&patch) {
            Ok(_) => {
                ctx.monitoring
                    .apply_settings(crate::monitoring::MonitorSettings::from_config(
                        ctx.config.config(),
                    ));
                tracing::info!("configuration updated");
                (Ok(configuration(ctx)), Effect::None)
            }
            Err(error) => {
                tracing::warn!(error = %error, "configuration update rejected");
                (Err(config_error(error)), Effect::None)
            }
        },
        Request::Shutdown => {
            tracing::info!("shutdown requested over IPC");
            (Ok(Response::ShutdownAccepted), Effect::Shutdown)
        }
        Request::RunHardwareDiscovery => {
            discovery(ctx.discovery.request(DiscoveryJob::Hardware, false))
        }
        Request::RefreshHardwareInventory => {
            discovery(ctx.discovery.request(DiscoveryJob::Hardware, true))
        }
        Request::DiscoverSystemUtilities => {
            discovery(ctx.discovery.request(DiscoveryJob::Utilities, false))
        }
        Request::RefreshSystemUtilities => {
            discovery(ctx.discovery.request(DiscoveryJob::Utilities, true))
        }
        Request::CancelDiscovery { job } => discovery(ctx.discovery.cancel(job)),
        Request::GetDiscoveryStatus => discovery(ctx.discovery.status()),
        Request::GetHardwareInventory => {
            let (inventory, origin, progress) = ctx.discovery.hardware();
            let answer = InventoryAnswer {
                inventory: inventory.map(|i| Box::new((*i).clone())),
                origin,
                progress,
            };
            (Ok(Response::HardwareInventory(answer)), Effect::None)
        }
        Request::GetSystemUtilities => {
            let (catalog, origin, progress) = ctx.discovery.utilities();
            let answer = UtilitiesAnswer {
                catalog: catalog.map(|c| Box::new((*c).clone())),
                origin,
                progress,
            };
            (Ok(Response::SystemUtilities(answer)), Effect::None)
        }
        Request::GetHardwareCapabilities => {
            let caps = sentinel_hardware::capabilities::detect(
                &ctx.discovery.root(),
                crate::context::unix_now(),
            );
            (Ok(Response::HardwareCapabilities(caps)), Effect::None)
        }
        Request::GetDocuments => documents(ctx, Ok(())),
        Request::StartDocumentDiscovery { force } => {
            let inventory = ctx.discovery.hardware().0;
            let result =
                ctx.documents
                    .start(inventory.as_deref(), &ctx.config.config().documents, force);
            documents(ctx, result)
        }
        Request::CancelDocumentDiscovery => {
            ctx.documents.cancel();
            documents(ctx, Ok(()))
        }
        Request::DownloadDocument { document_id } => {
            let result = ctx
                .documents
                .download(document_id, &ctx.config.config().documents);
            documents(ctx, result)
        }
        Request::RemoveDocumentFile { document_id } => {
            let result = ctx.documents.remove_file(document_id);
            documents(ctx, result)
        }
        Request::ClearDocumentCache => {
            let inventory = ctx.discovery.hardware().0;
            let result = ctx.documents.clear_cache(inventory.as_deref());
            documents(ctx, result)
        }
        Request::StartMonitoring
        | Request::StopMonitoring
        | Request::PauseMonitoring
        | Request::ResumeMonitoring => unreachable!("monitoring requests are handled above"),
        other => (crate::telemetry_requests::handle(ctx, other), Effect::None),
    }
}

fn documents(
    ctx: &DaemonContext,
    result: Result<(), IpcError>,
) -> (Result<Response, IpcError>, Effect) {
    if let Err(e) = result {
        return (Err(e), Effect::None);
    }
    let inventory = ctx.discovery.hardware().0;
    let view = ctx
        .documents
        .view(inventory.as_deref(), &ctx.config.config().documents);
    (Ok(Response::Documents(Box::new(view))), Effect::None)
}

fn discovery(status: sentinel_inventory::DiscoveryStatus) -> (Result<Response, IpcError>, Effect) {
    (Ok(Response::DiscoveryStatus(status)), Effect::None)
}

fn configuration(ctx: &DaemonContext) -> Response {
    Response::Configuration {
        config: ctx.config.config().clone(),
        path: ctx.config.path().display().to_string(),
    }
}

fn config_error(error: SentinelError) -> IpcError {
    let code = match error {
        SentinelError::InvalidConfig(_) => ErrorCode::ConfigError,
        _ => ErrorCode::Internal,
    };
    IpcError::new(code, error.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::monitoring::{Monitor, MonitorConfig, MonitorSettings};
    use sentinel_config::{ConfigPatch, ConfigStore, PowerMode, Theme};
    use sentinel_core::MonitoringState;

    /// A monitor that reads `dir` (an empty fixture root) and stores no
    /// history.
    pub fn test_monitor(dir: &std::path::Path) -> Monitor {
        Monitor::new(
            MonitorConfig {
                root: dir.to_path_buf(),
                history_path: None,
            },
            MonitorSettings::from_config(&sentinel_config::SentinelConfig::default()),
        )
    }

    pub fn test_discovery(dir: &std::path::Path) -> crate::discovery::Coordinator {
        crate::discovery::Coordinator::new(crate::discovery::DiscoveryConfig {
            root: dir.to_path_buf(),
            utility_env: Default::default(),
            database: None,
        })
    }

    fn running_context(dir: &tempfile::TempDir) -> DaemonContext {
        let config = ConfigStore::open(&dir.path().join("config.toml")).unwrap();
        let mut ctx =
            DaemonContext::new(config, test_monitor(dir.path()), test_discovery(dir.path()));
        ctx.transition(DaemonState::Running).unwrap();
        ctx
    }

    #[test]
    fn status_reflects_real_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        let (result, effect) = handle(&mut ctx, Request::GetDaemonStatus);
        assert_eq!(effect, Effect::None);
        let Ok(Response::DaemonStatus(status)) = result else {
            panic!("unexpected {result:?}");
        };
        assert_eq!(status.state, DaemonState::Running);
        assert_eq!(status.pid, std::process::id());
        assert_eq!(status.app_version, APP_VERSION);
        assert_eq!(status.monitoring.state, MonitoringState::Stopped);
        assert!(status.monitoring.engine_available);
        assert_eq!(status.operating_mode, PowerMode::Balanced);
        assert!(status.config_path.ends_with("config.toml"));
    }

    #[test]
    fn version_request() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        let (result, _) = handle(&mut ctx, Request::GetApplicationVersion);
        assert_eq!(
            result.unwrap(),
            Response::ApplicationVersion {
                app_version: APP_VERSION.into(),
                protocol_version: PROTOCOL_VERSION
            }
        );
    }

    #[test]
    fn monitoring_lifecycle_is_acknowledged_only_when_reached() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        for request in [Request::PauseMonitoring, Request::ResumeMonitoring] {
            let (result, _) = handle(&mut ctx, request);
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidStateTransition);
        }
        let (result, _) = handle(&mut ctx, Request::StartMonitoring);
        assert!(
            matches!(result, Ok(Response::Monitoring(s)) if s.state == MonitoringState::Running)
        );
        assert!(
            ctx.config.config().monitoring_enabled,
            "start saves the preference"
        );
        let (result, _) = handle(&mut ctx, Request::StartMonitoring);
        assert_eq!(result.unwrap_err().code, ErrorCode::InvalidStateTransition);
        let (result, _) = handle(&mut ctx, Request::PauseMonitoring);
        assert!(
            matches!(result, Ok(Response::Monitoring(s)) if s.state == MonitoringState::Paused)
        );
        assert!(
            ctx.config.config().monitoring_enabled,
            "pause keeps the preference"
        );
        let (result, _) = handle(&mut ctx, Request::ResumeMonitoring);
        assert!(
            matches!(result, Ok(Response::Monitoring(s)) if s.state == MonitoringState::Running)
        );
        let (result, _) = handle(&mut ctx, Request::StopMonitoring);
        assert!(
            matches!(result, Ok(Response::Monitoring(s)) if s.state == MonitoringState::Stopped)
        );
        assert!(!ctx.config.config().monitoring_enabled);
        // Off: no live measurements are returned.
        let (result, _) = handle(&mut ctx, Request::GetLatestMetrics);
        let Ok(Response::Metrics(m)) = result else {
            panic!("{result:?}")
        };
        assert!(m.snapshot.is_none());
        assert_eq!(
            m.monitoring.phase,
            sentinel_telemetry::model::MonitoringPhase::Off
        );
    }

    #[test]
    fn configuration_update_is_persisted_and_reflected() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        let (result, _) = handle(
            &mut ctx,
            Request::UpdateConfiguration {
                patch: ConfigPatch {
                    power_mode: Some(PowerMode::Eco),
                    theme: Some(Theme::Black),
                    ..Default::default()
                },
            },
        );
        let Ok(Response::Configuration { config, .. }) = result else {
            panic!("unexpected {result:?}");
        };
        assert_eq!(config.power_mode, PowerMode::Eco);
        assert_eq!(ctx.status().operating_mode, PowerMode::Eco);

        let on_disk = ConfigStore::read(&dir.path().join("config.toml")).unwrap();
        assert_eq!(on_disk.config().theme, Theme::Black);
    }

    #[test]
    fn enabling_the_monitoring_preference_does_not_start_monitoring() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        let (result, _) = handle(
            &mut ctx,
            Request::UpdateConfiguration {
                patch: ConfigPatch {
                    monitoring_enabled: Some(true),
                    ..Default::default()
                },
            },
        );
        assert!(result.is_ok());
        // The preference only matters at the next daemon start.
        let status = ctx.status();
        assert_eq!(status.monitoring.state, MonitoringState::Stopped);
    }

    #[test]
    fn empty_update_is_rejected_even_without_the_codec() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        let (result, _) = handle(
            &mut ctx,
            Request::UpdateConfiguration {
                patch: ConfigPatch::default(),
            },
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn shutdown_is_accepted_with_an_effect() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        let (result, effect) = handle(&mut ctx, Request::Shutdown);
        assert_eq!(result.unwrap(), Response::ShutdownAccepted);
        assert_eq!(effect, Effect::Shutdown);
    }

    #[test]
    fn discovery_requests_answer_with_status_and_never_scan_unasked() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = running_context(&dir);
        let (result, _) = handle(&mut ctx, Request::GetHardwareInventory);
        let Ok(Response::HardwareInventory(answer)) = result else {
            panic!("unexpected {result:?}");
        };
        assert!(answer.inventory.is_none() && answer.origin.is_none());
        assert_eq!(answer.progress.state, sentinel_inventory::JobState::Idle);

        let (result, _) = handle(&mut ctx, Request::RefreshHardwareInventory);
        assert!(matches!(result, Ok(Response::DiscoveryStatus(_))));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while ctx.discovery.status().hardware.state.is_active() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (result, _) = handle(&mut ctx, Request::GetHardwareInventory);
        let Ok(Response::HardwareInventory(answer)) = result else {
            panic!("unexpected {result:?}");
        };
        assert!(answer.inventory.unwrap().root_override.is_some());
        assert_eq!(
            answer.origin,
            Some(sentinel_inventory::InventoryOrigin::ThisSession)
        );

        let (result, _) = handle(&mut ctx, Request::GetHardwareCapabilities);
        assert!(matches!(result, Ok(Response::HardwareCapabilities(_))));
        let (result, _) = handle(
            &mut ctx,
            Request::CancelDiscovery {
                job: DiscoveryJob::Utilities,
            },
        );
        assert!(matches!(result, Ok(Response::DiscoveryStatus(_))));
    }

    #[test]
    fn requests_are_refused_unless_running() {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigStore::open(&dir.path().join("config.toml")).unwrap();
        let mut ctx =
            DaemonContext::new(config, test_monitor(dir.path()), test_discovery(dir.path()));
        let (result, _) = handle(&mut ctx, Request::GetDaemonStatus);
        assert_eq!(result.unwrap_err().code, ErrorCode::ShuttingDown);

        ctx.transition(DaemonState::Running).unwrap();
        ctx.transition(DaemonState::ShuttingDown).unwrap();
        let (result, effect) = handle(&mut ctx, Request::Shutdown);
        assert_eq!(result.unwrap_err().code, ErrorCode::ShuttingDown);
        assert_eq!(effect, Effect::None);
    }
}
