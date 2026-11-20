//! Phase 3: monitoring end to end. The daemon reads a synthetic machine
//! (`SENTINEL_DISCOVERY_ROOT`), so every value it reports is known in
//! advance, and the tests never depend on the load of the computer running
//! them.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sentinel_client::MetricsSection;
use sentinel_config::ConfigPatch;
use sentinel_core::{MonitoringCommand, MonitoringState};
use sentinel_telemetry::{
    AlertStatus, CollectorHealth, CollectorKind, HistoryQuery, MonitoringPhase, SamplingProfile,
    SeriesKey, TimeWindow,
};
use sentinel_tests::Sandbox;
use sentinel_tests::hwfixture::{self, HwFixture};

/// The AMD laptop fixture plus the counters monitoring reads, kept
/// advancing by a writer thread at a fixed ratio: every logical CPU is 75 %
/// busy (3 user ticks per idle tick) and wlan0 receives 1000 bytes per
/// step. Whatever the moment of sampling, every delta shows the same ratio.
pub(crate) struct Machine {
    fixture: HwFixture,
    stop: Arc<AtomicBool>,
    writer: Option<std::thread::JoinHandle<()>>,
}

fn atomic_write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    let tmp = path.with_extension("tmp-write");
    std::fs::write(&tmp, content).unwrap();
    std::fs::rename(&tmp, &path).unwrap();
}

fn stat_text(k: u64) -> String {
    let line = |name: &str, scale: u64| {
        format!("{name} {} 0 0 {} 0 0 0 0 0 0\n", 3 * k * scale, k * scale)
    };
    let mut text = line("cpu ", 16);
    for cpu in 0..16 {
        text += &line(&format!("cpu{cpu}"), 1);
    }
    text + "btime 1700000000\nprocesses 100\n"
}

impl Machine {
    pub(crate) fn new() -> Self {
        let fixture = hwfixture::amd_laptop();
        fixture
            .file("proc/stat", stat_text(1))
            .file("proc/loadavg", "0.50 0.40 0.30 2/400 1234\n")
            .file("proc/uptime", "1000.00 3000.00\n")
            .file("proc/diskstats", " 259 0 nvme0n1 100 0 2000 0 50 0 1000 0 0 100 0\n")
            .file("proc/self/mounts", "")
            .file("sys/class/net/wlan0/statistics/rx_bytes", "1000\n")
            .file("sys/class/net/wlan0/statistics/tx_bytes", "100\n")
            .file("proc/4242/stat", "4242 (editor) S 1 4242 4242 0 -1 0 0 0 0 0 50 10 0 0 20 0 4 0 900 2000000 25600 0\n")
            .file("proc/4242/status", "Name:\teditor\nUid:\t1000\t1000\t1000\t1000\n")
            .file("proc/4242/cmdline", b"/usr/bin/editor\0--secret-arg\0".as_slice())
            .file("proc/4242/environ", "API_KEY=do-not-read\0");
        let stop = Arc::new(AtomicBool::new(false));
        let root = fixture.root().to_path_buf();
        let flag = stop.clone();
        let writer = std::thread::spawn(move || {
            let mut k = 1;
            while !flag.load(Ordering::Relaxed) {
                k += 1;
                if root.join("proc/stat").exists() {
                    atomic_write(&root, "proc/stat", &stat_text(k));
                }
                atomic_write(
                    &root,
                    "sys/class/net/wlan0/statistics/rx_bytes",
                    &format!("{}\n", 1000 * k),
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        Self {
            fixture,
            stop,
            writer: Some(writer),
        }
    }

    fn root(&self) -> PathBuf {
        self.fixture.root().to_path_buf()
    }

    pub(crate) fn sandbox(&self) -> Sandbox {
        Sandbox::new().with_env("SENTINEL_DISCOVERY_ROOT", self.root())
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

/// Like `Sandbox::wait_until`, with a longer deadline for sampling.
pub(crate) fn eventually(what: &str, seconds: u64, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out after {seconds} s waiting for: {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(crate) fn performance(sandbox: &Sandbox) {
    let client = sandbox.client();
    client
        .set_sampling(SamplingProfile::Performance, Some(false))
        .unwrap();
}

fn collector_health(sandbox: &Sandbox, kind: CollectorKind) -> CollectorHealth {
    let report = sandbox.client().monitoring_report().unwrap();
    report
        .collectors
        .iter()
        .find(|c| c.kind == kind)
        .map(|c| c.health)
        .unwrap_or(CollectorHealth::Waiting)
}

fn cpu_interval(info: &sentinel_telemetry::SamplingInfo) -> u64 {
    info.intervals_ms
        .iter()
        .find(|(k, _)| *k == CollectorKind::Cpu)
        .unwrap()
        .1
}

// ---------------------------------------------------------------------

#[test]
fn monitoring_reports_the_values_of_the_machine_it_reads() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    performance(&sandbox);
    sandbox.cli(&["monitor", "start"]).expect_code(0);
    let client = sandbox.client();

    eventually("a CPU utilisation value", 10, || {
        client
            .metrics(MetricsSection::Cpu)
            .unwrap()
            .snapshot
            .and_then(|s| s.cpu)
            .is_some_and(|c| c.total_percent.value.is_some())
    });
    let snap = client
        .metrics(MetricsSection::All)
        .unwrap()
        .snapshot
        .unwrap();
    let cpu = snap.cpu.unwrap();
    assert_eq!(cpu.logical_cpus, 16);
    assert_eq!(cpu.total_percent.value, Some(75.0));
    assert!(
        cpu.cores
            .iter()
            .all(|c| c.utilisation_percent.value.is_none_or(|v| v == 75.0))
    );

    // 15 728 640 kB total, 9 000 000 kB available: 42.8 % used.
    let memory = snap.memory.unwrap();
    assert_eq!(memory.total_bytes.value, Some(15_728_640 * 1024));
    assert_eq!(memory.used_percent.value, Some(42.8));

    // The fixture's battery: 38 of 50 Wh, 7.5 W, 50 of 56 Wh design.
    let battery = snap.battery.unwrap();
    assert_eq!(battery.on_ac.value, Some(false));
    let bat = &battery.batteries[0];
    assert_eq!(bat.percent.value, Some(77.0));
    assert_eq!(bat.power_w.value, Some(7.5));
    assert_eq!(bat.health_percent.value, Some(89.3));
    assert_eq!(bat.cycle_count.value, Some(120));
    assert_eq!(
        bat.temperature_c.value, None,
        "the fixture battery has no temperature sensor"
    );

    // Sensors: three hwmon inputs at 42 °C and the ACPI zone at 45 °C.
    let thermal = snap.thermal.unwrap();
    assert_eq!(thermal.temperatures.len(), 4);
    assert!(
        thermal
            .temperatures
            .iter()
            .all(|t| matches!(t.value_c.value, Some(42.0 | 45.0)))
    );
    assert!(
        snap.fans.is_none_or(|f| f.fans.is_empty()),
        "the fixture has no fans"
    );

    // Every collector ran, together, on one schedule.
    let report = client.monitoring_report().unwrap();
    assert_eq!(report.phase, MonitoringPhase::Active);
    assert_eq!(report.sampling.effective, SamplingProfile::Performance);
    for c in &report.collectors {
        assert_ne!(c.health, CollectorHealth::Waiting, "{:?} never ran", c.kind);
        assert_ne!(
            c.health,
            CollectorHealth::Failing,
            "{:?}: {:?}",
            c.kind,
            c.note
        );
    }

    // Processes: CPU needs two samples, so wait for the second one.
    eventually("process list", 10, || {
        client.processes().unwrap().processes.is_some()
    });
    let list = client.processes().unwrap().processes.unwrap();
    let editor = list.processes.iter().find(|p| p.pid == 4242).unwrap();
    assert_eq!(editor.command.as_deref(), Some("editor"));
    assert_eq!(editor.rss_bytes, 25_600 * rustix::param::page_size() as u64);
    let json = sandbox
        .cli(&["metrics", "processes", "--json"])
        .expect_code(0)
        .stdout;
    for secret in ["secret-arg", "API_KEY", "do-not-read"] {
        assert!(!json.contains(secret), "{secret} leaked");
    }

    // Network throughput is a positive rate from the advancing counter.
    eventually("network rate", 10, || {
        client
            .metrics(MetricsSection::Network)
            .unwrap()
            .snapshot
            .and_then(|s| s.network)
            .is_some_and(|n| {
                n.interfaces
                    .iter()
                    .any(|i| i.name == "wlan0" && i.rx_bps.value.is_some_and(|v| v > 0.0))
            })
    });

    // The CLI renders the same numbers.
    sandbox
        .cli(&["metrics", "cpu"])
        .expect_code(0)
        .expect_stdout("75");
    sandbox
        .cli(&["metrics", "memory"])
        .expect_code(0)
        .expect_stdout("42.8");
}

#[test]
fn cli_controls_monitoring_and_metrics_need_it_on() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);

    // Off: no data, a distinct exit code, and nothing invented.
    sandbox
        .cli(&["metrics"])
        .expect_code(13)
        .expect_stdout("no live measurements");
    sandbox
        .cli(&["monitor", "resume"])
        .expect_code(11)
        .expect_stderr("not paused");

    sandbox.cli(&["monitor", "start"]).expect_code(0);
    let status = sandbox
        .cli(&["monitor", "status", "--json"])
        .expect_code(0)
        .json();
    assert_eq!(status["phase"], "active");
    assert_eq!(status["enabled_preference"], true);
    sandbox
        .cli(&["monitor", "profile", "balanced", "--power-aware", "off"])
        .expect_code(0);
    let status = sandbox
        .cli(&["monitor", "status", "--json"])
        .expect_code(0)
        .json();
    assert_eq!(status["sampling"]["effective"], "balanced");
    let intervals = status["sampling"]["intervals_ms"].as_array().unwrap();
    assert!(
        intervals
            .iter()
            .any(|pair| pair[0] == "cpu" && pair[1] == 2000),
        "{intervals:?}"
    );

    sandbox.cli(&["monitor", "pause"]).expect_code(0);
    assert_eq!(
        sandbox.client().status().unwrap().monitoring.state,
        MonitoringState::Paused
    );
    sandbox
        .cli(&["monitor", "status"])
        .expect_code(0)
        .expect_stdout("Monitoring: paused");
    sandbox.cli(&["monitor", "resume"]).expect_code(0);
    sandbox.cli(&["monitor", "stop"]).expect_code(0);
    assert_eq!(
        sandbox.client().status().unwrap().monitoring.state,
        MonitoringState::Stopped
    );
    sandbox.cli(&["metrics", "cpu"]).expect_code(13);
    let status = sandbox
        .cli(&["monitor", "status", "--json"])
        .expect_code(0)
        .json();
    assert_eq!(status["enabled_preference"], false);
}

#[test]
fn monitoring_off_stops_collection_and_the_daemon_goes_quiet() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();
    let pid = client.status().unwrap().pid;
    performance(&sandbox);
    client.monitoring(MonitoringCommand::Start).unwrap();
    eventually("sampling", 10, || {
        client.monitoring_report().unwrap().last_sample_at.is_some()
    });
    assert_eq!(
        sentinel_tests::proc_field(pid, "Threads"),
        "2",
        "one monitor thread while on"
    );

    client.monitoring(MonitoringCommand::Stop).unwrap();
    let report = client.monitoring_report().unwrap();
    assert_eq!(report.phase, MonitoringPhase::Off);
    assert!(report.collectors.is_empty() && report.last_sample_at.is_none());
    assert!(
        client
            .metrics(MetricsSection::All)
            .unwrap()
            .snapshot
            .is_none(),
        "no stale data is served as live"
    );
    // The monitor thread has exited and nothing reads the machine any more.
    eventually("monitor thread exit", 5, || {
        sentinel_tests::proc_field(pid, "Threads") == "1"
    });
    let reads = || {
        let io = std::fs::read_to_string(format!("/proc/{pid}/io")).unwrap();
        io.lines()
            .find(|l| l.starts_with("syscr:"))
            .unwrap()
            .to_string()
    };
    let before = reads();
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(reads(), before, "a stopped monitor must not read anything");
}

#[test]
fn history_is_stored_and_survives_a_daemon_restart() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    performance(&sandbox);
    sandbox.cli(&["monitor", "start"]).expect_code(0);
    let client = sandbox.client();
    eventually("CPU samples", 10, || {
        client
            .metrics(MetricsSection::Cpu)
            .unwrap()
            .snapshot
            .and_then(|s| s.cpu)
            .is_some_and(|c| c.total_percent.value.is_some())
    });
    std::thread::sleep(Duration::from_secs(2));

    // Live history comes from memory at full resolution.
    let query = |window| HistoryQuery {
        series: vec![
            SeriesKey::new("cpu.total").unwrap(),
            SeriesKey::new("mem.used_percent").unwrap(),
        ],
        window,
    };
    let live = client
        .telemetry_history(query(TimeWindow::Minutes5))
        .unwrap();
    let cpu = live.iter().find(|s| s.key.as_str() == "cpu.total").unwrap();
    assert!(
        cpu.points.iter().filter(|p| p.v == Some(75.0)).count() >= 2,
        "{:?}",
        cpu.points
    );
    assert_eq!(cpu.unit, "%");

    // Restart the daemon. Monitoring was turned on with the CLI, so the
    // preference brings it back; stopping flushed the open buckets.
    sandbox.cli(&["stop"]).expect_code(0);
    let db = sandbox.paths().data_dir.join("history.db");
    assert!(db.is_file());
    sandbox.cli(&["start"]).expect_code(0);
    assert_eq!(
        client.status().unwrap().monitoring.state,
        MonitoringState::Running
    );

    let stored = client.telemetry_history(query(TimeWindow::Hour1)).unwrap();
    let mem = stored
        .iter()
        .find(|s| s.key.as_str() == "mem.used_percent")
        .unwrap();
    assert!(
        mem.points.iter().any(|p| p.v == Some(42.8)),
        "stored history: {:?}",
        mem.points
    );
    let cpu = stored
        .iter()
        .find(|s| s.key.as_str() == "cpu.total")
        .unwrap();
    assert!(
        cpu.points.iter().any(|p| p.v == Some(75.0)),
        "stored history: {:?}",
        cpu.points
    );
    // Each series keeps its provenance.
    assert_eq!(cpu.source, "/proc/stat");
    assert_eq!(cpu.quality, Some(sentinel_telemetry::Quality::Calculated));
    assert_eq!(mem.source, "/proc/meminfo");
    assert!(client.monitoring_report().unwrap().history_available);
}

#[test]
fn a_failing_collector_is_isolated_and_recovers() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    performance(&sandbox);
    sandbox.cli(&["monitor", "start"]).expect_code(0);
    eventually("CPU ok", 10, || {
        collector_health(&sandbox, CollectorKind::Cpu) == CollectorHealth::Ok
    });

    std::fs::remove_file(machine.root().join("proc/stat")).unwrap();
    eventually("CPU failing", 10, || {
        collector_health(&sandbox, CollectorKind::Cpu) == CollectorHealth::Failing
    });
    // Everything else continues, and the daemon answers normally.
    assert_eq!(
        collector_health(&sandbox, CollectorKind::Memory),
        CollectorHealth::Ok
    );
    let report = sandbox.client().monitoring_report().unwrap();
    assert_eq!(report.phase, MonitoringPhase::Active);
    let cpu = report
        .collectors
        .iter()
        .find(|c| c.kind == CollectorKind::Cpu)
        .unwrap();
    assert!(
        cpu.note
            .as_deref()
            .unwrap_or_default()
            .contains("/proc/stat")
    );
    sandbox
        .cli(&["monitor", "status"])
        .expect_code(0)
        .expect_stdout("failing");

    atomic_write(&machine.root(), "proc/stat", &stat_text(1_000_000));
    eventually("CPU recovered after backoff", 15, || {
        collector_health(&sandbox, CollectorKind::Cpu) == CollectorHealth::Ok
    });
}

#[test]
fn power_aware_sampling_follows_the_power_source() {
    let machine = Machine::new();
    machine
        .fixture
        .file("sys/class/power_supply/AC/online", "1\n");
    machine
        .fixture
        .file("sys/class/power_supply/BAT0/status", "Charging\n");
    let sandbox = machine.sandbox();
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();
    client
        .set_sampling(SamplingProfile::Performance, Some(true))
        .unwrap();
    client.monitoring(MonitoringCommand::Start).unwrap();
    eventually("first battery sample", 10, || {
        client.sampling().unwrap().effective == SamplingProfile::Performance
    });

    // Unplugged: the next battery sample (every 10 s in Performance)
    // switches to Eco and says why.
    machine
        .fixture
        .file("sys/class/power_supply/AC/online", "0\n");
    machine
        .fixture
        .file("sys/class/power_supply/BAT0/status", "Discharging\n");
    eventually("switch to Eco", 15, || {
        client.sampling().unwrap().effective == SamplingProfile::Eco
    });
    let info = client.sampling().unwrap();
    assert_eq!(info.selected, SamplingProfile::Performance);
    assert!(info.reason.as_deref().unwrap().contains("battery"));
    assert_eq!(cpu_interval(&info), 5000);

    // Turning power-aware sampling off restores the chosen profile at once.
    let info = client
        .set_sampling(SamplingProfile::Performance, Some(false))
        .unwrap();
    assert_eq!(info.effective, SamplingProfile::Performance);
    assert_eq!(cpu_interval(&info), 1000);
}

// ------------------------------------------------- alerts and notifications

/// A stand-in session bus at `$XDG_RUNTIME_DIR/bus`: it authenticates the
/// client, answers every method call with success and keeps what it was
/// sent. Nothing reaches the real desktop.
struct FakeBus {
    received: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
}

fn method_return(reply_serial: u32) -> Vec<u8> {
    // Little-endian METHOD_RETURN, no body, header field REPLY_SERIAL (5).
    let mut m = vec![b'l', 2, 1, 1];
    m.extend_from_slice(&0u32.to_le_bytes()); // body length
    m.extend_from_slice(&(1000 + reply_serial).to_le_bytes()); // serial
    let mut fields = vec![5u8, 1, b'u', 0];
    fields.extend_from_slice(&reply_serial.to_le_bytes());
    m.extend_from_slice(&(fields.len() as u32).to_le_bytes());
    m.extend_from_slice(&fields);
    while !m.len().is_multiple_of(8) {
        m.push(0);
    }
    m
}

impl FakeBus {
    fn start(path: &Path) -> Self {
        let listener = UnixListener::bind(path).unwrap();
        let received: Arc<std::sync::Mutex<Vec<Vec<u8>>>> = Default::default();
        let store = received.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let store = store.clone();
                std::thread::spawn(move || {
                    let _ = (|| -> std::io::Result<()> {
                        let mut reader = BufReader::new(stream.try_clone()?);
                        let mut writer = stream;
                        let mut line = Vec::new();
                        reader.read_until(b'\n', &mut line)?; // \0AUTH EXTERNAL <uid>
                        writer.write_all(b"OK 0123456789abcdef0123456789abcdef\r\n")?;
                        line.clear();
                        reader.read_until(b'\n', &mut line)?; // BEGIN
                        loop {
                            let mut head = [0u8; 16];
                            reader.read_exact(&mut head)?;
                            let body = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
                            let serial = u32::from_le_bytes(head[8..12].try_into().unwrap());
                            let fields =
                                u32::from_le_bytes(head[12..16].try_into().unwrap()) as usize;
                            let mut rest = vec![0u8; (16 + fields).div_ceil(8) * 8 - 16 + body];
                            reader.read_exact(&mut rest)?;
                            store
                                .lock()
                                .unwrap()
                                .push([head.as_slice(), &rest].concat());
                            writer.write_all(&method_return(serial))?;
                        }
                    })();
                });
            }
        });
        Self { received }
    }

    fn notifications(&self) -> Vec<String> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .map(|m| String::from_utf8_lossy(m).into_owned())
            .filter(|m| m.contains("org.freedesktop.Notifications") && m.contains("Notify"))
            .collect()
    }
}

fn set_available_memory(machine: &Machine, available_kib: u64) {
    machine
        .fixture
        .meminfo(15_728_640, available_kib, 4_194_304);
}

#[test]
fn sustained_low_memory_raises_one_alert_notifies_and_resolves() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    let bus = FakeBus::start(&sandbox.root().join("run/bus"));
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();
    performance(&sandbox);
    // No sustain time, so the test does not wait a minute; everything else
    // (hysteresis band, cooldown) at the defaults.
    let mut thresholds = client.configuration().unwrap().config.alerts;
    thresholds.sustain_s = 0;
    client
        .update_configuration(ConfigPatch {
            alerts: Some(thresholds),
            notifications_enabled: Some(true),
            ..Default::default()
        })
        .unwrap();

    // 3 % available: below the 5 % critical threshold.
    set_available_memory(&machine, 470_000);
    client.monitoring(MonitoringCommand::Start).unwrap();
    eventually("memory alert", 10, || {
        client
            .active_alerts()
            .unwrap()
            .iter()
            .any(|a| a.rule == "memory_low")
    });
    let alert = client
        .active_alerts()
        .unwrap()
        .into_iter()
        .find(|a| a.rule == "memory_low")
        .unwrap();
    assert!(
        alert.evidence.contains("3.0% of memory available"),
        "{}",
        alert.evidence
    );
    assert!(alert.recommendation.is_some());

    // Delivered once to the (fake) desktop, with the alert's title.
    eventually("notification", 10, || !bus.notifications().is_empty());
    std::thread::sleep(Duration::from_secs(2));
    let sent = bus.notifications();
    assert_eq!(sent.len(), 1, "one notification for one condition");
    assert!(sent[0].contains("Low available memory"));
    let report = client.monitoring_report().unwrap();
    assert_eq!(
        (report.notifications.delivered, report.notifications.failed),
        (1, 0)
    );
    assert_eq!(report.notifications.last_delivery_ok, Some(true));
    assert_eq!(
        client
            .active_alerts()
            .unwrap()
            .iter()
            .filter(|a| a.rule == "memory_low")
            .count(),
        1
    );

    // Acknowledging changes only Sentinel's record.
    sandbox
        .cli(&["alerts", "ack", &alert.id.to_string()])
        .expect_code(0);
    assert!(
        client
            .active_alerts()
            .unwrap()
            .iter()
            .find(|a| a.id == alert.id)
            .unwrap()
            .acknowledged
    );
    // ... and it sticks: later samples do not reset it, and it is stored.
    std::thread::sleep(Duration::from_secs(3));
    let acknowledged = |alerts: Vec<sentinel_telemetry::Alert>| {
        alerts.iter().any(|a| a.id == alert.id && a.acknowledged)
    };
    assert!(acknowledged(client.active_alerts().unwrap()));
    assert!(acknowledged(client.alert_history(50).unwrap()));

    // Inside the hysteresis band (12 %: above 10 %, below 15 %): still active.
    set_available_memory(&machine, 1_900_000);
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        client
            .active_alerts()
            .unwrap()
            .iter()
            .any(|a| a.id == alert.id),
        "the band must not clear it"
    );

    // Recovered (57 %): resolved once it has stayed clear for 30 s, and
    // kept in the history.
    set_available_memory(&machine, 9_000_000);
    let recovered = Instant::now();
    eventually("resolution", 45, || {
        client
            .active_alerts()
            .unwrap()
            .iter()
            .all(|a| a.id != alert.id)
    });
    assert!(
        recovered.elapsed() >= Duration::from_secs(29),
        "cleared before the 30 s clear time"
    );
    let history = client.alert_history(50).unwrap();
    let resolved = history.iter().find(|a| a.id == alert.id).unwrap();
    assert_eq!(resolved.status, AlertStatus::Resolved);
    assert!(resolved.resolved_at.is_some());
    sandbox
        .cli(&["alerts", "--history"])
        .expect_code(0)
        .expect_stdout("Low available memory");
    assert_eq!(bus.notifications().len(), 1, "recovery does not notify");
}

#[test]
fn a_missing_notification_service_is_recorded_and_monitoring_continues() {
    let machine = Machine::new();
    let sandbox = machine.sandbox(); // no bus in the sandbox
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();
    performance(&sandbox);
    let mut thresholds = client.configuration().unwrap().config.alerts;
    thresholds.sustain_s = 0;
    client
        .update_configuration(ConfigPatch {
            alerts: Some(thresholds),
            notifications_enabled: Some(true),
            ..Default::default()
        })
        .unwrap();
    set_available_memory(&machine, 470_000);
    client.monitoring(MonitoringCommand::Start).unwrap();
    eventually("failed delivery recorded", 10, || {
        client.monitoring_report().unwrap().notifications.failed >= 1
    });
    let report = client.monitoring_report().unwrap();
    assert_eq!(report.notifications.last_delivery_ok, Some(false));
    assert!(report.notifications.last_error.is_some());
    assert_eq!(report.phase, MonitoringPhase::Active);
    assert!(
        client
            .active_alerts()
            .unwrap()
            .iter()
            .any(|a| a.rule == "memory_low"),
        "the alert is still recorded"
    );
}

#[test]
fn disabled_notifications_are_never_sent() {
    let machine = Machine::new();
    let sandbox = machine.sandbox();
    let bus = FakeBus::start(&sandbox.root().join("run/bus"));
    sandbox.cli(&["start"]).expect_code(0);
    let client = sandbox.client();
    performance(&sandbox);
    let mut thresholds = client.configuration().unwrap().config.alerts;
    thresholds.sustain_s = 0;
    client
        .update_configuration(ConfigPatch {
            alerts: Some(thresholds),
            notifications_enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
    set_available_memory(&machine, 470_000);
    client.monitoring(MonitoringCommand::Start).unwrap();
    eventually("memory alert", 10, || {
        client
            .active_alerts()
            .unwrap()
            .iter()
            .any(|a| a.rule == "memory_low")
    });
    std::thread::sleep(Duration::from_secs(2));
    assert!(bus.notifications().is_empty());
}
