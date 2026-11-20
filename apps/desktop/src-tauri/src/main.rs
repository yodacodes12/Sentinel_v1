//! `sentinel-desktop`: the Sentinel desktop application.
//!
//! This shell contains no monitoring or hardware-discovery logic. It displays
//! what the backend reports, through the same `sentinel-client` interface the CLI
//! uses. The window and the daemon are separate processes: closing the
//! window leaves the daemon exactly as it was.

mod commands;

use sentinel_core::{AppPaths, platform};
use sentinel_logging::LogOptions;

fn main() {
    if let Err(error) = platform::ensure_supported_platform()
        .and_then(|()| platform::ensure_not_root(platform::current_uid()))
    {
        eprintln!("sentinel-desktop: {error}");
        std::process::exit(i32::from(error.exit_code()));
    }

    // The desktop keeps its own log beside the daemon's. Failing to set it
    // up is not a reason to refuse to open the window.
    if let Ok(paths) = AppPaths::from_env()
        && sentinel_core::paths::ensure_private_dir(&paths.log_dir()).is_ok()
    {
        let _ = sentinel_logging::init(&LogOptions::for_file(paths.log_dir().join("desktop.log")));
    }
    tracing::info!(version = sentinel_core::APP_VERSION, "desktop starting");

    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::start_daemon,
            commands::stop_daemon,
            commands::get_configuration,
            commands::update_configuration,
            commands::monitoring_command,
            commands::get_inventory,
            commands::get_utilities,
            commands::get_discovery_status,
            commands::run_discovery,
            commands::cancel_discovery,
            commands::get_capabilities,
            commands::launch_utility,
            commands::get_monitoring_status,
            commands::get_metrics,
            commands::get_processes,
            commands::get_active_alerts,
            commands::get_alert_history,
            commands::acknowledge_alert,
            commands::get_sampling,
            commands::set_sampling,
            commands::get_telemetry_history,
            commands::get_documents,
            commands::start_document_discovery,
            commands::cancel_document_discovery,
            commands::download_document,
            commands::remove_document_file,
            commands::clear_document_cache,
            commands::open_document,
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        tracing::error!(%error, "desktop failed");
        eprintln!("sentinel-desktop: the application could not start: {error}");
        std::process::exit(1);
    }
}
