//! `sentinel monitor`, `sentinel metrics` and `sentinel alerts`.

use std::process::ExitCode;

use sentinel_client::{MetricsSection, Sentinel};
use sentinel_core::{MonitoringCommand, Result, SentinelError};
use sentinel_telemetry::model::*;

use crate::inventory::{bytes, json};

/// Exit code when live metrics were asked for but monitoring is off.
pub const EXIT_MONITORING_OFF: u8 = 13;

fn clock(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02} UTC")
}

fn r<T: std::fmt::Display>(reading: &Reading<T>, unit: &str) -> String {
    match &reading.value {
        Some(v) => {
            let mark = match reading.quality {
                Quality::Estimated => " (estimate)",
                _ => "",
            };
            format!("{v}{unit}{mark}")
        }
        None => match reading.state {
            ReadingState::Pending => "measuring…".into(),
            ReadingState::PermissionDenied => "not available (administrator only)".into(),
            ReadingState::Unsupported => "not provided by this system".into(),
            ReadingState::Error => "read error".into(),
            _ => "not available".into(),
        },
    }
}

fn rate(reading: &Reading<f64>) -> String {
    match reading.value {
        Some(v) => format!("{}/s", bytes(v as u64)),
        None => r(reading, ""),
    }
}

fn phase_text(p: MonitoringPhase) -> &'static str {
    match p {
        MonitoringPhase::Off => "off",
        MonitoringPhase::Starting => "starting",
        MonitoringPhase::Active => "active",
        MonitoringPhase::Paused => "paused",
        MonitoringPhase::Stopping => "stopping",
        MonitoringPhase::Error => "error",
    }
}

pub fn render_report(m: &MonitoringReport) -> String {
    let mut out = format!("Monitoring: {}\n", phase_text(m.phase));
    if let Some(e) = &m.error {
        out.push_str(&format!("  Error: {e}\n"));
    }
    out.push_str(&format!(
        "  Sampling: {:?} selected, {:?} in use{}\n",
        m.sampling.selected,
        m.sampling.effective,
        if m.sampling.power_aware {
            " (power-aware sampling on)"
        } else {
            ""
        }
    ));
    if let Some(reason) = &m.sampling.reason {
        out.push_str(&format!("  {reason}\n"));
    }
    out.push_str(&format!(
        "  Preference: monitoring {} at daemon start\n",
        if m.enabled_preference {
            "resumes"
        } else {
            "stays off"
        }
    ));
    if let Some(t) = m.last_sample_at {
        out.push_str(&format!("  Last sample: {}\n", clock(t)));
    }
    out.push_str(&format!(
        "  Notifications: {}{}\n",
        if m.notifications.enabled { "on" } else { "off" },
        match (
            &m.notifications.last_delivery_ok,
            &m.notifications.last_error
        ) {
            (Some(false), Some(e)) => format!(" (last delivery failed: {e})"),
            _ => format!(" ({} delivered)", m.notifications.delivered),
        }
    ));
    if !m.collectors.is_empty() {
        out.push_str("  Collectors:\n");
        for c in &m.collectors {
            out.push_str(&format!(
                "    {:<14} {:<12} every {:>5.1} s{}\n",
                c.kind.label(),
                format!("{:?}", c.health).to_lowercase(),
                c.interval_ms as f64 / 1000.0,
                c.note
                    .as_ref()
                    .map(|n| format!("  ({n})"))
                    .unwrap_or_default()
            ));
        }
    }
    out
}

pub fn control(sentinel: &Sentinel, command: MonitoringCommand) -> Result<ExitCode> {
    let status = sentinel.monitoring(command)?;
    println!("{}", status.note);
    Ok(ExitCode::SUCCESS)
}

pub fn status(sentinel: &Sentinel, as_json: bool) -> Result<ExitCode> {
    let report = sentinel.monitoring_report()?;
    if as_json {
        json(&report)?;
    } else {
        print!("{}", render_report(&report));
    }
    Ok(ExitCode::SUCCESS)
}

pub fn profile(
    sentinel: &Sentinel,
    profile: SamplingProfile,
    power_aware: Option<bool>,
) -> Result<ExitCode> {
    let info = sentinel.set_sampling(profile, power_aware)?;
    println!(
        "Sampling profile set to {:?}{}.",
        info.selected,
        if info.power_aware {
            " with power-aware sampling"
        } else {
            ""
        }
    );
    Ok(ExitCode::SUCCESS)
}

pub fn render_snapshot(s: &TelemetrySnapshot) -> String {
    let mut out = String::new();
    let section = |out: &mut String, title: &str, at: u64| {
        out.push_str(&format!("\n{title}  (measured {})\n", clock(at)))
    };
    if let Some(c) = &s.cpu {
        section(&mut out, "CPU", c.measured_at);
        out.push_str(&format!(
            "  Total {}   iowait {}   avg frequency {}\n",
            r(&c.total_percent, "%"),
            r(&c.times.iowait_percent, "%"),
            r(&c.average_frequency_khz.map(|k| k / 1000), " MHz")
        ));
        let cores: Vec<String> = c
            .cores
            .iter()
            .map(|k| format!("cpu{} {}", k.cpu, r(&k.utilisation_percent, "%")))
            .collect();
        out.push_str(&format!("  {}\n", cores.join("  ")));
    }
    if let Some(l) = &s.load {
        out.push_str(&format!(
            "  Load {} {} {}   tasks {} running of {}\n",
            r(&l.load1, ""),
            r(&l.load5, ""),
            r(&l.load15, ""),
            r(&l.runnable_tasks, ""),
            r(&l.total_tasks, "")
        ));
    }
    if let Some(m) = &s.memory {
        section(&mut out, "MEMORY", m.measured_at);
        let b = |x: &Reading<u64>| x.value.map(bytes).unwrap_or_else(|| r(x, ""));
        out.push_str(&format!(
            "  Used {} ({})   available {}   cached {}   total {}\n",
            b(&m.used_bytes),
            r(&m.used_percent, "%"),
            b(&m.available_bytes),
            b(&m.cached_bytes),
            b(&m.total_bytes)
        ));
        out.push_str(&format!(
            "  Swap used {} of {}   in/out {} / {} pages/s   pressure {}\n",
            b(&m.swap_used_bytes),
            b(&m.swap_total_bytes),
            r(&m.swap_in_pages_per_s, ""),
            r(&m.swap_out_pages_per_s, ""),
            r(&m.pressure.some_avg10, "%")
        ));
    }
    if let Some(b) = &s.battery {
        section(&mut out, "BATTERY", b.measured_at);
        for bat in &b.batteries {
            let runtime = match &bat.runtime {
                RuntimeEstimate::NotDischarging => "not discharging".to_string(),
                RuntimeEstimate::Calculating { .. } => "Calculating remaining time...".to_string(),
                RuntimeEstimate::Insufficient { reason } => {
                    format!("Insufficient battery data for a reliable estimate. {reason}")
                }
                RuntimeEstimate::Estimated {
                    seconds,
                    confidence,
                    ..
                } => format!(
                    "about {}h {:02}m left (estimate, {:?} confidence)",
                    seconds / 3600,
                    (seconds % 3600) / 60,
                    confidence
                )
                .to_lowercase(),
            };
            out.push_str(&format!(
                "  {}: {} {}   power {}   health {}   temperature {}\n  Runtime: {runtime}\n",
                bat.name,
                r(&bat.percent, "%"),
                bat.state
                    .value
                    .map(|s| format!("{s:?}").to_lowercase())
                    .unwrap_or_default(),
                r(&bat.power_w, " W"),
                r(&bat.health_percent, "%"),
                r(&bat.temperature_c, " °C"),
            ));
        }
        out.push_str(&format!("  On mains power: {}\n", r(&b.on_ac, "")));
    }
    if let Some(t) = &s.thermal {
        section(&mut out, "TEMPERATURES", t.measured_at);
        for x in &t.temperatures {
            out.push_str(&format!(
                "  {:<22} {:>10}   {:?} ({:?} confidence){}\n",
                format!("{} {}", x.chip, x.label.clone().unwrap_or_default()),
                r(&x.value_c, " °C"),
                x.association.component,
                x.association.confidence,
                x.critical_c
                    .map(|c| format!("  critical {c} °C (driver)"))
                    .unwrap_or_default()
            ));
        }
    }
    if let Some(f) = &s.fans {
        for fan in &f.fans {
            out.push_str(&format!(
                "  Fan {} {}: {}\n",
                fan.chip,
                fan.label.clone().unwrap_or_default(),
                r(&fan.rpm, " RPM")
            ));
        }
    }
    if let Some(g) = &s.gpu {
        section(&mut out, "GPU", g.measured_at);
        for gpu in &g.gpus {
            out.push_str(&format!(
                "  {} ({}): busy {}   temp {}   power {}   clock {}   memory {:?} used {}\n",
                gpu.pci_address,
                gpu.driver.clone().unwrap_or_default(),
                r(&gpu.busy_percent, "%"),
                r(&gpu.temperature_c, " °C"),
                r(&gpu.power_w, " W"),
                r(&gpu.core_clock_mhz, " MHz"),
                gpu.memory_model,
                gpu.vram_used_bytes
                    .value
                    .map(bytes)
                    .unwrap_or_else(|| r(&gpu.vram_used_bytes, ""))
            ));
            if let Some(n) = &gpu.note {
                out.push_str(&format!("    {n}\n"));
            }
        }
    }
    if let Some(st) = &s.storage {
        section(&mut out, "STORAGE", st.measured_at);
        for d in &st.devices {
            out.push_str(&format!(
                "  {}: read {}   write {}   busy {}\n",
                d.name,
                rate(&d.read_bps),
                rate(&d.write_bps),
                r(&d.busy_percent, "%")
            ));
        }
        for fs in &st.filesystems {
            out.push_str(&format!(
                "  {} ({}): {} used, {} free\n",
                fs.mount_point,
                fs.fs_type,
                r(&fs.used_percent, "%"),
                fs.available_bytes
                    .value
                    .map(bytes)
                    .unwrap_or_else(|| r(&fs.available_bytes, ""))
            ));
        }
    }
    if let Some(n) = &s.network {
        section(&mut out, "NETWORK", n.measured_at);
        for i in n.interfaces.iter().filter(|i| !i.loopback) {
            out.push_str(&format!(
                "  {}: down {}   up {}   errors {}/{}   dropped {}/{}\n",
                i.name,
                rate(&i.rx_bps),
                rate(&i.tx_bps),
                i.rx_errors,
                i.tx_errors,
                i.rx_dropped,
                i.tx_dropped
            ));
        }
    }
    if !s.health.is_empty() {
        out.push_str("\nHEALTH\n");
        for h in &s.health {
            out.push_str(&format!(
                "  {:<20} {:<9} {}{}\n",
                format!("{:?}", h.area),
                format!("{:?}", h.level).to_lowercase(),
                h.summary,
                if h.evidence.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", h.evidence.join("; "))
                }
            ));
        }
    }
    out
}

pub fn metrics(sentinel: &Sentinel, section: Option<&str>, as_json: bool) -> Result<ExitCode> {
    if section == Some("processes") {
        let mut answer = sentinel.processes()?;
        // CPU shares need two samples. Asking switches the process
        // collector to full rate, so one interval later they exist.
        let pending = |a: &sentinel_ipc::ProcessAnswer| {
            a.processes.as_ref().is_some_and(|l| {
                l.processes
                    .iter()
                    .all(|p| p.cpu_percent.state == ReadingState::Pending)
            })
        };
        if pending(&answer) {
            let wait = answer
                .monitoring
                .sampling
                .intervals_ms
                .iter()
                .find(|(k, _)| *k == CollectorKind::Processes)
                .map(|(_, ms)| *ms)
                .unwrap_or(5000);
            std::thread::sleep(std::time::Duration::from_millis(wait.min(10_000) + 500));
            answer = sentinel.processes()?;
        }
        if as_json {
            json(&answer)?;
        } else {
            match &answer.processes {
                None => return off(&answer.monitoring),
                Some(list) => {
                    let mut procs = list.processes.clone();
                    procs.sort_by(|a, b| {
                        b.cpu_percent
                            .value
                            .unwrap_or(0.0)
                            .total_cmp(&a.cpu_percent.value.unwrap_or(0.0))
                    });
                    println!(
                        "{} processes ({} running, {} not readable by this user), measured {}",
                        list.total,
                        list.running,
                        list.inaccessible,
                        clock(list.measured_at)
                    );
                    println!(
                        "  {:>7}  {:<16} {:>7} {:>10} {:>6}  {:<10} STATE",
                        "PID", "NAME", "CPU%", "MEMORY", "MEM%", "USER"
                    );
                    for p in procs.iter().take(25) {
                        println!(
                            "  {:>7}  {:<16} {:>7} {:>10} {:>6}  {:<10} {:?}",
                            p.pid,
                            p.name,
                            r(&p.cpu_percent, ""),
                            bytes(p.rss_bytes),
                            r(&p.memory_percent, ""),
                            p.user.clone().unwrap_or_else(|| "?".into()),
                            p.state
                        );
                    }
                    println!("  (read-only: Sentinel never stops or changes processes)");
                }
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    let kind = match section {
        None | Some("all") => MetricsSection::All,
        Some("cpu") => MetricsSection::Cpu,
        Some("memory") => MetricsSection::Memory,
        Some("battery") => MetricsSection::Battery,
        Some("thermal") => MetricsSection::Thermal,
        Some("gpu") => MetricsSection::Gpu,
        Some("storage") => MetricsSection::Storage,
        Some("network") => MetricsSection::Network,
        Some(other) => {
            return Err(SentinelError::InvalidConfig(format!(
                "unknown section {other:?}; use cpu, memory, processes, battery, thermal, gpu, storage or network"
            )));
        }
    };
    let answer = sentinel.metrics(kind)?;
    if as_json {
        json(&answer)?;
        return Ok(if answer.snapshot.is_some() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(EXIT_MONITORING_OFF)
        });
    }
    match &answer.snapshot {
        None => off(&answer.monitoring),
        Some(s) => {
            let label = if answer.monitoring.phase == MonitoringPhase::Paused {
                "PAUSED — last values before pausing, not live"
            } else {
                "Live"
            };
            println!(
                "{label} ({:?} sampling)",
                answer.monitoring.sampling.effective
            );
            print!("{}", render_snapshot(s));
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn off(m: &MonitoringReport) -> Result<ExitCode> {
    if matches!(m.phase, MonitoringPhase::Active | MonitoringPhase::Starting) {
        println!(
            "Monitoring is {} but this collector has not produced a sample yet. Try again in a moment.",
            phase_text(m.phase)
        );
        return Ok(ExitCode::from(EXIT_MONITORING_OFF));
    }
    println!(
        "Monitoring is {}: there are no live measurements. Start it with `sentinel monitor start`.",
        phase_text(m.phase)
    );
    Ok(ExitCode::from(EXIT_MONITORING_OFF))
}

pub fn render_alert(a: &Alert) -> String {
    format!(
        "#{} [{}] {:?} {} — {}{}\n    {}\n    Evidence: {}\n    Threshold: {} ({:?}){}\n",
        a.id,
        format!("{:?}", a.severity).to_uppercase(),
        a.status,
        a.title,
        clock(a.detected_at),
        a.resolved_at
            .map(|t| format!(", resolved {}", clock(t)))
            .unwrap_or_default(),
        a.explanation,
        a.evidence,
        a.threshold,
        a.threshold_source,
        a.recommendation
            .as_ref()
            .map(|r| format!("\n    Suggestion: {r}"))
            .unwrap_or_default()
    )
}

pub fn alerts(sentinel: &Sentinel, history: bool, as_json: bool) -> Result<ExitCode> {
    let alerts = if history {
        sentinel.alert_history(100)?
    } else {
        sentinel.active_alerts()?
    };
    if as_json {
        json(&alerts)?;
    } else if alerts.is_empty() {
        println!(
            "{}",
            if history {
                "No alerts recorded."
            } else {
                "No active alerts."
            }
        );
    } else {
        for a in &alerts {
            print!("{}", render_alert(a));
        }
        println!("Alerts are informational: Sentinel never acts on them.");
    }
    Ok(ExitCode::SUCCESS)
}

pub fn acknowledge(sentinel: &Sentinel, id: u64) -> Result<ExitCode> {
    let a = sentinel.acknowledge_alert(id)?;
    println!(
        "Alert #{} acknowledged. Nothing on the system was changed.",
        a.id
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_readings_are_explained() {
        assert_eq!(r(&Reading::<f64>::pending(), "%"), "measuring…");
        assert_eq!(
            r(
                &Reading::<f64>::missing(ReadingState::PermissionDenied),
                "%"
            ),
            "not available (administrator only)"
        );
        assert_eq!(r(&Reading::direct(0.0), "%"), "0%");
        assert_eq!(r(&Reading::estimated(82.0), "%"), "82% (estimate)");
    }
}
