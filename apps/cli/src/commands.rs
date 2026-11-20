use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use sentinel_client::{IpcState, Overview, Sentinel, StartOutcome, StopOutcome, launcher};
use sentinel_core::{MonitoringState, Result, SentinelError};
use sentinel_ipc::DaemonStatus;

pub fn start(sentinel: &Sentinel) -> Result<ExitCode> {
    match sentinel.start()? {
        StartOutcome::Started(status) => {
            println!("Sentinel daemon started (pid {}).", status.pid);
        }
        StartOutcome::AlreadyRunning(status) => {
            println!(
                "Sentinel daemon is already running (pid {}, up {}).",
                status.pid,
                format_uptime(status.uptime_secs)
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn stop(sentinel: &Sentinel) -> Result<ExitCode> {
    match sentinel.stop()? {
        StopOutcome::Stopped => println!("Sentinel daemon stopped."),
        StopOutcome::NotRunning => println!("Sentinel daemon is not running."),
    }
    Ok(ExitCode::SUCCESS)
}

pub fn status(sentinel: &Sentinel, json: bool) -> Result<ExitCode> {
    let overview = sentinel.overview();
    if json {
        let text = serde_json::to_string_pretty(&overview)
            .map_err(|e| SentinelError::Protocol(e.to_string()))?;
        println!("{text}");
    } else {
        print!("{}", render_status(&overview));
    }
    // Like `systemctl status`: non-zero when the daemon is not running.
    Ok(match overview.ipc {
        IpcState::Connected => ExitCode::SUCCESS,
        IpcState::NotRunning => ExitCode::from(SentinelError::DaemonUnavailable.exit_code()),
        IpcState::Error => ExitCode::from(SentinelError::Ipc(String::new()).exit_code()),
    })
}

pub fn ui() -> Result<ExitCode> {
    let binary = launcher::locate(launcher::DESKTOP_BINARY, launcher::DESKTOP_ENV)?;
    let mut child = launcher::spawn_detached(&binary, &[])?;

    // Catch an immediate failure (missing libraries, no display) instead of
    // claiming a launch that did not happen.
    sleep(Duration::from_millis(500));
    match child.try_wait() {
        Ok(Some(exit)) if !exit.success() => Err(SentinelError::Launch {
            what: "the Sentinel desktop application".into(),
            reason: format!("it exited immediately ({exit})"),
        }),
        Ok(_) => {
            println!("Sentinel desktop application launched.");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => Err(SentinelError::Launch {
            what: "the Sentinel desktop application".into(),
            reason: e.to_string(),
        }),
    }
}

/// A next step for errors where one exists.
pub fn hint_for(error: &SentinelError) -> Option<&'static str> {
    match error {
        SentinelError::DaemonUnavailable => Some("Start it with `sentinel start`."),
        SentinelError::Launch { what, .. } if what == launcher::DESKTOP_BINARY => Some(
            "The desktop application is not installed next to this program. See the README \
             for how to build it.",
        ),
        SentinelError::Launch { what, .. } if what == launcher::DAEMON_BINARY => Some(
            "The daemon binary `sentineld` should be installed in the same directory as \
             `sentinel`.",
        ),
        _ => None,
    }
}

pub fn render_status(overview: &Overview) -> String {
    let mut out = format!("Sentinel {}\n", overview.client_version);
    let mut row = |label: &str, value: String| {
        out.push_str(&format!("  {:<16}{value}\n", format!("{label}:")));
    };

    match &overview.daemon {
        Some(daemon) => {
            row("Daemon", format!("running (pid {})", daemon.pid));
            if daemon.app_version != overview.client_version {
                row("Daemon version", daemon.app_version.clone());
            }
            row("Uptime", format_uptime(daemon.uptime_secs));
        }
        // A daemon that holds its socket but does not answer (busy, stuck,
        // stopped by a debugger) is not "stopped".
        None if overview.ipc == IpcState::Error => {
            row("Daemon", "not answering (see IPC below)".into())
        }
        None => row("Daemon", "stopped".into()),
    }

    row(
        "IPC",
        match overview.ipc {
            IpcState::Connected => format!("connected ({})", overview.socket_path),
            IpcState::NotRunning => "not connected (daemon is not running)".into(),
            IpcState::Error => format!(
                "error: {}",
                overview.ipc_detail.as_deref().unwrap_or("unknown")
            ),
        },
    );
    row(
        "Operating mode",
        format!("{} (sampling profile)", overview.operating_mode),
    );
    if let Some(daemon) = &overview.daemon {
        row("Monitoring", describe_monitoring(daemon));
        if !daemon.storage_available {
            row("Storage", "unavailable (see `sentinel logs`)".into());
        }
        if daemon.recovered_from_unclean_shutdown {
            row(
                "Note",
                "the previous daemon session did not shut down cleanly".into(),
            );
        }
    }
    row("Configuration", overview.config_path.clone());
    out
}

fn describe_monitoring(daemon: &DaemonStatus) -> String {
    let state = match daemon.monitoring.state {
        MonitoringState::Stopped => "stopped",
        MonitoringState::Running => "running",
        MonitoringState::Paused => "paused",
    };
    match daemon.monitoring.state {
        MonitoringState::Stopped => format!("{state} (turn on with `sentinel monitor start`)"),
        _ => state.to_string(),
    }
}

pub fn format_uptime(secs: u64) -> String {
    let (days, hours, minutes, seconds) = (
        secs / 86_400,
        (secs % 86_400) / 3_600,
        (secs % 3_600) / 60,
        secs % 60,
    );
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_config::PowerMode;
    use sentinel_core::DaemonState;
    use sentinel_ipc::MonitoringStatus;

    fn overview(daemon: Option<DaemonStatus>) -> Overview {
        Overview {
            client_version: "0.1.0".into(),
            running: daemon.is_some(),
            ipc: if daemon.is_some() {
                IpcState::Connected
            } else {
                IpcState::NotRunning
            },
            ipc_detail: None,
            socket_path: "/run/user/1000/sentinel/daemon.sock".into(),
            daemon,
            operating_mode: PowerMode::Eco,
            config_path: "/home/u/.config/sentinel/config.toml".into(),
            log_path: "/home/u/.local/state/sentinel/logs/daemon.log".into(),
        }
    }

    fn daemon() -> DaemonStatus {
        DaemonStatus {
            app_version: "0.1.0".into(),
            protocol_version: 1,
            pid: 77,
            state: DaemonState::Running,
            started_at: 0,
            uptime_secs: 133,
            monitoring: MonitoringStatus {
                state: MonitoringState::Stopped,
                engine_available: false,
                note: String::new(),
            },
            operating_mode: PowerMode::Eco,
            config_path: "/home/u/.config/sentinel/config.toml".into(),
            storage_available: true,
            recovered_from_unclean_shutdown: false,
        }
    }

    #[test]
    fn uptime_formatting() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59), "59s");
        assert_eq!(format_uptime(133), "2m 13s");
        assert_eq!(format_uptime(3_700), "1h 1m");
        assert_eq!(format_uptime(90_061), "1d 1h 1m");
    }

    #[test]
    fn running_status_shows_every_required_field() {
        let text = render_status(&overview(Some(daemon())));
        for expected in [
            "Sentinel 0.1.0",
            "running (pid 77)",
            "2m 13s",
            "connected (/run/user/1000/sentinel/daemon.sock)",
            "eco (sampling profile)",
            "/home/u/.config/sentinel/config.toml",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
    }

    #[test]
    fn monitoring_is_never_described_as_active_when_stopped() {
        let text = render_status(&overview(Some(daemon())));
        assert!(text.contains("stopped (turn on with `sentinel monitor start`)"));
        assert!(!text.to_lowercase().contains("monitoring:      running"));
    }

    #[test]
    fn a_daemon_that_does_not_answer_is_not_called_stopped() {
        let mut o = overview(None);
        o.ipc = IpcState::Error;
        o.ipc_detail = Some("timed out waiting for the daemon's answer".into());
        let text = render_status(&o);
        assert!(text.contains("Daemon:         not answering"), "{text}");
        assert!(!text.contains("stopped"), "{text}");
        assert!(text.contains("timed out"));
    }

    #[test]
    fn stopped_status_has_no_uptime_or_monitoring_rows() {
        let text = render_status(&overview(None));
        assert!(text.contains("Daemon:         stopped"));
        assert!(text.contains("not connected"));
        assert!(!text.contains("Uptime"));
        assert!(!text.contains("Monitoring"));
        assert!(text.contains("Configuration:"));
    }

    #[test]
    fn unclean_shutdown_and_missing_storage_are_surfaced() {
        let mut status = daemon();
        status.storage_available = false;
        status.recovered_from_unclean_shutdown = true;
        let text = render_status(&overview(Some(status)));
        assert!(text.contains("Storage:"));
        assert!(text.contains("did not shut down cleanly"));
    }

    #[test]
    fn hints() {
        assert!(hint_for(&SentinelError::DaemonUnavailable).is_some());
        assert!(hint_for(&SentinelError::Timeout(String::new())).is_none());
    }
}
