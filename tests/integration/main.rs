//! Integration tests: the real `sentinel` and `sentineld` binaries working
//! together inside a sandboxed home directory.

mod documents;
mod reliability;
mod telemetry;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};
use sentinel_client::{ConfigSource, IpcState, StopOutcome};
use sentinel_config::{ConfigPatch, PowerMode, SentinelConfig, Theme};
use sentinel_core::{DaemonState, MonitoringCommand, MonitoringState, SentinelError};
use sentinel_tests::{FIXTURES, Sandbox};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap()
}

fn write_config(sandbox: &Sandbox, text: &str) {
    let paths = sandbox.paths();
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_file(), text).unwrap();
}

// ---------------------------------------------------------------- CLI

#[test]
fn cli_starts_queries_and_stops_the_daemon() {
    let sandbox = Sandbox::new();

    sandbox
        .cli(&["status"])
        .expect_code(3)
        .expect_stdout("stopped");

    sandbox
        .cli(&["start"])
        .expect_code(0)
        .expect_stdout("Sentinel daemon started (pid ");

    let status = sandbox.cli(&["status"]).expect_code(0);
    for expected in [
        concat!("Sentinel ", env!("CARGO_PKG_VERSION")),
        "running (pid ",
        "Uptime:",
        "connected (",
        "balanced",
        "config.toml",
    ] {
        assert!(
            status.stdout.contains(expected),
            "missing {expected:?}:\n{}",
            status.stdout
        );
    }

    let json = sandbox.cli(&["status", "--json"]).expect_code(0).json();
    assert_eq!(json["running"], true);
    assert_eq!(json["ipc"], "connected");
    assert_eq!(json["daemon"]["state"], "running");
    let pid = json["daemon"]["pid"].as_u64().unwrap();
    assert!(std::path::Path::new(&format!("/proc/{pid}")).exists());

    sandbox
        .cli(&["stop"])
        .expect_code(0)
        .expect_stdout("Sentinel daemon stopped.");
    // `stop` returns once the daemon has released its instance lock, the last
    // thing it does. Its socket is already gone and the process entry
    // disappears as soon as the kernel finishes tearing it down.
    assert!(!sandbox.paths().socket_path().exists());
    assert!(!sentinel_core::instance::is_locked(
        &sandbox.paths().lock_file()
    ));
    sandbox.wait_until("daemon process exit", || {
        !std::path::Path::new(&format!("/proc/{pid}")).exists()
    });

    sandbox.cli(&["status"]).expect_code(3);
    sandbox
        .cli(&["stop"])
        .expect_code(0)
        .expect_stdout("not running");
}

#[test]
fn starting_twice_keeps_a_single_instance() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let first = sandbox.client().status().unwrap();

    sandbox
        .cli(&["start"])
        .expect_code(0)
        .expect_stdout("already running");
    assert_eq!(sandbox.client().status().unwrap().pid, first.pid);

    // Launching the daemon binary directly is refused too, and leaves the
    // running daemon and its socket alone.
    sandbox
        .run_daemon_to_exit()
        .expect_code(10)
        .expect_stderr("already running");
    assert_eq!(sandbox.client().status().unwrap().pid, first.pid);
}

#[test]
fn concurrent_starts_produce_exactly_one_daemon() {
    let sandbox = Sandbox::new();
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..6)
            .map(|_| scope.spawn(|| sandbox.cli(&["start"])))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for result in &results {
        assert_eq!(result.code, 0, "{result:?}");
    }
    let started = results
        .iter()
        .filter(|r| r.stdout.contains("daemon started"))
        .count();
    assert_eq!(started, 1, "exactly one start may win: {results:?}");
}

#[test]
fn cli_reports_usage_errors_and_launch_failures() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["frobnicate"]).expect_code(2);
    sandbox.cli(&["status", "--bogus"]).expect_code(2);
    sandbox.cli(&[]).expect_code(2);
    sandbox
        .cli(&["--help"])
        .expect_code(0)
        .expect_stdout("start");
    sandbox
        .cli(&["--version"])
        .expect_code(0)
        .expect_stdout(env!("CARGO_PKG_VERSION"));

    // The sandbox points the desktop binary at a path that does not exist.
    sandbox
        .cli(&["ui"])
        .expect_code(8)
        .expect_stderr("could not launch");
}

#[test]
fn logs_command_shows_sentinels_own_log() {
    let sandbox = Sandbox::new();
    sandbox
        .cli(&["logs"])
        .expect_code(0)
        .expect_stderr("No Sentinel log exists yet");

    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["stop"]).expect_code(0);

    let logs = sandbox.cli(&["logs"]).expect_code(0);
    for expected in ["daemon starting", "daemon ready", "daemon stopped"] {
        assert!(
            logs.stdout.contains(expected),
            "missing {expected:?}:\n{}",
            logs.stdout
        );
    }
    let last = sandbox.cli(&["logs", "-n", "1"]).expect_code(0);
    assert_eq!(last.stdout.lines().count(), 1);
    assert!(last.stdout.contains("daemon stopped"));

    let raw = sandbox.cli(&["logs", "--json", "-n", "1"]).expect_code(0);
    assert_eq!(raw.json()["message"], "daemon stopped");

    let mode = std::fs::metadata(sandbox.paths().daemon_log_file())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

// ------------------------------------------- desktop backend interface

#[test]
fn desktop_interface_talks_to_the_daemon() {
    let sandbox = Sandbox::new();
    let client = sandbox.client();

    let before = client.overview();
    assert!(!before.running);
    assert_eq!(before.ipc, IpcState::NotRunning);

    sandbox.cli(&["start"]).expect_code(0);

    let overview = client.overview();
    assert!(overview.running);
    assert_eq!(overview.ipc, IpcState::Connected);
    let daemon = overview.daemon.expect("daemon details present");
    assert_eq!(daemon.state, DaemonState::Running);
    assert_eq!(daemon.app_version, env!("CARGO_PKG_VERSION"));
    assert!(daemon.storage_available);
    assert!(!daemon.recovered_from_unclean_shutdown);

    // Settings round-trip through the daemon and land on disk.
    let view = client
        .update_configuration(ConfigPatch {
            power_mode: Some(PowerMode::Eco),
            theme: Some(Theme::Black),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(view.source, ConfigSource::Daemon);
    assert_eq!(view.config.power_mode, PowerMode::Eco);
    assert_eq!(client.overview().operating_mode, PowerMode::Eco);

    // The monitoring control really drives the engine.
    let status = client.monitoring(MonitoringCommand::Start).unwrap();
    assert_eq!(status.state, MonitoringState::Running);
    assert!(status.engine_available);
    let status = client.monitoring(MonitoringCommand::Stop).unwrap();
    assert_eq!(status.state, MonitoringState::Stopped);

    assert_eq!(client.stop().unwrap(), StopOutcome::Stopped);
    assert!(!client.overview().running);

    // The settings survive the daemon and are served from the file.
    let view = client.configuration().unwrap();
    assert_eq!(view.source, ConfigSource::File);
    assert_eq!(view.config.theme, Theme::Black);

    // ... and are picked up by the next daemon.
    sandbox.cli(&["start"]).expect_code(0);
    assert_eq!(client.status().unwrap().operating_mode, PowerMode::Eco);
}

#[test]
fn many_clients_are_served_concurrently() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let pid = sandbox.client().status().unwrap().pid;

    std::thread::scope(|scope| {
        for _ in 0..12 {
            scope.spawn(|| {
                let client = sandbox.client();
                let mut connection = client.connect().unwrap();
                for _ in 0..25 {
                    assert_eq!(connection.status().unwrap().pid, pid);
                    assert_eq!(client.status().unwrap().pid, pid);
                }
            });
        }
    });
    sandbox.cli(&["status"]).expect_code(0);
}

// ------------------------------------------------- failure and recovery

#[test]
fn daemon_restarts_after_unexpected_termination() {
    let mut sandbox = Sandbox::new();
    let pid = sandbox.spawn_daemon();

    // Simulate a crash: SIGKILL the test's own child. No cleanup can run.
    let mut child = sandbox.take_daemon(pid);
    child.kill().unwrap();
    child.wait().unwrap();

    let socket = sandbox.paths().socket_path();
    assert!(socket.exists(), "a killed daemon leaves its socket behind");
    sandbox
        .cli(&["status"])
        .expect_code(3)
        .expect_stdout("stopped");

    sandbox
        .cli(&["start"])
        .expect_code(0)
        .expect_stdout("started");
    let status = sandbox.client().status().unwrap();
    assert_ne!(status.pid, pid);
    assert!(
        status.recovered_from_unclean_shutdown,
        "the new daemon must notice the previous unclean shutdown"
    );
    sandbox
        .cli(&["status"])
        .expect_code(0)
        .expect_stdout("did not shut down cleanly");

    // A clean restart clears the flag.
    sandbox.cli(&["stop"]).expect_code(0);
    sandbox.cli(&["start"]).expect_code(0);
    assert!(
        !sandbox
            .client()
            .status()
            .unwrap()
            .recovered_from_unclean_shutdown
    );
}

#[test]
fn stale_socket_is_replaced_on_start() {
    let sandbox = Sandbox::new();
    let paths = sandbox.paths();
    std::fs::create_dir_all(&paths.runtime_dir).unwrap();
    drop(UnixListener::bind(paths.socket_path()).unwrap());
    assert!(paths.socket_path().exists());

    sandbox.cli(&["status"]).expect_code(3);
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["status"]).expect_code(0);

    let meta = std::fs::symlink_metadata(paths.socket_path()).unwrap();
    assert!(meta.file_type().is_socket());
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let dir_mode = std::fs::metadata(&paths.runtime_dir)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(dir_mode & 0o777, 0o700);
}

#[test]
fn foreign_file_at_the_socket_path_is_never_deleted() {
    let sandbox = Sandbox::new();
    let paths = sandbox.paths();
    std::fs::create_dir_all(&paths.runtime_dir).unwrap();
    std::fs::write(paths.socket_path(), "not ours").unwrap();

    sandbox
        .cli(&["start"])
        .expect_code(8)
        .expect_stderr("exited during start-up");
    assert_eq!(
        std::fs::read_to_string(paths.socket_path()).unwrap(),
        "not ours"
    );
    // Run directly, the daemon explains itself.
    sandbox
        .run_daemon_to_exit()
        .expect_code(4)
        .expect_stderr("is not a socket");
}

#[test]
fn sigterm_shuts_the_daemon_down_gracefully() {
    let mut sandbox = Sandbox::new();
    let pid = sandbox.spawn_daemon();
    let mut child = sandbox.take_daemon(pid);

    // SIGHUP (terminal closed) must be ignored.
    kill_process(Pid::from_child(&child), Signal::HUP).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(sandbox.client().status().unwrap().pid, pid);

    // SIGTERM (what `systemctl --user stop` sends) is a clean shutdown.
    kill_process(Pid::from_child(&child), Signal::TERM).unwrap();
    let exit = child.wait().unwrap();
    assert!(exit.success(), "graceful shutdown must exit 0, got {exit}");
    assert!(
        !sandbox.paths().socket_path().exists(),
        "socket must be removed"
    );

    sandbox.cli(&["start"]).expect_code(0);
    assert!(
        !sandbox
            .client()
            .status()
            .unwrap()
            .recovered_from_unclean_shutdown,
        "a SIGTERM shutdown is a clean one"
    );
    let logs = sandbox.cli(&["logs"]).expect_code(0);
    assert!(logs.stdout.contains("ignoring SIGHUP"));
    assert!(logs.stdout.contains("received SIGTERM"));
}

// ------------------------------------------------------------ IPC abuse

fn raw_exchange(stream: &mut BufReader<UnixStream>, line: &str) -> serde_json::Value {
    stream.get_mut().write_all(line.as_bytes()).unwrap();
    stream.get_mut().write_all(b"\n").unwrap();
    let mut answer = String::new();
    stream.read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap_or_else(|e| panic!("bad answer {answer:?}: {e}"))
}

#[test]
fn invalid_ipc_requests_are_rejected_without_harming_the_daemon() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let pid = sandbox.client().status().unwrap().pid;

    let stream = UnixStream::connect(sandbox.paths().socket_path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut stream = BufReader::new(stream);

    let cases = [
        ("garbage \u{1} not json", "malformed_request"),
        ("{}", "malformed_request"),
        ("null", "malformed_request"),
        (
            r#"{"version":1,"request":{"type":"GetDaemonStatus"}}"#,
            "malformed_request",
        ),
        (
            r#"{"version":2,"id":1,"request":{"type":"GetDaemonStatus"}}"#,
            "unsupported_version",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"Exec","cmd":"rm -rf /"}}"#,
            "invalid_request",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"KillProcess","pid":1}}"#,
            "invalid_request",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"UpdateConfiguration","patch":{}}}"#,
            "invalid_request",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"UpdateConfiguration","patch":{"power_mode":"x"}}}"#,
            "invalid_request",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"GetAlertHistory","limit":0}}"#,
            "invalid_request",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"UpdateSamplingProfile","profile":"turbo","power_aware":null}}"#,
            "invalid_request",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"GetTelemetryHistory","query":{"series":["$(id)"],"window":"hour1"}}}"#,
            "invalid_request",
        ),
        (
            r#"{"version":1,"id":1,"request":{"type":"ResumeMonitoring"}}"#,
            "invalid_state_transition",
        ),
    ];
    for (line, code) in cases {
        let answer = raw_exchange(&mut stream, line);
        assert_eq!(answer["outcome"]["status"], "error", "{line}");
        assert_eq!(answer["outcome"]["error"]["code"], code, "{line}");
        assert_eq!(answer["version"], 1);
    }

    // The connection and the daemon are both still fine.
    let answer = raw_exchange(
        &mut stream,
        r#"{"version":1,"id":42,"request":{"type":"GetApplicationVersion"}}"#,
    );
    assert_eq!(answer["id"], 42);
    assert_eq!(
        answer["outcome"]["response"]["app_version"],
        env!("CARGO_PKG_VERSION")
    );

    // A flood of oversized junk closes that connection only.
    let mut flood = UnixStream::connect(sandbox.paths().socket_path()).unwrap();
    let _ = flood.write_all(&vec![b'A'; 200_000]);
    drop(flood);

    let status = sandbox.client().status().unwrap();
    assert_eq!(status.pid, pid);
    assert_eq!(status.monitoring.state, MonitoringState::Stopped);
    // None of the rejected requests changed the configuration.
    assert_eq!(
        sandbox.client().configuration().unwrap().config,
        SentinelConfig::default()
    );
}

// -------------------------------------------------------- configuration

#[test]
fn missing_configuration_is_created_with_defaults() {
    let sandbox = Sandbox::new();
    assert!(!sandbox.paths().config_file().exists());
    sandbox.cli(&["start"]).expect_code(0);

    let config_file = sandbox.paths().config_file();
    assert!(config_file.exists());
    let mode = std::fs::metadata(&config_file)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);

    let view = sandbox.client().configuration().unwrap();
    assert_eq!(view.config, SentinelConfig::default());
    assert!(
        !view.config.monitoring_enabled,
        "monitoring is never on by default"
    );
}

#[test]
fn corrupted_configuration_is_preserved_and_replaced() {
    let sandbox = Sandbox::new();
    let corrupt = fixture("config_corrupt.toml");
    write_config(&sandbox, &corrupt);

    sandbox.cli(&["start"]).expect_code(0);
    assert_eq!(
        sandbox.client().configuration().unwrap().config,
        SentinelConfig::default()
    );

    let backups: Vec<_> = std::fs::read_dir(&sandbox.paths().config_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
        .collect();
    assert_eq!(backups.len(), 1, "the unreadable file must be kept");
    assert_eq!(std::fs::read_to_string(backups[0].path()).unwrap(), corrupt);
    sandbox
        .cli(&["logs"])
        .expect_code(0)
        .expect_stdout("configuration was unreadable");
}

#[test]
fn outdated_configuration_is_migrated() {
    let sandbox = Sandbox::new();
    write_config(&sandbox, &fixture("config_v0_unversioned.toml"));
    sandbox.cli(&["start"]).expect_code(0);

    let view = sandbox.client().configuration().unwrap();
    assert_eq!(view.config.version, sentinel_config::CONFIG_VERSION);
    assert_eq!(view.config.power_mode, PowerMode::Performance);
    assert_eq!(view.config.theme, Theme::Black);
    let on_disk = std::fs::read_to_string(sandbox.paths().config_file()).unwrap();
    assert!(on_disk.contains(&format!("version = {}", sentinel_config::CONFIG_VERSION)));
}

#[test]
fn configuration_from_a_newer_sentinel_is_left_untouched() {
    let sandbox = Sandbox::new();
    let newer = fixture("config_newer.toml");
    write_config(&sandbox, &newer);
    sandbox.cli(&["start"]).expect_code(0);

    let client = sandbox.client();
    assert_eq!(
        client.configuration().unwrap().config,
        SentinelConfig::default()
    );
    let err = client
        .update_configuration(ConfigPatch {
            theme: Some(Theme::Black),
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(&err, SentinelError::Daemon { code, .. } if code == "config_error"));
    assert_eq!(
        std::fs::read_to_string(sandbox.paths().config_file()).unwrap(),
        newer
    );
}

#[test]
fn unusable_database_degrades_gracefully() {
    let sandbox = Sandbox::new();
    let paths = sandbox.paths();
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    let junk = "this is not an sqlite database, it is a text file of sufficient length";
    std::fs::write(paths.database_file(), junk).unwrap();

    sandbox.cli(&["start"]).expect_code(0);
    let status = sandbox.client().status().unwrap();
    assert!(!status.storage_available);
    sandbox
        .cli(&["status"])
        .expect_code(0)
        .expect_stdout("Storage:");
    assert_eq!(
        std::fs::read_to_string(paths.database_file()).unwrap(),
        junk,
        "an unreadable database is reported, never deleted or rewritten"
    );
}

#[test]
fn missing_home_is_a_clear_unsupported_environment_error() {
    let output = std::process::Command::new(sentinel_tests::bin("sentinel"))
        .env_clear()
        .arg("status")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(String::from_utf8_lossy(&output.stderr).contains("HOME"));
}

// -------------------------------------------------------------- Phase 2

mod discovery {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    use sentinel_client::ViewSource;
    use sentinel_inventory::{DiscoveryJob, InventoryOrigin, JobState, ModuleOutcome};
    use sentinel_tests::Sandbox;
    use sentinel_tests::hwfixture::{self, HwFixture};

    fn fixture_sandbox(fixture: &HwFixture) -> Sandbox {
        Sandbox::new().with_env("SENTINEL_DISCOVERY_ROOT", fixture.root())
    }

    fn wait_for_scans(sandbox: &Sandbox) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let status = sandbox.client().discovery_status().unwrap();
            if !status.hardware.state.is_active() && !status.utilities.state.is_active() {
                return;
            }
            assert!(Instant::now() < deadline, "scan did not finish");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn full_pipeline_scan_through_cli_and_client() {
        let fixture = hwfixture::amd_laptop();
        let sandbox = fixture_sandbox(&fixture);
        sandbox.cli(&["start"]).expect_code(0);

        // Nothing is scanned automatically.
        let before = sandbox.client().inventory().unwrap();
        assert!(before.inventory.is_none());
        assert_eq!(before.progress.unwrap().state, JobState::Idle);
        sandbox
            .cli(&["hardware"])
            .expect_code(3)
            .expect_stdout("No hardware inventory yet");

        let out = sandbox.cli(&["scan"]).expect_code(0);
        assert!(out.stdout.contains("System identity"), "{}", out.stdout);
        assert!(out.stdout.contains("saved to the local cache"));

        let view = sandbox.client().inventory().unwrap();
        assert_eq!(view.origin, Some(InventoryOrigin::ThisSession));
        assert_eq!(view.source, ViewSource::Daemon);
        let inv = view.inventory.unwrap();
        assert!(inv.root_override.is_some());
        assert_eq!(inv.processors[0].physical_cores.value(), Some(&8));
        assert_eq!(inv.discovery_status.len(), 12);

        let text = sandbox.cli(&["hardware"]).expect_code(0).stdout;
        for expected in [
            "Example Corp",
            "EXB14-A",
            "Example AMD 8-Core Processor",
            "Physical cores:       8",
            "not this machine",
            "Port numbers are Linux's",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
        let json = sandbox.cli(&["hardware", "--json"]).expect_code(0).json();
        assert_eq!(json["origin"], "this_session");
        assert_eq!(
            json["inventory"]["system"]["product_name"]["value"],
            "Example Book 14 AMD"
        );

        // Utilities in a fixture root: no systemd is queried.
        let utilities = sandbox.client().utilities().unwrap().catalog.unwrap();
        let services = utilities
            .discovery_status
            .iter()
            .find(|r| r.module == sentinel_inventory::ModuleId::Services)
            .unwrap();
        assert_eq!(services.outcome, ModuleOutcome::Unavailable);
        assert!(utilities.services.is_empty());
    }

    #[test]
    fn cached_inventory_survives_restarts_and_is_labelled_cached() {
        let fixture = hwfixture::intel_hybrid_laptop();
        let sandbox = fixture_sandbox(&fixture);
        sandbox.cli(&["start"]).expect_code(0);
        sandbox.cli(&["scan", "--hardware"]).expect_code(0);
        let scanned_at = sandbox
            .client()
            .inventory()
            .unwrap()
            .inventory
            .unwrap()
            .discovered_at;
        sandbox.cli(&["stop"]).expect_code(0);

        // Daemon stopped: read straight from the cache file, labelled as such.
        let view = sandbox.client().inventory().unwrap();
        assert_eq!(view.source, ViewSource::CacheFile);
        assert_eq!(view.origin, Some(InventoryOrigin::Cache));
        assert_eq!(view.inventory.unwrap().discovered_at, scanned_at);
        sandbox
            .cli(&["hardware"])
            .expect_code(0)
            .expect_stdout("not live");

        // Restarted daemon: serves the cache, still labelled cached.
        sandbox.cli(&["start"]).expect_code(0);
        let view = sandbox.client().inventory().unwrap();
        assert_eq!(view.origin, Some(InventoryOrigin::Cache));
        assert_eq!(view.source, ViewSource::Daemon);
        let progress = view.progress.unwrap();
        assert_eq!(progress.state, JobState::Idle);
        assert_eq!(progress.last_successful_scan, Some(scanned_at));

        // A non-forced run keeps the cache; a refresh replaces it.
        sandbox
            .client()
            .run_discovery(DiscoveryJob::Hardware, false)
            .unwrap();
        assert_eq!(
            sandbox.client().discovery_status().unwrap().hardware.state,
            JobState::Idle
        );
        std::thread::sleep(Duration::from_millis(1100));
        sandbox
            .client()
            .run_discovery(DiscoveryJob::Hardware, true)
            .unwrap();
        wait_for_scans(&sandbox);
        let view = sandbox.client().inventory().unwrap();
        assert_eq!(view.origin, Some(InventoryOrigin::ThisSession));
        assert!(view.inventory.unwrap().discovered_at > scanned_at);
    }

    #[test]
    fn manual_refresh_picks_up_hardware_changes() {
        let fixture = hwfixture::amd_laptop();
        let sandbox = fixture_sandbox(&fixture);
        sandbox.cli(&["start"]).expect_code(0);
        sandbox.cli(&["scan", "--hardware"]).expect_code(0);
        assert_eq!(
            sandbox
                .client()
                .inventory()
                .unwrap()
                .inventory
                .unwrap()
                .batteries
                .len(),
            1
        );
        fixture.power_supply(
            "BAT1",
            &[("type", "Battery"), ("status", "Full"), ("present", "1")],
        );
        // No rescan happens by itself.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            sandbox
                .client()
                .inventory()
                .unwrap()
                .inventory
                .unwrap()
                .batteries
                .len(),
            1
        );
        sandbox.cli(&["scan", "--hardware"]).expect_code(0);
        assert_eq!(
            sandbox
                .client()
                .inventory()
                .unwrap()
                .inventory
                .unwrap()
                .batteries
                .len(),
            2
        );
    }

    #[test]
    fn partial_discovery_reports_failing_modules_and_keeps_the_rest() {
        let fixture = hwfixture::amd_laptop();
        std::fs::remove_dir_all(fixture.root().join("sys/class/net")).unwrap();
        std::fs::remove_file(fixture.root().join("proc/meminfo")).unwrap();
        let sandbox = fixture_sandbox(&fixture);
        sandbox.cli(&["start"]).expect_code(0);
        let out = sandbox.cli(&["scan", "--hardware"]).expect_code(0);
        assert!(out.stdout.contains("n/a"), "{}", out.stdout);
        let inv = sandbox.client().inventory().unwrap().inventory.unwrap();
        let get = |m| {
            inv.discovery_status
                .iter()
                .find(|r| r.module == m)
                .unwrap()
                .outcome
        };
        assert_eq!(
            get(sentinel_inventory::ModuleId::Network),
            ModuleOutcome::Unavailable
        );
        assert_eq!(
            get(sentinel_inventory::ModuleId::Memory),
            ModuleOutcome::Unavailable
        );
        assert_eq!(
            get(sentinel_inventory::ModuleId::Cpu),
            ModuleOutcome::Completed
        );
        assert!(inv.network.is_empty());
        assert_eq!(inv.graphics.len(), 1);
    }

    #[test]
    fn scans_can_be_cancelled_and_keep_previous_results() {
        let fixture = hwfixture::amd_laptop();
        let sandbox = fixture_sandbox(&fixture);
        sandbox.cli(&["start"]).expect_code(0);
        sandbox.cli(&["scan", "--hardware"]).expect_code(0);
        let first = sandbox
            .client()
            .inventory()
            .unwrap()
            .inventory
            .unwrap()
            .discovered_at;

        // Queue utilities then hardware and cancel hardware at once. Either
        // it is cancelled while queued or while running; in both cases the
        // previous inventory remains.
        let client = sandbox.client();
        client.run_discovery(DiscoveryJob::Utilities, true).unwrap();
        client.run_discovery(DiscoveryJob::Hardware, true).unwrap();
        client.cancel_discovery(DiscoveryJob::Hardware).unwrap();
        wait_for_scans(&sandbox);
        let status = client.discovery_status().unwrap();
        assert!(
            matches!(
                status.hardware.state,
                JobState::Cancelled | JobState::Completed
            ),
            "{:?}",
            status.hardware.state
        );
        if status.hardware.state == JobState::Cancelled {
            let view = client.inventory().unwrap();
            assert_eq!(view.inventory.unwrap().discovered_at, first);
            assert!(
                status
                    .hardware
                    .error
                    .unwrap()
                    .contains("previous result is kept")
            );
        }
        sandbox.cli(&["scan", "--cancel"]).expect_code(0);
        sandbox
            .cli(&["scan", "--status"])
            .expect_code(0)
            .expect_stdout("hardware:");
    }

    #[test]
    fn concurrent_scan_requests_never_run_twice() {
        let fixture = hwfixture::amd_laptop();
        let sandbox = fixture_sandbox(&fixture);
        sandbox.cli(&["start"]).expect_code(0);
        let workers: Vec<_> = (0..6)
            .map(|_| {
                let client = sandbox.client();
                std::thread::spawn(move || {
                    client.run_discovery(DiscoveryJob::Hardware, true).unwrap()
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        wait_for_scans(&sandbox);
        let log = std::fs::read_to_string(sandbox.paths().daemon_log_file()).unwrap();
        let started = log.matches("\"discovery started\"").count();
        let queued = log.matches("\"discovery queued\"").count();
        assert_eq!(started, queued, "each queued scan ran once");
        assert!((1..=6).contains(&started));
        // At most one scan runs at a time: start/finish lines alternate.
        let mut running = 0;
        for line in log.lines() {
            if line.contains("\"discovery started\"") {
                running += 1;
                assert_eq!(running, 1, "two scans overlapped");
            } else if line.contains("\"discovery finished\"") {
                running -= 1;
            }
        }
    }

    #[test]
    fn discovery_needs_the_daemon_and_says_so() {
        let sandbox = Sandbox::new();
        sandbox
            .cli(&["scan"])
            .expect_code(3)
            .expect_stderr("sentinel start");
        sandbox.cli(&["hardware"]).expect_code(3);
        sandbox.cli(&["utilities"]).expect_code(3);
        assert!(
            sandbox.files().is_empty(),
            "looking creates nothing: {:?}",
            sandbox.files()
        );
    }

    #[test]
    fn real_machine_scan_through_the_daemon() {
        let sandbox = Sandbox::new();
        sandbox.cli(&["start"]).expect_code(0);
        sandbox.cli(&["scan"]).expect_code(0);
        let inv = sandbox.client().inventory().unwrap().inventory.unwrap();
        assert!(inv.root_override.is_none());
        assert!(!inv.processors.is_empty());
        let catalog = sandbox.client().utilities().unwrap().catalog.unwrap();
        assert!(!catalog.utilities.is_empty());
        let caps = sandbox.client().capabilities().unwrap();
        assert!(caps.interfaces.iter().any(|c| c.id == "cpu_topology"));
        sandbox
            .cli(&["hardware", "capabilities"])
            .expect_code(0)
            .expect_stdout("cpu_topology");
        sandbox
            .cli(&["utilities", "--all"])
            .expect_code(0)
            .expect_stdout("known utilities are installed");
        sandbox
            .cli(&["utilities", "services"])
            .expect_code(0)
            .expect_stdout("read-only listing");
        sandbox
            .cli(&["utilities", "--category", "bogus"])
            .expect_code(5);
    }

    /// Opening a utility goes through the desktop launcher with the desktop
    /// file's path, and only for entries in the discovered catalogue.
    #[test]
    fn opening_a_utility_uses_the_desktop_launcher_only() {
        let sandbox_dir = tempfile::tempdir().unwrap();
        let bin = sandbox_dir.path().join("bin");
        let apps = sandbox_dir.path().join("share/applications");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&apps).unwrap();
        let record = sandbox_dir.path().join("gio-args");
        // A stand-in for `gio`: records its arguments, launches nothing.
        let gio = bin.join("gio");
        std::fs::write(
            &gio,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&gio, std::fs::Permissions::from_mode(0o755)).unwrap();
        let entry = apps.join("sentinel-test-monitor.desktop");
        std::fs::write(&entry, "[Desktop Entry]\nType=Application\nName=Test Monitor\nExec=/bin/false --never-run\nCategories=System;Monitor;\n").unwrap();

        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let sandbox = Sandbox::new()
            .with_env("PATH", path)
            .with_env("XDG_DATA_DIRS", sandbox_dir.path().join("share"));
        sandbox.cli(&["start"]).expect_code(0);
        sandbox.cli(&["scan", "--utilities"]).expect_code(0);

        // Refused: not in the catalogue, malformed, and command-line tools.
        sandbox
            .cli(&["utilities", "open", "no-such-tool"])
            .expect_code(8)
            .expect_stderr("not in the discovered utility catalogue");
        sandbox
            .cli(&["utilities", "open", "x; rm -rf ~"])
            .expect_code(8)
            .expect_stderr("not a valid utility identifier");
        assert!(
            !record.exists(),
            "nothing was launched for refused requests"
        );

        sandbox
            .cli(&["utilities", "open", "desktop:sentinel-test-monitor"])
            .expect_code(0)
            .expect_stdout("Opened Test Monitor");
        let args = std::fs::read_to_string(&record).unwrap();
        assert_eq!(
            args,
            format!("launch\n{}\n", entry.display()),
            "gio launch <desktop file>, nothing else"
        );
        assert!(
            !args.contains("--never-run"),
            "the Exec line is never interpreted by Sentinel"
        );
    }
}
