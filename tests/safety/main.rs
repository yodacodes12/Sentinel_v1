//! Safety tests: evidence that Sentinel stays inside its policy.
//!
//! Behavioural tests observe the real daemon through `/proc`. Static tests
//! scan the source tree so a future change that introduces a forbidden
//! capability fails here, loudly, before it can ship.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sentinel_config::ConfigPatch;
use sentinel_core::{MonitoringCommand, MonitoringState, platform::current_uid};
use sentinel_tests::{Sandbox, proc_field, workspace_root};

/// Put a running daemon through everything Phase 1 can do.
fn exercise(sandbox: &Sandbox) {
    let client = sandbox.client();
    client.status().unwrap();
    client.configuration().unwrap();
    client
        .update_configuration(ConfigPatch {
            monitoring_enabled: Some(true),
            notifications_enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
    for command in [
        MonitoringCommand::Start,
        MonitoringCommand::Pause,
        MonitoringCommand::Resume,
        MonitoringCommand::Stop,
    ] {
        let _ = client.monitoring(command);
    }
    sandbox.cli(&["status"]).expect_code(0);
    sandbox.cli(&["logs"]).expect_code(0);
    // Phase 2: a full read-only scan of the real machine, both jobs.
    sandbox.cli(&["scan"]).expect_code(0);
    sandbox.cli(&["hardware", "--json"]).expect_code(0);
    sandbox.cli(&["utilities", "--json"]).expect_code(0);
    let _ = client.capabilities();
    // Phase 6: with the default configuration document discovery is
    // offline, so a search reads only the (empty) library.
    sandbox.cli(&["docs", "search", "--refresh"]).expect_code(0);
    sandbox.cli(&["docs", "--json"]).expect_code(0);
}

/// Wait until the daemon is back to one thread with no children (a scan's
/// worker thread and its systemd queries have finished).
fn wait_until_quiet(pid: u32) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let children: String = std::fs::read_dir(format!("/proc/{pid}/task"))
            .map(|tasks| {
                tasks
                    .flatten()
                    .map(|t| std::fs::read_to_string(t.path().join("children")).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        if proc_field(pid, "Threads") == "1" && children.trim().is_empty() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the daemon did not return to idle after scanning"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ------------------------------------------------------------ behaviour

#[test]
fn daemon_opens_no_network_sockets() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    exercise(&sandbox);
    let pid = sandbox.client().status().unwrap().pid;

    // Inodes of every socket the daemon holds right now.
    let daemon_sockets = || -> BTreeSet<String> {
        std::fs::read_dir(format!("/proc/{pid}/fd"))
            .unwrap()
            .flatten()
            .filter_map(|entry| std::fs::read_link(entry.path()).ok())
            .filter_map(|target| {
                target
                    .to_str()
                    .and_then(|t| t.strip_prefix("socket:["))
                    .and_then(|t| t.strip_suffix(']'))
                    .map(str::to_string)
            })
            .collect()
    };
    // Inode column of a /proc/net table.
    let inodes = |table: &str, column: usize| -> BTreeSet<String> {
        std::fs::read_to_string(format!("/proc/net/{table}"))
            .unwrap_or_default()
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().nth(column).map(str::to_string))
            .collect()
    };

    // The descriptor list and the kernel tables are read at different
    // instants, and a just-closed client connection may still be going away.
    // Wait for a moment where the daemon's sockets are stable across both.
    let mut sockets = BTreeSet::new();
    let mut unix = BTreeSet::new();
    for _ in 0..50 {
        sockets = daemon_sockets();
        unix = inodes("unix", 6);
        if sockets == daemon_sockets() && sockets.is_subset(&unix) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !sockets.is_empty(),
        "the daemon should at least hold its IPC socket"
    );

    // Positive confirmation: every one of its sockets is a Unix socket...
    for socket in &sockets {
        assert!(
            unix.contains(socket),
            "socket inode {socket} is not AF_UNIX"
        );
    }
    // ...and none of them is an internet or raw socket.
    for table in ["tcp", "tcp6", "udp", "udp6", "raw", "raw6"] {
        let network = inodes(table, 9);
        let overlap: Vec<_> = sockets.intersection(&network).collect();
        assert!(
            overlap.is_empty(),
            "daemon holds a {table} socket: {overlap:?}"
        );
    }
}

#[test]
fn daemon_runs_unprivileged_as_the_invoking_user() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let pid = sandbox.client().status().unwrap().pid;

    let uid = current_uid().to_string();
    assert_ne!(uid, "0", "these tests must not be run as root");
    let uids = proc_field(pid, "Uid");
    assert!(
        uids.split_whitespace().all(|value| value == uid),
        "daemon real/effective/saved/fs uids must all be {uid}: {uids}"
    );
    // No effective or permitted capabilities.
    for field in ["CapEff", "CapPrm"] {
        let caps = proc_field(pid, field);
        assert!(
            caps.trim_start_matches('0').is_empty(),
            "daemon must hold no capabilities ({field}={caps})"
        );
    }
}

#[test]
fn daemon_spawns_no_processes_and_no_extra_threads() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    exercise(&sandbox);
    let pid = sandbox.client().status().unwrap().pid;
    // Scans use a worker thread and short-lived read-only systemd queries;
    // both must be gone once the scan has finished.
    wait_until_quiet(pid);

    assert_eq!(
        proc_field(pid, "Threads"),
        "1",
        "the daemon is single-threaded"
    );
    for task in std::fs::read_dir(format!("/proc/{pid}/task"))
        .unwrap()
        .flatten()
    {
        let children = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
        assert!(
            children.trim().is_empty(),
            "the daemon must not start any process, found children: {children}"
        );
    }
}

#[test]
fn daemon_is_idle_when_nothing_talks_to_it() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let pid = sandbox.client().status().unwrap().pid;
    // Idle must hold after scanning too: no rescans, no timers, no polling.
    sandbox.cli(&["scan"]).expect_code(0);
    wait_until_quiet(pid);
    std::thread::sleep(Duration::from_millis(300)); // let start-up settle

    let wakeups = |pid| -> u64 {
        proc_field(pid, "voluntary_ctxt_switches")
            .parse::<u64>()
            .unwrap()
            + proc_field(pid, "nonvoluntary_ctxt_switches")
                .parse::<u64>()
                .unwrap()
    };
    let cpu_ticks = |pid: u32| -> u64 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        // Fields after the parenthesised command name; utime and stime are
        // the 12th and 13th of those.
        let rest = stat.rsplit_once(')').unwrap().1;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
    };

    let (wakeups_before, cpu_before) = (wakeups(pid), cpu_ticks(pid));
    std::thread::sleep(Duration::from_secs(3));
    let (wakeups_after, cpu_after) = (wakeups(pid), cpu_ticks(pid));

    assert_eq!(
        wakeups_after, wakeups_before,
        "an idle daemon must not wake up at all (no polling, no timers)"
    );
    assert_eq!(cpu_after, cpu_before, "an idle daemon must use no CPU time");
}

#[test]
fn unrelated_processes_are_never_terminated() {
    // A bystander owned by this test, standing in for the user's programs.
    let mut bystander = Command::new("sleep")
        .arg("120")
        .stdin(Stdio::null())
        .spawn()
        .expect("sleep is available");

    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    exercise(&sandbox);
    sandbox.cli(&["stop"]).expect_code(0);
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["stop"]).expect_code(0);

    let still_running = bystander.try_wait().unwrap().is_none();
    let _ = bystander.kill();
    let _ = bystander.wait();
    assert!(
        still_running,
        "Sentinel must never terminate another process"
    );
}

#[test]
fn only_sentinels_own_files_are_written() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    exercise(&sandbox);
    sandbox.cli(&["stop"]).expect_code(0);

    let allowed = [
        "home/.config/sentinel/config.toml",
        "home/.local/share/sentinel/sentinel.db",
        "home/.local/share/sentinel/history.db",
        "home/.local/state/sentinel/logs/daemon.log",
        "run/sentinel/daemon.lock",
    ];
    let files = sandbox.files();
    for file in &files {
        assert!(
            allowed.contains(&file.as_str()),
            "unexpected file written: {file} (all files: {files:?})"
        );
    }
    // Every directory Sentinel created is private to the user.
    use std::os::unix::fs::PermissionsExt;
    let paths = sandbox.paths();
    for dir in [
        &paths.config_dir,
        &paths.data_dir,
        &paths.state_dir,
        &paths.runtime_dir,
    ] {
        let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{} must be 0700", dir.display());
    }
    for file in &files {
        let mode = std::fs::metadata(sandbox.root().join(file))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{file} must not be readable by others");
    }
}

#[test]
fn monitoring_state_is_reported_accurately() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();

    let assert_state = |want: MonitoringState, word: &str, context: &str| {
        let status = client.status().unwrap();
        assert_eq!(status.monitoring.state, want, "{context}");
        assert!(status.monitoring.engine_available, "{context}");
        let text = sandbox.cli(&["status"]).expect_code(0).stdout;
        assert!(
            text.contains(&format!("Monitoring:     {word}")),
            "{context}: status must say monitoring is {word}:\n{text}"
        );
    };

    // Off by default: nothing is collected until the user turns it on.
    assert_state(MonitoringState::Stopped, "stopped", "fresh daemon");
    assert!(client.monitoring(MonitoringCommand::Resume).is_err());
    assert_state(MonitoringState::Stopped, "stopped", "resume while stopped");

    client.monitoring(MonitoringCommand::Start).unwrap();
    assert_state(MonitoringState::Running, "running", "after start");
    client.monitoring(MonitoringCommand::Pause).unwrap();
    assert_state(MonitoringState::Paused, "paused", "after pause");
    client.monitoring(MonitoringCommand::Resume).unwrap();
    assert_state(MonitoringState::Running, "running", "after resume");
    client.monitoring(MonitoringCommand::Stop).unwrap();
    assert_state(MonitoringState::Stopped, "stopped", "after stop");

    // The stored preference decides what the next daemon does, and the
    // status reports the truth rather than the preference.
    client
        .update_configuration(ConfigPatch {
            monitoring_enabled: Some(true),
            ..Default::default()
        })
        .unwrap();
    assert_state(
        MonitoringState::Stopped,
        "stopped",
        "preference alone starts nothing",
    );
    sandbox.cli(&["stop"]).expect_code(0);
    sandbox.cli(&["start"]).expect_code(0);
    assert_state(
        MonitoringState::Running,
        "running",
        "restart with the preference on",
    );

    client
        .update_configuration(ConfigPatch {
            monitoring_enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
    sandbox.cli(&["stop"]).expect_code(0);
    sandbox.cli(&["start"]).expect_code(0);
    assert_state(
        MonitoringState::Stopped,
        "stopped",
        "restart with the preference off",
    );
}

#[test]
fn status_contains_no_telemetry_fields() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let json = sandbox.cli(&["status", "--json"]).expect_code(0).json();

    let keys = |value: &serde_json::Value| -> BTreeSet<String> {
        value.as_object().unwrap().keys().cloned().collect()
    };
    let expect =
        |names: &[&str]| -> BTreeSet<String> { names.iter().map(|s| s.to_string()).collect() };

    // Exact allow-lists: adding a field (say, a CPU reading) must be a
    // deliberate change to this test.
    assert_eq!(
        keys(&json),
        expect(&[
            "client_version",
            "running",
            "ipc",
            "ipc_detail",
            "socket_path",
            "daemon",
            "operating_mode",
            "config_path",
            "log_path",
        ])
    );
    assert_eq!(
        keys(&json["daemon"]),
        expect(&[
            "app_version",
            "protocol_version",
            "pid",
            "state",
            "started_at",
            "uptime_secs",
            "monitoring",
            "operating_mode",
            "config_path",
            "storage_available",
            "recovered_from_unclean_shutdown",
        ])
    );
    assert_eq!(
        keys(&json["daemon"]["monitoring"]),
        expect(&["state", "engine_available", "note"])
    );
}

#[test]
fn discovery_makes_no_network_connections_while_scanning() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    let pid = sandbox.client().status().unwrap().pid;
    let network: BTreeSet<String> = ["tcp", "tcp6", "udp", "udp6", "raw", "raw6"]
        .iter()
        .flat_map(|t| {
            std::fs::read_to_string(format!("/proc/net/{t}"))
                .unwrap_or_default()
                .lines()
                .skip(1)
                .filter_map(|l| l.split_whitespace().nth(9).map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect();
    let client = sandbox.client();
    client
        .run_discovery(sentinel_inventory::DiscoveryJob::Hardware, true)
        .unwrap();
    client
        .run_discovery(sentinel_inventory::DiscoveryJob::Utilities, true)
        .unwrap();
    let mut samples = 0;
    loop {
        // Sample the daemon's open sockets throughout the scan.
        for entry in std::fs::read_dir(format!("/proc/{pid}/fd"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Ok(target) = std::fs::read_link(entry.path()) {
                let target = target.to_string_lossy().to_string();
                if let Some(inode) = target
                    .strip_prefix("socket:[")
                    .and_then(|t| t.strip_suffix(']'))
                {
                    assert!(
                        !network.contains(inode),
                        "the daemon holds a network socket during discovery"
                    );
                }
            }
        }
        samples += 1;
        let status = client.discovery_status().unwrap();
        if !status.hardware.state.is_active() && !status.utilities.state.is_active() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(samples > 1);
}

#[test]
fn discovery_logs_contain_no_machine_identifiers() {
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["scan"]).expect_code(0);
    sandbox.cli(&["stop"]).expect_code(0);
    let log = std::fs::read_to_string(sandbox.paths().daemon_log_file()).unwrap();
    assert!(log.contains("discovery finished"));
    for entry in std::fs::read_dir("/sys/class/net").unwrap().flatten() {
        if let Ok(mac) = std::fs::read_to_string(entry.path().join("address")) {
            let mac = mac.trim();
            if mac != "00:00:00:00:00:00" && !mac.is_empty() {
                assert!(!log.contains(mac), "a MAC address was logged");
            }
        }
    }
    let lower = log.to_lowercase();
    for word in ["serial", "uuid", "product_name", "model_name"] {
        assert!(!lower.contains(word), "log mentions {word}");
    }
    // The inventory's identity values are not logged either.
    let dmi = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
    if dmi.trim().len() > 3 {
        assert!(!log.contains(dmi.trim()), "the product name was logged");
    }
}

#[test]
fn utility_discovery_never_runs_what_it_finds() {
    let bin = tempfile::tempdir().unwrap();
    let marker = bin.path().join("was-executed");
    for name in [
        "htop",
        "inxi",
        "lshw",
        "sensors",
        "smartctl",
        "nvme",
        "systemctl-impostor",
    ] {
        let path = bin.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let sandbox = Sandbox::new().with_env("PATH", path);
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["scan", "--utilities"]).expect_code(0);
    let catalog = sandbox.client().utilities().unwrap().catalog.unwrap();
    let inxi = catalog.utilities.iter().find(|u| u.id == "inxi").unwrap();
    assert!(inxi.installed, "found by name on PATH");
    assert!(!marker.exists(), "a discovered program was executed");
}

#[test]
fn scanning_changes_no_system_file() {
    let watched = [
        "/etc/os-release",
        "/var/lib/dpkg/status",
        "/var/lib/rpm/rpmdb.sqlite",
        "/usr/lib/sysimage/rpm/rpmdb.sqlite",
        "/usr/share/hwdata/pci.ids",
        "/usr/share/misc/pci.ids",
        "/usr/share/hwdata/usb.ids",
    ];
    let stamp = |path: &str| {
        std::fs::metadata(path).ok().map(|m| {
            use std::os::unix::fs::MetadataExt;
            (m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ino())
        })
    };
    let before: Vec<_> = watched.iter().map(|p| stamp(p)).collect();
    let rpm_dir_before: Option<usize> = std::fs::read_dir("/var/lib/rpm").ok().map(|d| d.count());
    let sandbox = Sandbox::new();
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["scan"]).expect_code(0);
    sandbox.cli(&["stop"]).expect_code(0);
    let after: Vec<_> = watched.iter().map(|p| stamp(p)).collect();
    for ((path, b), a) in watched.iter().zip(before).zip(after) {
        // A package manager may legitimately change dpkg's status during the
        // test; only Sentinel is under test, so differences fail loudly.
        assert_eq!(b, a, "{path} changed during a scan");
    }
    assert_eq!(
        rpm_dir_before,
        std::fs::read_dir("/var/lib/rpm").ok().map(|d| d.count()),
        "no RPM lock or journal files"
    );
}

// --------------------------------------------------------------- static

/// Production Rust sources: everything except test modules and comments.
fn production_sources() -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // `tests/` holds a crate's integration tests: test code,
                // like the `#[cfg(test)]` modules cut off below.
                if name != "target" && name != "node_modules" && name != "gen" && name != "tests" {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let root = workspace_root();
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files);
    walk(&root.join("apps"), &mut files);
    assert!(
        files.len() > 15,
        "source scan found too few files: {}",
        files.len()
    );

    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            // Unit tests sit at the end of each file behind `#[cfg(test)]`.
            let production = text.split("#[cfg(test)]").next().unwrap_or("");
            let code: String = production
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            (path.strip_prefix(&root).unwrap().to_path_buf(), code)
        })
        .collect()
}

#[test]
fn processes_are_only_launched_by_the_launcher() {
    // Phase 2 adds exactly one more place that starts a process: the fixed,
    // read-only systemd listings used to discover services.
    let allowed = [
        Path::new("crates/sentinel-client/src/launcher.rs"),
        Path::new("crates/sentinel-utilities/src/services/query.rs"),
    ];
    for (path, code) in production_sources() {
        for needle in [
            "Command::new",
            "process::Command",
            "posix_spawn",
            "execv",
            "fork(",
        ] {
            assert!(
                !code.contains(needle) || allowed.contains(&path.as_path()),
                "{} uses `{needle}`; only the launcher and the systemd query module may start a process",
                path.display()
            );
        }
    }
    // The launcher is only ever asked for Sentinel's own binaries or the
    // desktop's application launchers.
    for (path, code) in production_sources() {
        for (index, _) in code.match_indices("launcher::locate(") {
            let call = &code[index..(index + 90).min(code.len())];
            assert!(
                call.contains("launcher::DAEMON_BINARY")
                    || call.contains("launcher::DESKTOP_BINARY"),
                "{} locates a binary that is not Sentinel's own: {call}",
                path.display()
            );
        }
    }
    let launcher =
        std::fs::read_to_string(workspace_root().join("crates/sentinel-client/src/launcher.rs"))
            .unwrap();
    assert!(launcher.contains("pub const GIO_BINARY: &str = \"gio\";"));
    assert!(launcher.contains("pub const GTK_LAUNCH_BINARY: &str = \"gtk-launch\";"));
    assert!(launcher.contains("if name != GIO_BINARY && name != GTK_LAUNCH_BINARY"));
}

#[test]
fn systemd_queries_only_list_and_never_change_anything() {
    let source = std::fs::read_to_string(
        workspace_root().join("crates/sentinel-utilities/src/services/query.rs"),
    )
    .unwrap();
    let production = source.split("#[cfg(test)]").next().unwrap();
    // Every quoted argument in the query table is a listing verb or a flag.
    let args = production
        .split("pub fn args")
        .nth(1)
        .unwrap()
        .split("\n    }\n")
        .next()
        .unwrap();
    let literals: Vec<&str> = args.split('"').skip(1).step_by(2).collect();
    assert!(!literals.is_empty());
    for literal in &literals {
        assert!(
            literal.starts_with("--") || *literal == "list-units" || *literal == "list-unit-files",
            "systemctl argument `{literal}` is not a read-only listing"
        );
    }
    for verb in [
        "\"start\"",
        "\"stop\"",
        "\"restart\"",
        "\"reload\"",
        "\"enable\"",
        "\"disable\"",
        "\"mask\"",
        "\"unmask\"",
        "\"kill\"",
        "\"isolate\"",
        "\"set-property\"",
        "\"daemon-reload\"",
        "\"edit\"",
        "\"preset\"",
        "\"revert\"",
        "\"reset-failed\"",
        "\"poweroff\"",
        "\"reboot\"",
        "\"suspend\"",
    ] {
        assert!(!production.contains(verb), "query.rs contains {verb}");
    }
    // A slow query is abandoned, never killed.
    assert!(!production.contains(".kill("));
    assert!(production.contains(".env_clear()"));
}

#[test]
fn discovery_crates_never_write_or_change_anything() {
    let write_apis = [
        "File::create",
        "OpenOptions",
        "fs::write",
        "create_dir",
        "remove_file",
        "remove_dir",
        "fs::rename",
        "set_permissions",
        "write_all",
        ".write(",
        "symlink(",
        "hard_link",
        "fs::copy",
        "Storage::open(",
        "set_len",
    ];
    let mut scanned = 0;
    for (path, code) in production_sources() {
        let text = path.to_string_lossy();
        if !(text.starts_with("crates/sentinel-hardware/src")
            || text.starts_with("crates/sentinel-utilities/src")
            || text.starts_with("crates/sentinel-telemetry/src"))
        {
            continue;
        }
        scanned += 1;
        for api in write_apis {
            assert!(
                !code.contains(api),
                "{} uses `{api}`; discovery and telemetry are read-only",
                path.display()
            );
        }
    }
    assert!(
        scanned >= 30,
        "scanned only {scanned} discovery and telemetry files"
    );
    // Filesystem access in the hardware and telemetry crates goes through
    // SysRoot only, so the refuse list (environ, mem, ...) always applies.
    for (path, code) in production_sources() {
        let text = path.to_string_lossy();
        let read_only = text.starts_with("crates/sentinel-hardware/src")
            || text.starts_with("crates/sentinel-telemetry/src");
        if read_only && !text.ends_with("source.rs") {
            assert!(
                !code.contains("std::fs") && !code.contains("fs::read"),
                "{} reads files directly",
                path.display()
            );
        }
    }
}

#[test]
fn no_forbidden_capability_appears_in_the_sources() {
    // Program names as string literals, privilege escalation, signals,
    // network listeners and system locations. None has any business in
    // Sentinel's production code.
    let forbidden = [
        // privilege escalation
        "\"sudo\"",
        "\"pkexec\"",
        "\"doas\"",
        "\"su\"",
        "setuid",
        "polkit",
        // package managers
        "\"apt\"",
        "\"apt-get\"",
        "\"dnf\"",
        "\"yum\"",
        "\"pacman\"",
        "\"zypper\"",
        "\"snap\"",
        "\"flatpak\"",
        "\"dpkg\"",
        "\"rpm\"",
        // service, power, kernel and disk control
        "\"systemctl\"",
        "\"service\"",
        "\"sysctl\"",
        "\"modprobe\"",
        "\"mount\"",
        "\"umount\"",
        "\"fsck\"",
        "\"mkfs\"",
        "\"parted\"",
        "\"fdisk\"",
        "\"cpupower\"",
        "\"iptables\"",
        "\"nft\"",
        "\"ufw\"",
        "\"nmcli\"",
        "\"ip\"",
        // shells
        "\"sh\"",
        "\"bash\"",
        "\"-c\"",
        // signalling other processes
        "kill_process",
        "Signal::",
        "libc::kill",
        ".kill()",
        // network
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "reqwest",
        "hyper::",
        "ureq",
        "rustls",
        "0.0.0.0",
        "127.0.0.1",
        // writing to system locations
        "\"/etc",
        "\"/sys",
        "\"/boot",
        "\"/usr",
        "\"/var",
        "\"/dev",
    ];
    // Exemptions, each for a stated reason:
    // - Phase 3: the telemetry collectors name /sys and /proc in messages
    //   about what they could not read; the crate is held to the same
    //   read-only checks as discovery.
    // - The read-only discovery crates name system locations because they
    //   read them (/sys, /proc, /etc/os-release, /usr/share/hwdata, package
    //   databases). `discovery_crates_never_write_or_change_anything` proves
    //   they cannot write.
    // - The utility catalogue names well-known programs as data to match
    //   against; it contains no process API.
    // - The systemd query module names `systemctl`; its arguments are pinned
    //   by `systemd_queries_only_list_and_never_change_anything`.
    // - The package and desktop-entry adapters name the Snap and Flatpak
    //   directories they list (`/snap`, `/var/lib/flatpak/...`).
    // - Phase 6: `crates/sentinel-docs/src/net.rs` is the one module that
    //   may use HTTPS (`ureq`, `rustls`) for opt-in document discovery. Its
    //   own tests prove it refuses local, private and non-HTTPS targets;
    //   `network_code_is_confined_to_document_discovery` pins the rest.
    let exempt = |path: &Path, needle: &str| -> bool {
        let p = path.to_string_lossy();
        let discovery = p.starts_with("crates/sentinel-hardware/src")
            || p.starts_with("crates/sentinel-utilities/src")
            || p.starts_with("crates/sentinel-telemetry/src");
        let system_path = ["\"/etc", "\"/sys", "\"/usr", "\"/var", "\"/dev"].contains(&needle);
        (discovery && system_path)
            || (p == "crates/sentinel-docs/src/net.rs" && (needle == "ureq" || needle == "rustls"))
            || p == "crates/sentinel-utilities/src/catalogue.rs"
            || (p == "crates/sentinel-utilities/src/services/query.rs" && needle == "\"systemctl\"")
            || ((p == "crates/sentinel-utilities/src/packages.rs"
                || p == "crates/sentinel-utilities/src/desktop.rs")
                && (needle == "\"snap\"" || needle == "\"flatpak\""))
    };
    let catalogue = std::fs::read_to_string(
        workspace_root().join("crates/sentinel-utilities/src/catalogue.rs"),
    )
    .unwrap();
    for api in ["Command::", "std::process", ".spawn(", "execv", "exec("] {
        assert!(
            !catalogue.contains(api),
            "the utility catalogue is data only"
        );
    }
    for (path, code) in production_sources() {
        for needle in forbidden {
            assert!(
                !code.contains(needle) || exempt(&path, needle),
                "{} contains forbidden `{needle}`",
                path.display()
            );
        }
    }
}

#[test]
fn no_crate_uses_unsafe_code() {
    let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
    assert!(manifest.contains("unsafe_code = \"forbid\""));
    for (path, code) in production_sources() {
        assert!(
            !code.contains("unsafe "),
            "{} contains unsafe code",
            path.display()
        );
    }
    // Every Rust package opts in to the workspace lints.
    let root = workspace_root();
    let mut manifests = vec![
        root.join("apps/cli/Cargo.toml"),
        root.join("tests/Cargo.toml"),
    ];
    manifests.extend(
        std::fs::read_dir(root.join("crates"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path().join("Cargo.toml")),
    );
    let desktop = root.join("apps/desktop/src-tauri/Cargo.toml");
    if desktop.exists() {
        manifests.push(desktop);
    }
    for manifest in manifests {
        if manifest.ends_with("tests/Cargo.toml") {
            continue; // test harness only
        }
        let text = std::fs::read_to_string(&manifest).unwrap();
        assert!(
            text.contains("[lints]\nworkspace = true"),
            "{} must inherit the workspace lints",
            manifest.display()
        );
    }
}

#[test]
fn ipc_protocol_has_no_open_ended_requests() {
    // The request enum is the entire attack surface of the socket. This test
    // pins it: adding a request means consciously editing this list.
    let source =
        std::fs::read_to_string(workspace_root().join("crates/sentinel-ipc/src/protocol.rs"))
            .unwrap();
    let body = source
        .split("pub enum Request {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("Request enum found");
    let variants: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|line| line.chars().next().is_some_and(char::is_uppercase))
        .map(|line| line.trim_end_matches([',', '{', ' ']))
        .collect();
    assert_eq!(
        variants,
        [
            "GetDaemonStatus",
            "StartMonitoring",
            "StopMonitoring",
            "PauseMonitoring",
            "ResumeMonitoring",
            "GetApplicationVersion",
            "GetConfiguration",
            "UpdateConfiguration",
            "Shutdown",
            "RunHardwareDiscovery",
            "RefreshHardwareInventory",
            "GetHardwareInventory",
            "GetDiscoveryStatus",
            "DiscoverSystemUtilities",
            "RefreshSystemUtilities",
            "GetSystemUtilities",
            "GetHardwareCapabilities",
            "CancelDiscovery",
            // Phase 3: read-only telemetry, plus alert acknowledgement and
            // the sampling profile, which change only Sentinel's own state.
            "GetMonitoringStatus",
            "GetLatestMetrics",
            "GetCpuMetrics",
            "GetMemoryMetrics",
            "GetProcessMetrics",
            "GetBatteryMetrics",
            "GetThermalMetrics",
            "GetGpuMetrics",
            "GetStorageMetrics",
            "GetNetworkMetrics",
            "GetActiveAlerts",
            "GetAlertHistory",
            "AcknowledgeAlert",
            "GetSamplingProfile",
            "UpdateSamplingProfile",
            "GetTelemetryHistory",
            // Phase 6: document discovery. Opt-in network use happens in the
            // daemon; requests carry only a flag or a numeric document ID.
            "GetDocuments",
            "StartDocumentDiscovery",
            "CancelDocumentDiscovery",
            "DownloadDocument",
            "RemoveDocumentFile",
            "ClearDocumentCache",
        ]
    );
    // The only payloads: typed, bounded values. No free text, no paths.
    for payload in [
        "patch: ConfigPatch",
        "job: DiscoveryJob",
        "limit: u32",
        "alert_id: u64",
        "profile: SamplingProfile",
        "power_aware: Option<bool>",
        "query: HistoryQuery",
        "force: bool",
    ] {
        assert_eq!(body.matches(payload).count(), 1, "{payload}");
    }
    assert_eq!(body.matches("document_id: u64").count(), 2);
    assert_eq!(
        body.matches(": ").count(),
        10,
        "no other request carries data"
    );
    assert!(!body.contains("String") && !body.contains("Vec<") && !body.contains("PathBuf"));
    // A history query names validated series keys and an enumerated window.
    let model =
        std::fs::read_to_string(workspace_root().join("crates/sentinel-telemetry/src/model.rs"))
            .unwrap();
    let query = model
        .split("pub struct HistoryQuery {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap();
    assert_eq!(
        query.split_whitespace().collect::<Vec<_>>(),
        [
            "pub",
            "series:",
            "Vec<SeriesKey>,",
            "pub",
            "window:",
            "TimeWindow,"
        ]
    );
    assert!(
        model.contains("#[serde(try_from = \"String\", into = \"String\")]\npub struct SeriesKey")
    );
}

#[test]
fn systemd_unit_is_a_manual_unprivileged_user_unit() {
    let unit = std::fs::read_to_string(workspace_root().join("resources/systemd/sentinel.service"))
        .unwrap();
    assert!(unit.contains("ExecStart=") && unit.contains("sentineld"));
    for forbidden in [
        "User=root",
        "sudo",
        "CapabilityBoundingSet=CAP",
        "AmbientCapabilities=CAP",
    ] {
        assert!(!unit.contains(forbidden), "unit contains {forbidden}");
    }
    assert!(unit.contains("NoNewPrivileges=yes"));
    // The unit itself stays limited to Unix sockets. Phase 6 ships an
    // optional, manually installed drop-in that adds only IPv4/IPv6 for
    // opt-in document discovery.
    assert!(unit.contains("RestrictAddressFamilies=AF_UNIX\n"));
    let dropin = std::fs::read_to_string(
        workspace_root().join("resources/systemd/sentinel-online-discovery.conf"),
    )
    .unwrap();
    let settings: Vec<&str> = dropin
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(
        settings,
        [
            "[Service]",
            "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6"
        ]
    );
    // Nothing in the repository enables or starts the unit on the user's behalf.
    for (path, code) in production_sources() {
        assert!(
            !code.contains("sentinel.service"),
            "{} refers to the systemd unit; it must stay manual",
            path.display()
        );
    }
}

#[test]
fn desktop_shell_grants_the_webview_only_window_controls() {
    let root = workspace_root().join("apps/desktop/src-tauri");

    // Capabilities: exactly the four window controls, nothing else.
    let capability: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("capabilities/main-window.json")).unwrap(),
    )
    .unwrap();
    let permissions: Vec<&str> = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().expect("plain permission identifiers only"))
        .collect();
    assert_eq!(
        permissions,
        [
            "core:window:allow-minimize",
            "core:window:allow-toggle-maximize",
            "core:window:allow-close",
            "core:window:allow-start-dragging",
        ]
    );
    let capability_files = std::fs::read_dir(root.join("capabilities"))
        .unwrap()
        .count();
    assert_eq!(
        capability_files, 1,
        "no additional capability files may be added unnoticed"
    );

    // No Tauri plugins: no shell, HTTP, file-system, updater or process access.
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(
        !manifest.contains("tauri-plugin"),
        "the desktop shell must not use Tauri plugins"
    );
    let package =
        std::fs::read_to_string(workspace_root().join("apps/desktop/package.json")).unwrap();
    assert!(
        !package.contains("@tauri-apps/plugin"),
        "the frontend must not use Tauri plugins"
    );

    // A content security policy is set and allows no remote origins.
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("tauri.conf.json")).unwrap())
            .unwrap();
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("a CSP is configured");
    assert!(csp.contains("default-src 'self'"));
    assert!(
        !csp.contains("https:") && !csp.contains('*'),
        "CSP must not allow remote content: {csp}"
    );
    assert_eq!(config["app"]["withGlobalTauri"], false);

    // The commands exposed to the frontend are a fixed, reviewed list.
    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    let handlers = main
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("handler list found");
    let commands: Vec<&str> = handlers
        .split(',')
        .map(|c| c.trim().trim_start_matches("commands::"))
        .filter(|c| !c.is_empty())
        .collect();
    assert_eq!(
        commands,
        [
            "get_overview",
            "start_daemon",
            "stop_daemon",
            "get_configuration",
            "update_configuration",
            "monitoring_command",
            "get_inventory",
            "get_utilities",
            "get_discovery_status",
            "run_discovery",
            "cancel_discovery",
            "get_capabilities",
            "launch_utility",
            "get_monitoring_status",
            "get_metrics",
            "get_processes",
            "get_active_alerts",
            "get_alert_history",
            "acknowledge_alert",
            "get_sampling",
            "set_sampling",
            "get_telemetry_history",
            // Phase 6: document discovery, by numeric document ID only.
            "get_documents",
            "start_document_discovery",
            "cancel_document_discovery",
            "download_document",
            "remove_document_file",
            "clear_document_cache",
            "open_document",
        ]
    );
    // The only free-form argument is a utility ID, validated and looked up
    // in the discovered catalogue before anything is launched.
    let commands = std::fs::read_to_string(root.join("src/commands.rs")).unwrap();
    assert_eq!(commands.matches("String)").count(), 1);
    assert!(commands.contains("launch_utility(id: String)"));
    assert!(!commands.contains("PathBuf") && !commands.contains("Command::"));
}

#[test]
fn network_code_is_confined_to_document_discovery() {
    // Only sentinel-docs depends on an HTTP client, and inside it only
    // net.rs uses one.
    let root = workspace_root();
    for entry in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        let manifest = std::fs::read_to_string(entry.path().join("Cargo.toml")).unwrap_or_default();
        let name = entry.file_name().into_string().unwrap();
        for http in ["ureq", "reqwest", "hyper", "rustls"] {
            assert!(
                !manifest.lines().any(|l| l.trim_start().starts_with(http))
                    || name == "sentinel-docs",
                "{name} depends on {http}"
            );
        }
    }
    for dir in ["apps/cli", "apps/desktop/src-tauri", "tests"] {
        let manifest = std::fs::read_to_string(root.join(dir).join("Cargo.toml")).unwrap();
        assert!(
            !manifest.lines().any(|l| ["ureq", "reqwest", "rustls"]
                .iter()
                .any(|h| l.trim_start().starts_with(h))),
            "{dir}"
        );
    }
    for (path, code) in production_sources() {
        if path.starts_with("crates/sentinel-docs/src")
            && path != Path::new("crates/sentinel-docs/src/net.rs")
        {
            for needle in ["ureq", "std::net::Tcp", "UdpSocket", "connect("] {
                assert!(!code.contains(needle), "{} uses `{needle}`", path.display());
            }
        }
    }
    let net = std::fs::read_to_string(root.join("crates/sentinel-docs/src/net.rs")).unwrap();
    for guard in [
        ".https_only(true)",
        ".max_redirects(0)",
        ".proxy(None)",
        "PublicOnlyResolver",
        "fn validate_url",
    ] {
        assert!(net.contains(guard), "net.rs lost `{guard}`");
    }
    // Online discovery and automatic downloads are off by default.
    let docs = sentinel_config::SentinelConfig::default().documents;
    assert!(!docs.online_discovery && !docs.auto_download && docs.catalogue_urls.is_empty());
}
