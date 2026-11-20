//! Alert rules: turn the latest telemetry into observations.
//!
//! Every rule uses a sustained duration and a hysteresis band (a lower
//! clear threshold) so brief spikes and values hovering at the threshold do
//! not produce alerts. Temperature rules use only driver-reported limits or
//! a threshold the user set; Sentinel invents no universal "safe"
//! temperature. Recommendations are text only.

use sentinel_config::AlertThresholds;
use sentinel_telemetry::model::{
    AlertCategory, AlertSeverity, ChargeState, Component, TelemetrySnapshot, ThresholdSource,
};

use crate::evaluator::{Level, Observation};

fn base(
    rule: &'static str,
    category: AlertCategory,
    metric: String,
    subject: Option<String>,
) -> Observation {
    Observation {
        rule,
        subject,
        category,
        metric,
        level: Level::Unknown,
        duration_ms: 0,
        clear_after_ms: 0,
        title: String::new(),
        explanation: String::new(),
        recommendation: None,
        threshold: String::new(),
        threshold_source: ThresholdSource::Default,
        evidence: String::new(),
    }
}

/// `value >= trigger` triggers; `value < clear` clears; between is the band.
fn above(value: Option<f64>, trigger: f64, clear: f64, severity: AlertSeverity) -> Level {
    match value {
        None => Level::Unknown,
        Some(v) if v >= trigger => Level::Triggered(severity),
        Some(v) if v < clear => Level::Clear,
        Some(_) => Level::Band,
    }
}

/// Lower is worse: `value <= trigger` triggers.
fn below(value: Option<f64>, warn: f64, critical: f64, clear: f64) -> Level {
    match value {
        None => Level::Unknown,
        Some(v) if v <= critical => Level::Triggered(AlertSeverity::Critical),
        Some(v) if v <= warn => Level::Triggered(AlertSeverity::Warning),
        Some(v) if v > clear => Level::Clear,
        Some(_) => Level::Band,
    }
}

fn mins(seconds: u64) -> String {
    if seconds >= 120 {
        format!("{} minutes", seconds / 60)
    } else {
        format!("{seconds} seconds")
    }
}

pub fn observe(s: &TelemetrySnapshot, t: &AlertThresholds) -> Vec<Observation> {
    let mut out = Vec::new();
    let sustain_ms = u64::from(t.sustain_s) * 1000;

    // ---------------------------------------------------------- CPU
    if let Some(cpu) = &s.cpu {
        let v = cpu.total_percent.value();
        let mut o = base("cpu_high", AlertCategory::Cpu, "cpu.total".into(), None);
        o.level = above(
            v,
            t.cpu_high_percent,
            t.cpu_high_percent - 15.0,
            AlertSeverity::Warning,
        );
        o.duration_ms = u64::from(t.cpu_high_duration_s) * 1000;
        o.clear_after_ms = 30_000;
        o.title = "Sustained high CPU usage".into();
        o.explanation = format!(
            "Total CPU utilisation has stayed at or above {}% for at least {}.",
            t.cpu_high_percent,
            mins(u64::from(t.cpu_high_duration_s))
        );
        o.recommendation = Some(
            "Check which applications are using the most CPU on the Processes page and consider closing ones you do not need."
                .into(),
        );
        o.threshold = format!(
            "≥ {}% for {}",
            t.cpu_high_percent,
            mins(u64::from(t.cpu_high_duration_s))
        );
        o.evidence = v
            .map(|v| format!("Latest total CPU utilisation {v:.0}%."))
            .unwrap_or_default();
        out.push(o);

        if let Some(load) = &s.load
            && cpu.logical_cpus > 0
        {
            let per_cpu = load.load1.value().map(|l| l / f64::from(cpu.logical_cpus));
            let mut o = base("load_high", AlertCategory::Cpu, "load.1".into(), None);
            o.level = above(
                per_cpu,
                t.load_per_cpu_high,
                t.load_per_cpu_high * 0.75,
                AlertSeverity::Warning,
            );
            o.duration_ms = 300_000;
            o.clear_after_ms = 60_000;
            o.title = "Unusually high system load".into();
            o.explanation = format!(
                "The 1-minute load average has stayed above {} per logical CPU for 5 minutes: more work is waiting than the processors can run. A possible cause is many busy processes or processes waiting for disk I/O.",
                t.load_per_cpu_high
            );
            o.recommendation = Some(
                "Review the busiest processes and any large file operations in progress.".into(),
            );
            o.threshold = format!(
                "≥ {} × {} logical CPUs for 5 minutes",
                t.load_per_cpu_high, cpu.logical_cpus
            );
            o.evidence = load
                .load1
                .value()
                .map(|l| format!("Load average {l:.2} on {} logical CPUs.", cpu.logical_cpus))
                .unwrap_or_default();
            out.push(o);
        }
    }

    // ------------------------------------------------------- memory
    if let Some(m) = &s.memory {
        let avail_pct = match (m.available_bytes.value(), m.total_bytes.value()) {
            (Some(a), Some(total)) if total > 0 => Some(a as f64 / total as f64 * 100.0),
            _ => None,
        };
        let mut o = base(
            "memory_low",
            AlertCategory::Memory,
            "mem.available".into(),
            None,
        );
        o.level = below(
            avail_pct,
            t.memory_available_low_percent,
            t.memory_available_critical_percent,
            t.memory_available_low_percent + 5.0,
        );
        o.duration_ms = sustain_ms;
        o.clear_after_ms = 30_000;
        o.title = "Low available memory".into();
        o.explanation = format!(
            "Available memory has remained below {}% of total for at least {}.",
            t.memory_available_low_percent,
            mins(u64::from(t.sustain_s))
        );
        o.recommendation = Some(
            "Review the applications consuming the most memory and consider closing applications you do not currently need."
                .into(),
        );
        o.threshold = format!(
            "≤ {}% available (critical ≤ {}%)",
            t.memory_available_low_percent, t.memory_available_critical_percent
        );
        o.evidence = avail_pct
            .map(|p| format!("{p:.1}% of memory available."))
            .unwrap_or_default();
        out.push(o);

        let psi = m.pressure.some_avg60.value();
        let mut o = base(
            "memory_pressure",
            AlertCategory::Memory,
            "mem.pressure".into(),
            None,
        );
        o.level = above(psi, 20.0, 10.0, AlertSeverity::Warning);
        o.duration_ms = sustain_ms;
        o.clear_after_ms = 60_000;
        o.title = "Sustained memory pressure".into();
        o.explanation = "Tasks have been waiting for memory a significant share of the time (Linux pressure stall information).".into();
        o.recommendation = Some("Review the processes using the most memory.".into());
        o.threshold = "≥ 20% stall time (60-second average)".into();
        o.evidence = psi
            .map(|p| format!("Memory stall time {p:.1}% over the last minute."))
            .unwrap_or_default();
        out.push(o);

        let swap = match (
            m.swap_in_pages_per_s.value(),
            m.swap_out_pages_per_s.value(),
        ) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        let mut o = base(
            "swap_activity",
            AlertCategory::Memory,
            "swap.out".into(),
            None,
        );
        o.level = above(
            swap,
            t.swap_activity_pages_per_s,
            t.swap_activity_pages_per_s / 2.0,
            AlertSeverity::Warning,
        );
        o.duration_ms = sustain_ms;
        o.clear_after_ms = 60_000;
        o.title = "High swap activity".into();
        o.explanation = "The system is moving memory to and from swap at a high rate, which usually slows everything down.".into();
        o.recommendation =
            Some("Closing memory-heavy applications you do not need may reduce swapping.".into());
        o.threshold = format!("≥ {} pages/s", t.swap_activity_pages_per_s);
        o.evidence = swap
            .map(|v| format!("{v:.0} pages/s swapped in and out."))
            .unwrap_or_default();
        out.push(o);
    }

    // ------------------------------------------------------ thermal
    if let Some(th) = &s.thermal {
        for sensor in &th.temperatures {
            // Battery sensors are covered by the battery rule.
            if sensor.association.component == Component::Battery {
                continue;
            }
            let (warn, warn_source) = match (t.temperature_warning_c, sensor.max_c) {
                (Some(user), _) => (Some(user), ThresholdSource::User),
                (None, Some(max)) => (Some(max), ThresholdSource::Driver),
                (None, None) => (None, ThresholdSource::Driver),
            };
            let critical = sensor.critical_c;
            if warn.is_none() && critical.is_none() {
                continue; // no reliable limit: no alert, by design
            }
            let v = sensor.value_c.value();
            let level = match v {
                None => Level::Unknown,
                Some(v) if critical.is_some_and(|c| v >= c) => {
                    Level::Triggered(AlertSeverity::Critical)
                }
                Some(v) if warn.is_some_and(|w| v >= w) => Level::Triggered(AlertSeverity::Warning),
                Some(v) if v < warn.or(critical).unwrap_or(f64::MAX) - 5.0 => Level::Clear,
                Some(_) => Level::Band,
            };
            let name = sensor
                .label
                .clone()
                .map(|l| format!("{} {l}", sensor.chip))
                .unwrap_or_else(|| sensor.chip.clone());
            let mut o = base(
                "temperature_high",
                AlertCategory::Thermal,
                format!("temp.{}", sensor.id),
                Some(sensor.id.clone()),
            );
            o.level = level;
            o.duration_ms = if matches!(level, Level::Triggered(AlertSeverity::Critical)) {
                10_000
            } else {
                sustain_ms
            };
            o.clear_after_ms = 30_000;
            o.title = format!("High temperature: {name}");
            o.explanation = format!(
                "The {name} sensor has stayed above its warning threshold for a sustained period. It measures: {}.",
                sensor.association.basis
            );
            o.recommendation = Some(
                "Check whether demanding applications are running and make sure the ventilation openings are unobstructed.".into(),
            );
            o.threshold = match (warn, critical) {
                (Some(w), Some(c)) => format!("warning {w:.0} °C, critical {c:.0} °C"),
                (Some(w), None) => format!("warning {w:.0} °C"),
                (None, Some(c)) => format!("critical {c:.0} °C"),
                _ => String::new(),
            };
            o.threshold_source = if warn.is_some() {
                warn_source
            } else {
                ThresholdSource::Driver
            };
            o.evidence = v.map(|v| format!("{name}: {v:.1} °C.")).unwrap_or_default();
            out.push(o);
        }
    }

    // ------------------------------------------------------ battery
    if let Some(report) = &s.battery {
        for b in &report.batteries {
            let discharging = b.state.value() == Some(ChargeState::Discharging);
            let percent = b.percent.value();
            let mut o = base(
                "battery_low",
                AlertCategory::Battery,
                format!("battery.{}.percent", b.name),
                Some(b.name.clone()),
            );
            o.level = match (b.state.value(), percent) {
                (None, _) | (_, None) => Level::Unknown,
                (Some(_), Some(_)) if !discharging => Level::Clear,
                (Some(_), p) => below(
                    p,
                    t.battery_low_percent,
                    t.battery_critical_percent,
                    t.battery_low_percent + 5.0,
                ),
            };
            // Two readings in a row, not one. The battery is read less often
            // than rules are evaluated, so the hold is one battery interval:
            // a single odd reading never raises the alert.
            o.duration_ms = report.interval_ms.max(1_000);
            o.title = "Low battery".into();
            o.explanation = format!(
                "The battery has reached the configured low-battery threshold of {}%.",
                t.battery_low_percent
            );
            o.recommendation = Some("Consider connecting your charger.".into());
            o.threshold = format!(
                "≤ {}% while discharging (critical ≤ {}%)",
                t.battery_low_percent, t.battery_critical_percent
            );
            o.evidence = percent
                .map(|p| format!("{}: {p:.0}%.", b.name))
                .unwrap_or_default();
            out.push(o);

            let health = b.health_percent.value();
            let mut o = base(
                "battery_health",
                AlertCategory::Battery,
                format!("battery.{}.health", b.name),
                Some(b.name.clone()),
            );
            o.level = below(
                health,
                t.battery_health_warning_percent,
                -1.0,
                t.battery_health_warning_percent + 2.0,
            );
            if let Level::Triggered(_) = o.level {
                o.level = Level::Triggered(AlertSeverity::Info);
            }
            o.title = "Battery capacity has decreased".into();
            o.explanation = format!(
                "The battery reports a full-charge capacity below {}% of its design capacity. This estimate is based on the capacities the battery reports, not a laboratory test.",
                t.battery_health_warning_percent
            );
            o.recommendation = Some("Expect shorter battery life. Replacement is a decision for you; Sentinel changes nothing.".into());
            o.threshold = format!("≤ {}% of design capacity", t.battery_health_warning_percent);
            o.threshold_source = ThresholdSource::Default;
            o.evidence = health
                .map(|h| format!("Estimated health {h:.1}%."))
                .unwrap_or_default();
            out.push(o);

            if let Some(limit) = t.battery_temperature_warning_c {
                let temp = b.temperature_c.value();
                let mut o = base(
                    "battery_temperature",
                    AlertCategory::Battery,
                    format!("battery.{}.temp", b.name),
                    Some(b.name.clone()),
                );
                o.level = above(temp, limit, limit - 3.0, AlertSeverity::Warning);
                o.duration_ms = sustain_ms;
                o.clear_after_ms = 30_000;
                o.title = "High battery temperature".into();
                o.explanation =
                    format!("The battery's own temperature sensor has stayed above {limit} °C.");
                o.recommendation =
                    Some("Let the computer cool down and avoid covering it while charging.".into());
                o.threshold = format!("≥ {limit} °C");
                o.threshold_source = ThresholdSource::User;
                o.evidence = temp
                    .map(|v| format!("Battery at {v:.1} °C."))
                    .unwrap_or_default();
                out.push(o);
            }
            if let Some(limit) = t.battery_discharge_high_w {
                let p = if discharging {
                    b.power_w.value()
                } else {
                    Some(0.0)
                };
                let mut o = base(
                    "battery_discharge_high",
                    AlertCategory::Battery,
                    format!("battery.{}.power", b.name),
                    Some(b.name.clone()),
                );
                o.level = above(p, limit, limit * 0.8, AlertSeverity::Warning);
                o.duration_ms = 300_000;
                o.clear_after_ms = 60_000;
                o.title = "Unusually high battery drain".into();
                o.explanation = format!(
                    "The battery has been discharging at more than {limit} W for 5 minutes."
                );
                o.recommendation = Some("Check which applications are busy; a possible cause is a background task or high screen brightness.".into());
                o.threshold = format!("≥ {limit} W for 5 minutes");
                o.threshold_source = ThresholdSource::User;
                o.evidence = b
                    .power_w
                    .value()
                    .map(|v| format!("Discharging at {v:.1} W."))
                    .unwrap_or_default();
                out.push(o);
            }
        }
    }

    // ------------------------------------------------------ storage
    if let Some(st) = &s.storage {
        for fs in st.filesystems.iter().filter(|f| !f.read_only) {
            let free = fs.used_percent.value().map(|u| 100.0 - u);
            let mut o = base(
                "storage_low",
                AlertCategory::Storage,
                "fs.used_percent".into(),
                Some(fs.mount_point.clone()),
            );
            o.level = below(
                free,
                t.storage_free_low_percent,
                t.storage_free_critical_percent,
                t.storage_free_low_percent + 2.0,
            );
            o.duration_ms = 1_000;
            o.title = format!("Low disk space on {}", fs.mount_point);
            o.explanation = format!(
                "Free space on {} is below {}%.",
                fs.mount_point, t.storage_free_low_percent
            );
            o.recommendation = Some("Consider removing files you no longer need or moving them elsewhere. Sentinel deletes nothing.".into());
            o.threshold = format!(
                "≤ {}% free (critical ≤ {}%)",
                t.storage_free_low_percent, t.storage_free_critical_percent
            );
            o.evidence = free
                .map(|f| format!("{f:.1}% free on {} ({}).", fs.mount_point, fs.device))
                .unwrap_or_default();
            out.push(o);
        }
        for d in &st.devices {
            let busy = d.busy_percent.value();
            let mut o = base(
                "disk_busy",
                AlertCategory::Storage,
                format!("disk.{}.busy", d.name),
                Some(d.name.clone()),
            );
            o.level = above(
                busy,
                t.disk_busy_high_percent,
                t.disk_busy_high_percent - 20.0,
                AlertSeverity::Warning,
            );
            o.duration_ms = 300_000;
            o.clear_after_ms = 60_000;
            o.title = format!("Sustained high I/O on {}", d.name);
            o.explanation = format!(
                "{} has been busy more than {}% of the time for 5 minutes. This is activity, not a health problem.",
                d.name, t.disk_busy_high_percent
            );
            o.recommendation = Some("Large copies, updates or indexing are possible causes; the system may feel slow until they finish.".into());
            o.threshold = format!("≥ {}% busy for 5 minutes", t.disk_busy_high_percent);
            o.evidence = busy
                .map(|b| format!("{} busy {b:.0}%.", d.name))
                .unwrap_or_default();
            out.push(o);
        }
    }

    // ------------------------------------------------------ network
    if let (Some(limit), Some(net)) = (t.network_high_mbps, &s.network) {
        let ifaces: Vec<_> = net.interfaces.iter().filter(|i| !i.loopback).collect();
        let total = if ifaces.iter().all(|i| i.rx_bps.is_ok() && i.tx_bps.is_ok()) {
            Some(
                ifaces
                    .iter()
                    .map(|i| i.rx_bps.value().unwrap_or(0.0) + i.tx_bps.value().unwrap_or(0.0))
                    .sum::<f64>()
                    * 8.0
                    / 1e6,
            )
        } else {
            None
        };
        let mut o = base(
            "network_high",
            AlertCategory::Network,
            "net.total".into(),
            None,
        );
        o.level = above(total, limit, limit * 0.75, AlertSeverity::Info);
        o.duration_ms = sustain_ms;
        o.clear_after_ms = 60_000;
        o.title = "High network traffic".into();
        o.explanation =
            format!("Total network traffic has stayed above your threshold of {limit} Mbit/s.");
        o.recommendation =
            Some("Large downloads, uploads or synchronisation are possible causes.".into());
        o.threshold = format!("≥ {limit} Mbit/s");
        o.threshold_source = ThresholdSource::User;
        o.evidence = total
            .map(|v| format!("{v:.1} Mbit/s in total."))
            .unwrap_or_default();
        out.push(o);
    }
    out
}
