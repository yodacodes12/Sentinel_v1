//! Phase 4: daemon lifecycle and IPC reliability with the real binaries.
//!
//! A frozen daemon (SIGSTOP), connection churn, a crash in the middle of
//! monitoring, and rapid control changes. Signals go only to the test's own
//! sandboxed daemon; the product itself never signals anything.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};
use sentinel_client::IpcState;
use sentinel_config::ConfigPatch;
use sentinel_core::{MonitoringCommand, MonitoringState, SentinelError};
use sentinel_telemetry::{CollectorKind, MonitoringPhase, SamplingProfile};
use sentinel_tests::proc_field;

use crate::telemetry::{Machine, eventually};

fn pid_of(pid: u32) -> Pid {
    Pid::from_raw(pid as i32).expect("a daemon pid is positive")
}

fn threads(pid: u32) -> u32 {
    proc_field(pid, "Threads").parse().unwrap()
}

fn open_fds(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .count()
}

/// Threads of the daemon: one for IPC, plus one while monitoring runs.
/// Anything more would be a second monitor, a leaked scan or a leaked
/// connection handler.
fn assert_threads(pid: u32, monitoring: bool, context: &str) {
    let expected = if monitoring { 2 } else { 1 };
    // A thread that is exiting may still be listed for a moment.
    eventually(context, 10, || threads(pid) == expected);
}

#[test]
fn a_frozen_daemon_makes_requests_fail_in_bounded_time_and_then_recovers() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["monitor", "start"]).expect_code(0);
    let client = sandbox.client();
    let pid = client.status().unwrap().pid;

    /// Resumes the daemon even if an assertion fails, so a failing test
    /// never leaves a frozen process behind.
    struct Frozen(u32);
    impl Drop for Frozen {
        fn drop(&mut self) {
            let _ = kill_process(pid_of(self.0), Signal::CONT);
        }
    }
    kill_process(pid_of(pid), Signal::STOP).unwrap();
    let frozen = Frozen(pid);
    // The kernel still accepts the connection; the answer never comes.
    let started = Instant::now();
    let err = client.status().unwrap_err();
    let waited = started.elapsed();
    assert!(matches!(err, SentinelError::Timeout(_)), "{err:?}");
    assert!(waited < Duration::from_secs(5), "waited {waited:?}");

    // The overview the window shows never fails: it reports the problem.
    let started = Instant::now();
    let overview = client.overview();
    assert_eq!(overview.ipc, IpcState::Error);
    assert!(!overview.running || overview.daemon.is_none());
    assert!(overview.ipc_detail.is_some());
    assert!(started.elapsed() < Duration::from_secs(5));

    // The CLI says so, with the IPC-failure exit code, instead of hanging
    // or calling a running daemon "stopped".
    let started = Instant::now();
    let out = sandbox.cli(&["status"]).expect_code(4);
    assert!(out.stdout.contains("not answering"), "{}", out.stdout);
    assert!(out.stdout.contains("timed out"), "{}", out.stdout);
    assert!(started.elapsed() < Duration::from_secs(8));

    drop(frozen); // SIGCONT
    // Requests that timed out were abandoned by their clients; the daemon
    // works through them and answers new ones normally.
    eventually("the daemon answers again", 10, || client.status().is_ok());
    let status = client.status().unwrap();
    assert_eq!(status.pid, pid, "the same daemon, not a second one");
    assert_eq!(status.monitoring.state, MonitoringState::Running);
    assert_threads(pid, true, "no extra threads after the freeze");
    eventually("monitoring is still sampling", 15, || {
        client
            .monitoring_report()
            .is_ok_and(|r| r.phase == MonitoringPhase::Active && r.last_sample_at.is_some())
    });
    sandbox.cli(&["stop"]).expect_code(0);
}

#[test]
fn connection_churn_leaks_no_descriptors_or_threads() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["monitor", "start"]).expect_code(0);
    let client = sandbox.client();
    let pid = client.status().unwrap().pid;
    let socket = sandbox.paths().socket_path();
    // Let the monitor open its history database and settle.
    eventually("first samples", 10, || {
        client
            .monitoring_report()
            .is_ok_and(|r| r.last_sample_at.is_some())
    });
    let fds_before = open_fds(pid);

    for i in 0..300 {
        match i % 4 {
            // A normal request on a fresh connection, as the window does.
            0 | 1 => {
                client.status().unwrap();
            }
            // Connect and leave without a word.
            2 => drop(UnixStream::connect(&socket).unwrap()),
            // Leave in the middle of a frame.
            _ => {
                let mut s = UnixStream::connect(&socket).unwrap();
                let _ = s.write_all(br#"{"version":1,"id":"#);
            }
        }
    }
    // A burst of simultaneous clients beyond the connection limit.
    let workers: Vec<_> = (0..40)
        .map(|_| {
            let client = sandbox.client();
            std::thread::spawn(move || {
                for _ in 0..10 {
                    let _ = client.status();
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }

    eventually("descriptors return to their level", 10, || {
        open_fds(pid) <= fds_before
    });
    assert_threads(pid, true, "no thread per connection is left behind");
    assert!(client.status().is_ok());
    sandbox.cli(&["stop"]).expect_code(0);
}

#[test]
fn monitoring_comes_back_once_after_a_crash() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    sandbox.cli(&["monitor", "start"]).expect_code(0);
    let client = sandbox.client();
    let pid = client.status().unwrap().pid;
    eventually("first samples", 10, || {
        client
            .monitoring_report()
            .is_ok_and(|r| r.last_sample_at.is_some())
    });

    // Crash: no cleanup runs, the socket and lock are left behind.
    kill_process(pid_of(pid), Signal::KILL).unwrap();
    eventually("the daemon process is gone", 10, || {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| s.contains(") Z "))
            .unwrap_or(true)
    });
    assert!(matches!(
        client.status(),
        Err(SentinelError::DaemonUnavailable)
    ));
    let overview = client.overview();
    assert!(!overview.running);
    assert_eq!(overview.ipc, IpcState::NotRunning);

    sandbox.cli(&["start"]).expect_code(0);
    let status = client.status().unwrap();
    assert_ne!(status.pid, pid);
    assert!(status.recovered_from_unclean_shutdown);
    // The user's choice (monitoring on) is honoured, with one monitor.
    eventually("monitoring resumes after the crash", 10, || {
        client
            .monitoring_report()
            .is_ok_and(|r| r.phase == MonitoringPhase::Active)
    });
    assert_threads(status.pid, true, "exactly one monitor after recovery");
    let report = client.monitoring_report().unwrap();
    let mut kinds: Vec<CollectorKind> = report.collectors.iter().map(|c| c.kind).collect();
    let n = kinds.len();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), n, "every collector is listed once");
    sandbox.cli(&["stop"]).expect_code(0);
}

#[test]
fn rapid_control_changes_never_duplicate_the_monitor() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();
    let pid = client.status().unwrap().pid;
    assert_threads(pid, false, "idle daemon");

    for round in 0..5 {
        client.monitoring(MonitoringCommand::Start).unwrap();
        for profile in [
            SamplingProfile::Eco,
            SamplingProfile::Performance,
            SamplingProfile::Balanced,
        ] {
            client.set_sampling(profile, Some(false)).unwrap();
        }
        client.monitoring(MonitoringCommand::Pause).unwrap();
        // Starting while paused, or resuming twice, is refused, not doubled.
        assert!(client.monitoring(MonitoringCommand::Start).is_err());
        client.monitoring(MonitoringCommand::Resume).unwrap();
        assert!(client.monitoring(MonitoringCommand::Resume).is_err());
        assert!(client.monitoring(MonitoringCommand::Start).is_err());
        assert_threads(pid, true, &format!("round {round}: one monitor"));
        client.monitoring(MonitoringCommand::Stop).unwrap();
        assert_threads(pid, false, &format!("round {round}: monitor gone"));
    }

    // The last choices are applied and survive a daemon restart.
    client.monitoring(MonitoringCommand::Start).unwrap();
    client
        .set_sampling(SamplingProfile::Eco, Some(false))
        .unwrap();
    eventually("Eco is in effect", 10, || {
        client
            .sampling()
            .is_ok_and(|s| s.effective == SamplingProfile::Eco)
    });
    sandbox.cli(&["stop"]).expect_code(0);
    sandbox.cli(&["start"]).expect_code(0);
    eventually("monitoring is back after the restart", 10, || {
        client
            .monitoring_report()
            .is_ok_and(|r| r.phase == MonitoringPhase::Active)
    });
    let sampling = client.sampling().unwrap();
    assert_eq!(sampling.selected, SamplingProfile::Eco);
    assert_eq!(sampling.effective, SamplingProfile::Eco);
    assert!(!sampling.power_aware);
    let cpu = sampling
        .intervals_ms
        .iter()
        .find(|(k, _)| *k == CollectorKind::Cpu)
        .unwrap()
        .1;
    assert_eq!(cpu, 5_000, "Eco samples the CPU every 5 s");
    assert_threads(client.status().unwrap().pid, true, "one monitor");
    sandbox.cli(&["stop"]).expect_code(0);
}

#[test]
fn invalid_settings_are_refused_and_change_nothing() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();
    let file = sandbox.paths().config_file();
    let before = std::fs::read_to_string(&file).unwrap();

    let mut bad = client.configuration().unwrap().config.alerts;
    bad.cpu_high_percent = 150.0;
    let err = client
        .update_configuration(ConfigPatch {
            alerts: Some(bad),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.to_string().contains("cpu_high_percent"), "{err}");
    let mut bad = client.configuration().unwrap().config.alerts;
    bad.memory_available_critical_percent = 30.0; // above the warning level
    assert!(
        client
            .update_configuration(ConfigPatch {
                alerts: Some(bad),
                ..Default::default()
            })
            .is_err()
    );
    let mut bad = client.configuration().unwrap().config.retention;
    bad.detailed_hours = 0;
    assert!(
        client
            .update_configuration(ConfigPatch {
                retention: Some(bad),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);

    // A valid change is applied by the daemon, written, and kept.
    let mut good = client.configuration().unwrap().config.alerts;
    good.cpu_high_percent = 85.0;
    client
        .update_configuration(ConfigPatch {
            alerts: Some(good),
            notifications_enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
    sandbox.cli(&["stop"]).expect_code(0);
    sandbox.cli(&["start"]).expect_code(0);
    let config = client.configuration().unwrap().config;
    assert_eq!(config.alerts.cpu_high_percent, 85.0);
    assert!(!config.notifications_enabled);
    sandbox.cli(&["monitor", "start"]).expect_code(0);
    eventually("the monitor applies the saved setting", 10, || {
        client
            .monitoring_report()
            .is_ok_and(|r| r.phase == MonitoringPhase::Active && !r.notifications.enabled)
    });
    sandbox.cli(&["stop"]).expect_code(0);
}

/// A desktop without a battery, and a CPU-only ARM board without a GPU,
/// battery or hwmon sensors: monitoring runs, reports what is missing as
/// unsupported, invents nothing, and never switches to Eco for a battery
/// that does not exist.
#[test]
fn machines_without_battery_or_gpu_are_monitored_honestly() {
    use sentinel_client::MetricsSection;
    use sentinel_telemetry::CollectorHealth;
    use sentinel_tests::Sandbox;
    use sentinel_tests::hwfixture;

    for (name, fixture, has_gpu) in [
        (
            "desktop without battery",
            hwfixture::desktop_missing_dmi(),
            true,
        ),
        ("ARM board, CPU only", hwfixture::arm_board(), false),
    ] {
        fixture
            .file(
                "proc/stat",
                "cpu  400 0 100 1500 0 0 0 0 0 0\ncpu0 400 0 100 1500 0 0 0 0 0 0\n",
            )
            .file("proc/loadavg", "0.10 0.10 0.10 1/100 42\n")
            .file("proc/uptime", "500.00 400.00\n")
            // Present on every Linux system; this test is about batteries and GPUs.
            .file(
                "proc/diskstats",
                " 8 0 sda 100 0 2000 0 50 0 1000 0 0 100 0\n",
            )
            .file("proc/self/mounts", "")
            .file("proc/net/dev", "Inter-|\n face |\n");
        let sandbox = Sandbox::new().with_env("SENTINEL_DISCOVERY_ROOT", fixture.root());
        sandbox.cli(&["start"]).expect_code(0);
        let client = sandbox.client();
        // Power-aware sampling on: without a battery it must change nothing.
        client
            .set_sampling(SamplingProfile::Performance, Some(true))
            .unwrap();
        sandbox.cli(&["monitor", "start"]).expect_code(0);
        eventually(&format!("{name}: every collector ran"), 15, || {
            client.monitoring_report().is_ok_and(|r| {
                r.collectors
                    .iter()
                    .all(|c| c.health != CollectorHealth::Waiting)
            })
        });
        let report = client.monitoring_report().unwrap();
        let health = |kind| {
            report
                .collectors
                .iter()
                .find(|c| c.kind == kind)
                .unwrap()
                .health
        };
        assert_eq!(
            report.sampling.effective,
            SamplingProfile::Performance,
            "{name}"
        );
        assert!(report.sampling.reason.is_none(), "{name}");
        for c in &report.collectors {
            assert_ne!(
                c.health,
                CollectorHealth::Failing,
                "{name}: {:?} {:?}",
                c.kind,
                c.note
            );
        }
        assert_eq!(
            health(CollectorKind::Battery),
            CollectorHealth::Unsupported,
            "{name}"
        );
        if !has_gpu {
            assert_eq!(
                health(CollectorKind::Gpu),
                CollectorHealth::Unsupported,
                "{name}"
            );
        }

        let snap = client
            .metrics(MetricsSection::All)
            .unwrap()
            .snapshot
            .unwrap();
        assert!(
            snap.battery.as_ref().is_none_or(|b| b.batteries.is_empty()),
            "{name}: no battery may be invented"
        );
        if !has_gpu {
            assert!(
                snap.gpu.as_ref().is_none_or(|g| g.gpus.is_empty()),
                "{name}"
            );
        }
        assert!(
            client
                .active_alerts()
                .unwrap()
                .iter()
                .all(|a| !a.rule.starts_with("battery")),
            "{name}"
        );
        // The CLI says so rather than printing zeros.
        let out = sandbox.cli(&["metrics", "battery"]).expect_code(0).stdout;
        assert!(!out.contains("0%"), "{name}: {out}");
        sandbox.cli(&["stop"]).expect_code(0);
    }
}
