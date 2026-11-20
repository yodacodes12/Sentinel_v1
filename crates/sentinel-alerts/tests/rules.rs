//! Every alert rule, driven through the real engine with synthetic
//! snapshots: when it raises, at which severity, that it raises once, holds
//! in its hysteresis band, resolves only after its clear time, honours the
//! cooldown, ignores brief spikes, and is not moved by missing data.
//!
//! No real hardware condition is created: the snapshots are built here.

use sentinel_alerts::{AlertEngine, AlertEvent};
use sentinel_config::AlertThresholds;
use sentinel_telemetry::model::*;

const STEP: u64 = 1_000;

fn ok(v: f64) -> Reading<f64> {
    Reading::direct(v)
}
fn gone<T>() -> Reading<T> {
    Reading::missing(ReadingState::Unavailable)
}

// ---------------------------------------------------------------- builders

fn cpu(total: Reading<f64>, logical: u32) -> CpuSample {
    CpuSample {
        measured_at: 0,
        interval_ms: 2_000,
        logical_cpus: logical,
        total_percent: total,
        times: CpuTimes {
            user_percent: gone(),
            nice_percent: gone(),
            system_percent: gone(),
            idle_percent: gone(),
            iowait_percent: gone(),
            irq_percent: gone(),
            softirq_percent: gone(),
            steal_percent: gone(),
        },
        cores: Vec::new(),
        average_frequency_khz: gone(),
    }
}

fn memory(
    available_pct: Reading<f64>,
    psi60: Reading<f64>,
    swap_pages: Reading<f64>,
) -> MemorySample {
    const TOTAL: u64 = 8 << 30;
    let available = available_pct
        .value()
        .map(|p| Reading::direct((p / 100.0 * TOTAL as f64) as u64))
        .unwrap_or_else(gone);
    MemorySample {
        measured_at: 0,
        interval_ms: 2_000,
        total_bytes: Reading::direct(TOTAL),
        available_bytes: available,
        used_bytes: gone(),
        used_percent: gone(),
        free_bytes: gone(),
        buffers_bytes: gone(),
        cached_bytes: gone(),
        active_bytes: gone(),
        inactive_bytes: gone(),
        swap_total_bytes: gone(),
        swap_used_bytes: gone(),
        swap_free_bytes: gone(),
        swap_in_pages_per_s: swap_pages.value().map(|v| ok(v / 2.0)).unwrap_or_else(gone),
        swap_out_pages_per_s: swap_pages.value().map(|v| ok(v / 2.0)).unwrap_or_else(gone),
        pressure: Pressure {
            some_avg60: psi60,
            ..Pressure::missing(ReadingState::Unavailable)
        },
    }
}

fn load(load1: Reading<f64>) -> SystemLoad {
    SystemLoad {
        measured_at: 0,
        interval_ms: 2_000,
        load1,
        load5: gone(),
        load15: gone(),
        runnable_tasks: gone(),
        total_tasks: gone(),
        uptime_s: gone(),
        cpu_pressure: Pressure::missing(ReadingState::Unavailable),
        io_pressure: Pressure::missing(ReadingState::Unavailable),
    }
}

fn sensor(
    id: &str,
    value: Reading<f64>,
    max: Option<f64>,
    crit: Option<f64>,
    component: Component,
) -> TemperatureReading {
    TemperatureReading {
        id: id.into(),
        chip: "k10temp".into(),
        label: Some("Tctl".into()),
        value_c: value,
        max_c: max,
        critical_c: crit,
        association: Association {
            component,
            confidence: AssociationConfidence::High,
            basis: "test".into(),
        },
        source: "test".into(),
    }
}

fn thermal(sensors: Vec<TemperatureReading>) -> ThermalReport {
    ThermalReport {
        measured_at: 0,
        interval_ms: 5_000,
        temperatures: sensors,
    }
}

const BATTERY_INTERVAL_MS: u64 = 15_000;

struct Bat {
    state: ChargeState,
    percent: Reading<f64>,
    health: Reading<f64>,
    temp: Reading<f64>,
    power: Reading<f64>,
}

impl Default for Bat {
    fn default() -> Self {
        Self {
            state: ChargeState::Discharging,
            percent: ok(80.0),
            health: ok(90.0),
            temp: gone(),
            power: ok(8.0),
        }
    }
}

fn battery(b: Bat) -> BatteryReport {
    BatteryReport {
        measured_at: 0,
        interval_ms: BATTERY_INTERVAL_MS,
        batteries: vec![BatterySample {
            measured_at: 0,
            interval_ms: BATTERY_INTERVAL_MS,
            name: "BAT0".into(),
            state: Reading::direct(b.state),
            percent: b.percent,
            energy_now_wh: gone(),
            energy_full_wh: gone(),
            energy_design_wh: gone(),
            charge_now_ah: gone(),
            charge_full_ah: gone(),
            charge_design_ah: gone(),
            voltage_v: gone(),
            current_a: gone(),
            power_w: b.power,
            cycle_count: gone(),
            health_percent: b.health,
            temperature_c: b.temp,
            runtime: RuntimeEstimate::NotDischarging,
            technology: None,
            manufacturer: None,
            model: None,
        }],
        sources: Vec::new(),
        on_ac: Reading::direct(false),
    }
}

fn filesystem(mount: &str, free_pct: Reading<f64>, read_only: bool) -> FilesystemUsage {
    FilesystemUsage {
        mount_point: mount.into(),
        device: "/dev/nvme0n1p2".into(),
        fs_type: "ext4".into(),
        total_bytes: gone(),
        used_bytes: gone(),
        available_bytes: gone(),
        used_percent: free_pct.value().map(|f| ok(100.0 - f)).unwrap_or_else(gone),
        read_only,
    }
}

fn disk(busy: Reading<f64>) -> DiskIo {
    DiskIo {
        name: "nvme0n1".into(),
        read_bps: gone(),
        write_bps: gone(),
        reads_per_s: gone(),
        writes_per_s: gone(),
        busy_percent: busy,
        read_total_bytes: 0,
        written_total_bytes: 0,
    }
}

fn storage(filesystems: Vec<FilesystemUsage>, devices: Vec<DiskIo>) -> StorageReport {
    StorageReport {
        measured_at: 0,
        interval_ms: 2_000,
        devices,
        filesystems,
    }
}

fn iface(name: &str, mbps: Reading<f64>, loopback: bool) -> InterfaceTraffic {
    let half_bytes = mbps.value().map(|m| ok(m * 1e6 / 8.0 / 2.0));
    InterfaceTraffic {
        name: name.into(),
        operational_state: Some("up".into()),
        loopback,
        rx_bps: half_bytes.unwrap_or_else(gone),
        tx_bps: half_bytes.unwrap_or_else(gone),
        rx_packets_per_s: gone(),
        tx_packets_per_s: gone(),
        rx_total_bytes: 0,
        tx_total_bytes: 0,
        rx_errors: 0,
        tx_errors: 0,
        rx_dropped: 0,
        tx_dropped: 0,
    }
}

fn network(interfaces: Vec<InterfaceTraffic>) -> NetworkReport {
    NetworkReport {
        measured_at: 0,
        interval_ms: 2_000,
        interfaces,
    }
}

fn snap(f: impl FnOnce(&mut TelemetrySnapshot)) -> TelemetrySnapshot {
    let mut s = TelemetrySnapshot::empty();
    f(&mut s);
    s
}

// ------------------------------------------------------------ the scenario

/// One rule and how to drive it. `make(None)` is a snapshot whose reading
/// for this rule is missing.
struct Spec {
    rule: &'static str,
    thresholds: AlertThresholds,
    make: Box<dyn Fn(Option<f64>) -> TelemetrySnapshot>,
    trigger: f64,
    /// Inside the hysteresis band: neither raising nor clearing.
    band: f64,
    clear: f64,
    /// Raises at critical severity, when the rule has one.
    critical: Option<f64>,
    hold_ms: u64,
    /// The hold at critical severity, when it differs.
    critical_hold_ms: Option<u64>,
    clear_ms: u64,
    severity: AlertSeverity,
}

struct Run {
    engine: AlertEngine,
    t: u64,
}

impl Run {
    fn new(spec: &Spec) -> Self {
        Self {
            engine: AlertEngine::new(spec.thresholds.clone(), 1),
            t: 1_000_000,
        }
    }

    /// Evaluate `snapshot` every second for `ms`, returning this rule's
    /// events. The first evaluation happens at the current time.
    fn feed(&mut self, spec: &Spec, value: Option<f64>, ms: u64) -> Vec<AlertEvent> {
        let snapshot = (spec.make)(value);
        let mut out = Vec::new();
        let end = self.t + ms;
        while self.t < end {
            out.extend(
                self.engine
                    .evaluate(&snapshot, self.t)
                    .into_iter()
                    .filter(|e| e.alert().rule == spec.rule),
            );
            self.t += STEP;
        }
        out
    }

    fn active(&self, spec: &Spec) -> Vec<Alert> {
        self.engine
            .evaluator
            .active()
            .into_iter()
            .filter(|a| a.rule == spec.rule)
            .collect()
    }
}

fn raised(events: &[AlertEvent]) -> Vec<(&Alert, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            AlertEvent::Raised { alert, notify } => Some((alert, *notify)),
            _ => None,
        })
        .collect()
}

fn resolved(events: &[AlertEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, AlertEvent::Resolved { .. }))
        .count()
}

fn check(spec: Spec) {
    let r = spec.rule;
    // Evaluations happen every second; a hold of h ms is satisfied at the
    // evaluation h ms after the first one.
    let before_hold = spec.hold_ms.saturating_sub(STEP);

    // 1. Held just short of the duration, then gone: a spike, not an alert.
    let mut run = Run::new(&spec);
    if spec.hold_ms > 0 {
        let ev = run.feed(&spec, Some(spec.trigger), before_hold.max(STEP));
        assert!(raised(&ev).is_empty(), "{r}: raised before its hold");
        let ev = run.feed(&spec, Some(spec.clear), 5 * STEP);
        assert!(ev.is_empty(), "{r}: a spike produced {ev:?}");
        assert!(run.active(&spec).is_empty(), "{r}: spike left an alert");
    }

    // 2. Sustained: raised once, notified, at the rule's severity.
    let mut run = Run::new(&spec);
    let ev = run.feed(&spec, Some(spec.trigger), spec.hold_ms + STEP);
    let up = raised(&ev);
    assert_eq!(up.len(), 1, "{r}: expected one raise, got {ev:?}");
    let (alert, notify) = up[0];
    assert!(notify, "{r}: the first raise must notify");
    assert_eq!(alert.severity, spec.severity, "{r}: severity");
    assert!(
        !alert.title.is_empty() && !alert.explanation.is_empty(),
        "{r}: wording"
    );
    assert!(
        !alert.threshold.is_empty() && !alert.evidence.is_empty(),
        "{r}: evidence"
    );

    // 3. Continuing condition: no duplicate, still one active alert.
    let ev = run.feed(&spec, Some(spec.trigger), 30 * STEP);
    assert!(ev.is_empty(), "{r}: duplicate events {ev:?}");
    assert_eq!(run.active(&spec).len(), 1, "{r}: one alert per condition");

    // 4. Missing data decides nothing.
    let ev = run.feed(&spec, None, 10 * STEP);
    assert!(ev.is_empty(), "{r}: missing data produced {ev:?}");
    assert_eq!(
        run.active(&spec).len(),
        1,
        "{r}: missing data cleared the alert"
    );

    // 5. In the band: stays active, no flapping.
    let ev = run.feed(&spec, Some(spec.band), 120 * STEP);
    assert!(ev.is_empty(), "{r}: the band produced {ev:?}");
    assert_eq!(
        run.active(&spec).len(),
        1,
        "{r}: the band cleared the alert"
    );

    // 6. Clear: resolved only after the clear time.
    if spec.clear_ms > 0 {
        let ev = run.feed(&spec, Some(spec.clear), spec.clear_ms);
        assert!(ev.is_empty(), "{r}: resolved before its clear time: {ev:?}");
    }
    let ev = run.feed(&spec, Some(spec.clear), STEP);
    assert_eq!(resolved(&ev), 1, "{r}: expected resolution, got {ev:?}");
    assert!(
        run.active(&spec).is_empty(),
        "{r}: still active after resolution"
    );
    let past = run.engine.evaluator.recently_resolved();
    let last = past.iter().find(|a| a.rule == r).expect("kept in history");
    assert_eq!(last.status, AlertStatus::Resolved);
    assert!(last.resolved_at.is_some());

    // 7. Back within the cooldown: recorded, not notified again.
    let ev = run.feed(&spec, Some(spec.trigger), spec.hold_ms + STEP);
    let up = raised(&ev);
    assert_eq!(up.len(), 1, "{r}: a repeated crossing is recorded: {ev:?}");
    assert!(!up[0].1, "{r}: notified again inside the cooldown");

    // 8. Critical severity, where the rule has one.
    if let Some(critical) = spec.critical {
        let mut run = Run::new(&spec);
        let hold = spec.critical_hold_ms.unwrap_or(spec.hold_ms);
        let ev = run.feed(&spec, Some(critical), hold + STEP);
        let up = raised(&ev);
        assert_eq!(up.len(), 1, "{r}: critical raise: {ev:?}");
        assert_eq!(up[0].0.severity, AlertSeverity::Critical, "{r}: critical");

        // A warning that worsens is escalated and notified again.
        let mut run = Run::new(&spec);
        run.feed(&spec, Some(spec.trigger), spec.hold_ms + STEP);
        let ev = run.feed(&spec, Some(critical), STEP);
        assert!(
            ev.iter().any(|e| matches!(e, AlertEvent::Escalated { notify: true, alert } if alert.severity == AlertSeverity::Critical)),
            "{r}: escalation: {ev:?}"
        );
        assert_eq!(
            run.active(&spec).len(),
            1,
            "{r}: escalation is the same alert"
        );
    }
}

fn defaults() -> AlertThresholds {
    AlertThresholds::default()
}

// ------------------------------------------------------------- the rules

#[test]
fn cpu_high() {
    check(Spec {
        rule: "cpu_high",
        thresholds: defaults(),
        make: Box::new(|v| snap(|s| s.cpu = Some(cpu(v.map(ok).unwrap_or_else(gone), 8)))),
        trigger: 95.0,
        band: 80.0,
        clear: 70.0,
        critical: None,
        hold_ms: 120_000,
        critical_hold_ms: None,
        clear_ms: 30_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn load_high() {
    // 2.0 per logical CPU on 4 CPUs triggers; below 1.5 per CPU clears.
    check(Spec {
        rule: "load_high",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| {
                s.cpu = Some(cpu(ok(10.0), 4));
                s.load = Some(load(v.map(ok).unwrap_or_else(gone)));
            })
        }),
        trigger: 8.0,
        band: 7.0,
        clear: 5.0,
        critical: None,
        hold_ms: 300_000,
        critical_hold_ms: None,
        clear_ms: 60_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn memory_low() {
    check(Spec {
        rule: "memory_low",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| s.memory = Some(memory(v.map(ok).unwrap_or_else(gone), gone(), gone())))
        }),
        trigger: 8.0,
        band: 13.0,
        clear: 20.0,
        critical: Some(3.0),
        hold_ms: 60_000,
        critical_hold_ms: None,
        clear_ms: 30_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn memory_pressure() {
    check(Spec {
        rule: "memory_pressure",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| s.memory = Some(memory(ok(50.0), v.map(ok).unwrap_or_else(gone), gone())))
        }),
        trigger: 25.0,
        band: 15.0,
        clear: 5.0,
        critical: None,
        hold_ms: 60_000,
        critical_hold_ms: None,
        clear_ms: 60_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn swap_activity() {
    check(Spec {
        rule: "swap_activity",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| s.memory = Some(memory(ok(50.0), gone(), v.map(ok).unwrap_or_else(gone))))
        }),
        trigger: 600.0,
        band: 300.0,
        clear: 100.0,
        critical: None,
        hold_ms: 60_000,
        critical_hold_ms: None,
        clear_ms: 60_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn temperature_high_uses_the_drivers_limits() {
    check(Spec {
        rule: "temperature_high",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| {
                s.thermal = Some(thermal(vec![sensor(
                    "hwmon1/temp1",
                    v.map(ok).unwrap_or_else(gone),
                    Some(90.0),
                    Some(100.0),
                    Component::Cpu,
                )]))
            })
        }),
        trigger: 92.0,
        band: 87.0,
        clear: 80.0,
        critical: Some(101.0),
        hold_ms: 60_000,
        critical_hold_ms: Some(10_000),
        clear_ms: 30_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn temperature_high_without_limits_never_alerts_unless_the_user_sets_one() {
    let hot = |user: Option<f64>| {
        let mut e = AlertEngine::new(
            AlertThresholds {
                temperature_warning_c: user,
                ..defaults()
            },
            1,
        );
        let s = snap(|s| {
            s.thermal = Some(thermal(vec![
                sensor("acpitz", ok(99.0), None, None, Component::Unknown),
                // The battery's own sensor belongs to battery_temperature.
                sensor("bat", ok(99.0), Some(50.0), None, Component::Battery),
            ]))
        });
        let mut raised = Vec::new();
        for t in (0..120_000).step_by(1_000) {
            raised.extend(
                e.evaluate(&s, t)
                    .into_iter()
                    .filter(|e| e.alert().rule == "temperature_high"),
            );
        }
        raised
    };
    assert!(
        hot(None).is_empty(),
        "no limit and no user threshold: no alert"
    );
    let ev = hot(Some(70.0));
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!(ev[0].alert().subject.as_deref(), Some("acpitz"));
    assert_eq!(ev[0].alert().threshold_source, ThresholdSource::User);
}

#[test]
fn battery_low() {
    check(Spec {
        rule: "battery_low",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| {
                s.battery = Some(battery(Bat {
                    percent: v.map(ok).unwrap_or_else(gone),
                    ..Default::default()
                }))
            })
        }),
        trigger: 12.0,
        band: 18.0,
        clear: 30.0,
        critical: Some(4.0),
        // Two readings: one battery interval.
        hold_ms: BATTERY_INTERVAL_MS,
        critical_hold_ms: None,
        clear_ms: 0,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn battery_low_needs_two_battery_readings_and_a_discharging_battery() {
    let mut e = AlertEngine::new(defaults(), 1);
    let low = |state| {
        snap(|s| {
            s.battery = Some(battery(Bat {
                state,
                percent: ok(10.0),
                ..Default::default()
            }))
        })
    };
    // One odd reading, evaluated on every pass until the next battery read.
    let mut ev = Vec::new();
    for t in (0..BATTERY_INTERVAL_MS - 1_000).step_by(1_000) {
        ev.extend(e.evaluate(&low(ChargeState::Discharging), t));
    }
    assert!(ev.iter().all(|e| e.alert().rule != "battery_low"), "{ev:?}");
    // Charging at 10 % is not "low battery".
    let mut e = AlertEngine::new(defaults(), 1);
    for t in (0..60_000).step_by(1_000) {
        assert!(
            e.evaluate(&low(ChargeState::Charging), t)
                .iter()
                .all(|e| e.alert().rule != "battery_low")
        );
    }
}

#[test]
fn battery_health() {
    check(Spec {
        rule: "battery_health",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| {
                s.battery = Some(battery(Bat {
                    health: v.map(ok).unwrap_or_else(gone),
                    ..Default::default()
                }))
            })
        }),
        trigger: 55.0,
        band: 61.0,
        clear: 70.0,
        critical: None,
        hold_ms: 0,
        critical_hold_ms: None,
        clear_ms: 0,
        severity: AlertSeverity::Info,
    });
}

#[test]
fn battery_temperature() {
    check(Spec {
        rule: "battery_temperature",
        thresholds: AlertThresholds {
            battery_temperature_warning_c: Some(45.0),
            ..defaults()
        },
        make: Box::new(|v| {
            snap(|s| {
                s.battery = Some(battery(Bat {
                    temp: v.map(ok).unwrap_or_else(gone),
                    ..Default::default()
                }))
            })
        }),
        trigger: 50.0,
        band: 43.0,
        clear: 40.0,
        critical: None,
        hold_ms: 60_000,
        critical_hold_ms: None,
        clear_ms: 30_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn battery_discharge_high() {
    check(Spec {
        rule: "battery_discharge_high",
        thresholds: AlertThresholds {
            battery_discharge_high_w: Some(20.0),
            ..defaults()
        },
        make: Box::new(|v| {
            snap(|s| {
                s.battery = Some(battery(Bat {
                    power: v.map(ok).unwrap_or_else(gone),
                    ..Default::default()
                }))
            })
        }),
        trigger: 25.0,
        band: 18.0,
        clear: 10.0,
        critical: None,
        hold_ms: 300_000,
        critical_hold_ms: None,
        clear_ms: 60_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn opt_in_rules_are_off_by_default() {
    let mut e = AlertEngine::new(defaults(), 1);
    let s = snap(|s| {
        s.battery = Some(battery(Bat {
            temp: ok(80.0),
            power: ok(500.0),
            ..Default::default()
        }));
        s.network = Some(network(vec![iface("wlan0", ok(10_000.0), false)]));
    });
    for t in (0..600_000).step_by(1_000) {
        for ev in e.evaluate(&s, t) {
            let rule = ev.alert().rule.clone();
            assert!(
                !matches!(
                    rule.as_str(),
                    "battery_temperature" | "battery_discharge_high" | "network_high"
                ),
                "{rule} raised without a user threshold"
            );
        }
    }
}

#[test]
fn storage_low() {
    check(Spec {
        rule: "storage_low",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| {
                s.storage = Some(storage(
                    vec![
                        filesystem("/", v.map(ok).unwrap_or_else(gone), false),
                        // Read-only filesystems (a mounted image, a snap) are
                        // full by nature and never alert.
                        filesystem("/snap/core/1", ok(0.0), true),
                    ],
                    Vec::new(),
                ))
            })
        }),
        trigger: 8.0,
        band: 11.0,
        clear: 20.0,
        critical: Some(3.0),
        hold_ms: 1_000,
        critical_hold_ms: None,
        clear_ms: 0,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn disk_busy() {
    check(Spec {
        rule: "disk_busy",
        thresholds: defaults(),
        make: Box::new(|v| {
            snap(|s| {
                s.storage = Some(storage(
                    Vec::new(),
                    vec![disk(v.map(ok).unwrap_or_else(gone))],
                ))
            })
        }),
        trigger: 95.0,
        band: 75.0,
        clear: 50.0,
        critical: None,
        hold_ms: 300_000,
        critical_hold_ms: None,
        clear_ms: 60_000,
        severity: AlertSeverity::Warning,
    });
}

#[test]
fn network_high() {
    check(Spec {
        rule: "network_high",
        thresholds: AlertThresholds {
            network_high_mbps: Some(100.0),
            ..defaults()
        },
        make: Box::new(|v| {
            snap(|s| {
                s.network = Some(network(vec![
                    iface("wlan0", v.map(ok).unwrap_or_else(gone), false),
                    // Loopback traffic is not network traffic.
                    iface("lo", ok(50_000.0), true),
                ]))
            })
        }),
        trigger: 150.0,
        band: 80.0,
        clear: 50.0,
        critical: None,
        hold_ms: 60_000,
        critical_hold_ms: None,
        clear_ms: 60_000,
        severity: AlertSeverity::Info,
    });
}

#[test]
fn repeated_temperature_warnings_are_summarised_once() {
    // Short cooldown so the sensor can raise again and again.
    let mut e = AlertEngine::new(
        AlertThresholds {
            cooldown_s: 0,
            ..defaults()
        },
        1,
    );
    let at = |v: f64| {
        snap(|s| {
            s.thermal = Some(thermal(vec![sensor(
                "hwmon1/temp1",
                ok(v),
                Some(90.0),
                None,
                Component::Cpu,
            )]))
        })
    };
    let mut t = 0;
    let mut events = Vec::new();
    for _ in 0..4 {
        for _ in 0..62 {
            events.extend(e.evaluate(&at(95.0), t));
            t += 1_000;
        }
        for _ in 0..32 {
            events.extend(e.evaluate(&at(70.0), t));
            t += 1_000;
        }
    }
    let raises = |rule: &str| {
        events
            .iter()
            .filter(|e| matches!(e, AlertEvent::Raised { alert, .. } if alert.rule == rule))
            .count()
    };
    assert_eq!(raises("temperature_high"), 4);
    assert_eq!(raises("thermal_repeated"), 1, "{events:?}");
}

#[test]
fn every_rule_is_covered() {
    // The 13 rules of docs/ALERT_ENGINE.md, each tested above.
    let mut thresholds = defaults();
    thresholds.battery_temperature_warning_c = Some(40.0);
    thresholds.battery_discharge_high_w = Some(10.0);
    thresholds.network_high_mbps = Some(10.0);
    thresholds.temperature_warning_c = Some(50.0);
    let s = snap(|s| {
        s.cpu = Some(cpu(ok(50.0), 4));
        s.load = Some(load(ok(1.0)));
        s.memory = Some(memory(ok(50.0), ok(1.0), ok(0.0)));
        s.thermal = Some(thermal(vec![sensor(
            "t",
            ok(40.0),
            None,
            None,
            Component::Cpu,
        )]));
        s.battery = Some(battery(Bat {
            temp: ok(30.0),
            ..Default::default()
        }));
        s.storage = Some(storage(
            vec![filesystem("/", ok(50.0), false)],
            vec![disk(ok(1.0))],
        ));
        s.network = Some(network(vec![iface("wlan0", ok(1.0), false)]));
    });
    let mut rules: Vec<&str> = sentinel_alerts::rules::observe(&s, &thresholds)
        .iter()
        .map(|o| o.rule)
        .collect();
    rules.sort();
    rules.dedup();
    assert_eq!(
        rules,
        [
            "battery_discharge_high",
            "battery_health",
            "battery_low",
            "battery_temperature",
            "cpu_high",
            "disk_busy",
            "load_high",
            "memory_low",
            "memory_pressure",
            "network_high",
            "storage_low",
            "swap_activity",
            "temperature_high",
        ]
    );
}
