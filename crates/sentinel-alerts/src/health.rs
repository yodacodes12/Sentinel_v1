//! Health indicators: one understandable status per area with the
//! evidence behind it. There is no combined score.
//!
//! Sustained conditions come from active alerts (which already require a
//! duration); current readings can raise an area to "elevated" at most, so
//! a brief spike never shows as "high" or "critical" on its own.

use sentinel_config::AlertThresholds;
use sentinel_telemetry::model::{
    Alert, AlertSeverity, ChargeState, CollectorHealth, CollectorStatus, HealthArea,
    HealthIndicator, HealthLevel, TelemetrySnapshot,
};

fn level_of(alerts: &[&Alert]) -> Option<HealthLevel> {
    alerts.iter().map(|a| a.severity).max().map(|s| match s {
        AlertSeverity::Critical => HealthLevel::Critical,
        AlertSeverity::Warning => HealthLevel::High,
        AlertSeverity::Info => HealthLevel::Elevated,
    })
}

fn indicator(
    area: HealthArea,
    level: HealthLevel,
    summary: impl Into<String>,
    evidence: Vec<String>,
) -> HealthIndicator {
    HealthIndicator {
        area,
        level,
        summary: summary.into(),
        evidence,
    }
}

pub fn indicators(
    s: &TelemetrySnapshot,
    t: &AlertThresholds,
    active: &[Alert],
    collectors: &[CollectorStatus],
) -> Vec<HealthIndicator> {
    let by_rule = |rules: &[&str]| -> Vec<&Alert> {
        active
            .iter()
            .filter(|a| rules.contains(&a.rule.as_str()))
            .collect()
    };
    let mut out = Vec::new();

    // CPU pressure.
    out.push(match &s.cpu {
        None => indicator(
            HealthArea::CpuPressure,
            HealthLevel::Unknown,
            "No CPU data yet",
            vec![],
        ),
        Some(cpu) => {
            let mut evidence = Vec::new();
            if let Some(v) = cpu.total_percent.value() {
                evidence.push(format!("CPU utilisation {v:.0}%"));
            }
            if let Some(p) = s
                .load
                .as_ref()
                .and_then(|l| l.cpu_pressure.some_avg60.value())
            {
                evidence.push(format!("CPU stall time {p:.1}% (60 s)"));
            }
            if let Some(l) = s.load.as_ref().and_then(|l| l.load1.value()) {
                evidence.push(format!(
                    "Load average {l:.2} on {} logical CPUs",
                    cpu.logical_cpus
                ));
            }
            let alerts = by_rule(&["cpu_high", "load_high"]);
            match level_of(&alerts) {
                Some(level) => indicator(
                    HealthArea::CpuPressure,
                    level,
                    "Sustained high CPU demand",
                    evidence,
                ),
                None if cpu.total_percent.value().is_some_and(|v| v >= 75.0) => indicator(
                    HealthArea::CpuPressure,
                    HealthLevel::Elevated,
                    "CPU busy right now",
                    evidence,
                ),
                None if cpu.total_percent.value().is_none() => indicator(
                    HealthArea::CpuPressure,
                    HealthLevel::Unknown,
                    "Waiting for a second sample",
                    evidence,
                ),
                None => indicator(
                    HealthArea::CpuPressure,
                    HealthLevel::Normal,
                    "Normal",
                    evidence,
                ),
            }
        }
    });

    // Memory and swap.
    match &s.memory {
        None => {
            out.push(indicator(
                HealthArea::MemoryPressure,
                HealthLevel::Unknown,
                "No memory data yet",
                vec![],
            ));
            out.push(indicator(
                HealthArea::SwapPressure,
                HealthLevel::Unknown,
                "No memory data yet",
                vec![],
            ));
        }
        Some(m) => {
            let avail = match (m.available_bytes.value(), m.total_bytes.value()) {
                (Some(a), Some(total)) if total > 0 => Some(a as f64 / total as f64 * 100.0),
                _ => None,
            };
            let mut evidence = Vec::new();
            if let Some(a) = avail {
                evidence.push(format!("{a:.0}% of memory available"));
            }
            if let Some(p) = m.pressure.some_avg60.value() {
                evidence.push(format!("Memory stall time {p:.1}% (60 s)"));
            }
            let level =
                level_of(&by_rule(&["memory_low", "memory_pressure"])).unwrap_or(match avail {
                    None => HealthLevel::Unknown,
                    Some(a) if a < t.memory_available_low_percent + 10.0 => HealthLevel::Elevated,
                    Some(_) => HealthLevel::Normal,
                });
            out.push(indicator(
                HealthArea::MemoryPressure,
                level,
                match level {
                    HealthLevel::Normal => "Normal",
                    HealthLevel::Unknown => "Not measured",
                    HealthLevel::Elevated => "Memory getting low",
                    _ => "Memory is low",
                },
                evidence,
            ));

            let mut evidence = Vec::new();
            let used_pct = match (m.swap_used_bytes.value(), m.swap_total_bytes.value()) {
                (Some(u), Some(total)) if total > 0 => Some(u as f64 / total as f64 * 100.0),
                _ => None,
            };
            if let Some(u) = used_pct {
                evidence.push(format!("{u:.0}% of swap in use"));
            }
            let activity = match (
                m.swap_in_pages_per_s.value(),
                m.swap_out_pages_per_s.value(),
            ) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
            if let Some(a) = activity {
                evidence.push(format!("{a:.0} pages/s swapped"));
            }
            let level = level_of(&by_rule(&["swap_activity"])).unwrap_or(
                match (m.swap_total_bytes.value(), activity) {
                    (Some(0), _) => HealthLevel::Normal,
                    (_, Some(a)) if a >= t.swap_activity_pages_per_s / 4.0 => HealthLevel::Elevated,
                    (_, Some(_)) => HealthLevel::Normal,
                    _ => HealthLevel::Unknown,
                },
            );
            // Swap that is used but idle is normal on Linux and is said so.
            let summary = match level {
                HealthLevel::Normal if used_pct.is_some_and(|u| u > 0.0) => "Swap in use but quiet",
                HealthLevel::Normal => "Normal",
                HealthLevel::Unknown => "Waiting for a second sample",
                _ => "Swapping",
            };
            out.push(indicator(
                HealthArea::SwapPressure,
                level,
                summary,
                evidence,
            ));
        }
    }

    // Thermal.
    out.push(match &s.thermal {
        None => indicator(
            HealthArea::Thermal,
            HealthLevel::Unknown,
            "No temperature data yet",
            vec![],
        ),
        Some(th) => {
            let readable: Vec<_> = th
                .temperatures
                .iter()
                .filter(|x| x.value_c.is_ok())
                .collect();
            let with_limits = readable
                .iter()
                .filter(|x| {
                    x.max_c.is_some() || x.critical_c.is_some() || t.temperature_warning_c.is_some()
                })
                .count();
            let mut evidence: Vec<String> = Vec::new();
            if let Some(hottest) = readable.iter().max_by(|a, b| {
                a.value_c
                    .value()
                    .unwrap_or(0.0)
                    .total_cmp(&b.value_c.value().unwrap_or(0.0))
            }) {
                evidence.push(format!(
                    "Hottest: {} {} at {:.1} °C",
                    hottest.chip,
                    hottest.label.clone().unwrap_or_default(),
                    hottest.value_c.value().unwrap_or(0.0)
                ));
            }
            evidence.push(format!(
                "{} of {} sensors have a known limit",
                with_limits,
                readable.len()
            ));
            match level_of(&by_rule(&["temperature_high", "thermal_repeated"])) {
                Some(level) => indicator(
                    HealthArea::Thermal,
                    level,
                    "A sensor is above its warning threshold",
                    evidence,
                ),
                None if readable.is_empty() => indicator(
                    HealthArea::Thermal,
                    HealthLevel::Unknown,
                    "No readable temperature sensors",
                    evidence,
                ),
                None if with_limits == 0 => indicator(
                    HealthArea::Thermal,
                    HealthLevel::Unknown,
                    "No driver-reported limits to compare against; you can set a warning threshold",
                    evidence,
                ),
                None => indicator(
                    HealthArea::Thermal,
                    HealthLevel::Normal,
                    "All sensors with known limits are below them",
                    evidence,
                ),
            }
        }
    });

    // Battery.
    out.push(match s.battery.as_ref().and_then(|b| b.batteries.first()) {
        None => indicator(
            HealthArea::Battery,
            HealthLevel::Unknown,
            "No battery",
            vec![],
        ),
        Some(b) => {
            let mut evidence = Vec::new();
            if let Some(p) = b.percent.value() {
                evidence.push(format!("{p:.0}% charged"));
            }
            if let Some(h) = b.health_percent.value() {
                evidence.push(format!("Estimated health {h:.0}%"));
            }
            let level = level_of(&by_rule(&[
                "battery_low",
                "battery_temperature",
                "battery_discharge_high",
                "battery_health",
            ]))
            .unwrap_or(match (b.state.value(), b.percent.value()) {
                (Some(ChargeState::Discharging), Some(p)) if p <= t.battery_low_percent + 10.0 => {
                    HealthLevel::Elevated
                }
                (_, Some(_)) => HealthLevel::Normal,
                _ => HealthLevel::Unknown,
            });
            indicator(
                HealthArea::Battery,
                level,
                match level {
                    HealthLevel::Normal => "Normal",
                    HealthLevel::Elevated => "Getting low or worn",
                    HealthLevel::Unknown => "Not reported",
                    _ => "Needs attention",
                },
                evidence,
            )
        }
    });

    // Storage capacity and activity.
    match &s.storage {
        None => {
            out.push(indicator(
                HealthArea::StorageCapacity,
                HealthLevel::Unknown,
                "No storage data yet",
                vec![],
            ));
            out.push(indicator(
                HealthArea::StorageActivity,
                HealthLevel::Unknown,
                "No storage data yet",
                vec![],
            ));
        }
        Some(st) => {
            let fullest = st
                .filesystems
                .iter()
                .filter(|f| f.used_percent.is_ok())
                .max_by(|a, b| {
                    a.used_percent
                        .value()
                        .unwrap_or(0.0)
                        .total_cmp(&b.used_percent.value().unwrap_or(0.0))
                });
            let evidence: Vec<String> = fullest
                .map(|f| {
                    vec![format!(
                        "Fullest: {} at {:.0}% used",
                        f.mount_point,
                        f.used_percent.value().unwrap_or(0.0)
                    )]
                })
                .unwrap_or_default();
            let level = level_of(&by_rule(&["storage_low"])).unwrap_or(
                match fullest.and_then(|f| f.used_percent.value()) {
                    None => HealthLevel::Unknown,
                    Some(u) if 100.0 - u < t.storage_free_low_percent + 10.0 => {
                        HealthLevel::Elevated
                    }
                    Some(_) => HealthLevel::Normal,
                },
            );
            out.push(indicator(
                HealthArea::StorageCapacity,
                level,
                match level {
                    HealthLevel::Normal => "Plenty of space",
                    HealthLevel::Elevated => "Space getting low",
                    HealthLevel::Unknown => "Not measured",
                    _ => "Low on space",
                },
                evidence,
            ));

            let busiest = st
                .devices
                .iter()
                .filter(|d| d.busy_percent.is_ok())
                .max_by(|a, b| {
                    a.busy_percent
                        .value()
                        .unwrap_or(0.0)
                        .total_cmp(&b.busy_percent.value().unwrap_or(0.0))
                });
            let evidence: Vec<String> = busiest
                .map(|d| {
                    vec![format!(
                        "Busiest: {} {:.0}% busy (activity, not health)",
                        d.name,
                        d.busy_percent.value().unwrap_or(0.0)
                    )]
                })
                .unwrap_or_default();
            let level = level_of(&by_rule(&["disk_busy"])).unwrap_or(
                match busiest.and_then(|d| d.busy_percent.value()) {
                    None => HealthLevel::Unknown,
                    Some(b) if b >= 60.0 => HealthLevel::Elevated,
                    Some(_) => HealthLevel::Normal,
                },
            );
            out.push(indicator(
                HealthArea::StorageActivity,
                level,
                match level {
                    HealthLevel::Normal => "Normal",
                    HealthLevel::Unknown => "Waiting for a second sample",
                    _ => "Busy",
                },
                evidence,
            ));
        }
    }

    // Network activity: informational unless the user set a threshold.
    out.push(match &s.network {
        None => indicator(
            HealthArea::NetworkActivity,
            HealthLevel::Unknown,
            "No network data yet",
            vec![],
        ),
        Some(net) => {
            let rates: Vec<f64> = net
                .interfaces
                .iter()
                .filter(|i| !i.loopback)
                .filter_map(|i| Some(i.rx_bps.value()? + i.tx_bps.value()?))
                .collect();
            let total_mbps = rates.iter().sum::<f64>() * 8.0 / 1e6;
            let level = level_of(&by_rule(&["network_high"])).unwrap_or(if rates.is_empty() {
                HealthLevel::Unknown
            } else {
                HealthLevel::Normal
            });
            indicator(
                HealthArea::NetworkActivity,
                level,
                if t.network_high_mbps.is_some() {
                    "Compared with your threshold"
                } else {
                    "Shown for information"
                },
                vec![format!("{total_mbps:.2} Mbit/s in total")],
            )
        }
    });

    // Sensor availability.
    let ran: Vec<_> = collectors
        .iter()
        .filter(|c| c.health != CollectorHealth::Waiting)
        .collect();
    let problems: Vec<String> = ran
        .iter()
        .filter(|c| {
            matches!(
                c.health,
                CollectorHealth::Failing | CollectorHealth::Partial | CollectorHealth::Unsupported
            )
        })
        .map(|c| {
            format!(
                "{}: {}",
                c.kind.label(),
                c.note
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", c.health).to_lowercase())
            )
        })
        .collect();
    out.push(if ran.is_empty() {
        indicator(
            HealthArea::SensorAvailability,
            HealthLevel::Unknown,
            "Monitoring is not running",
            vec![],
        )
    } else if ran.iter().any(|c| c.health == CollectorHealth::Failing) {
        indicator(
            HealthArea::SensorAvailability,
            HealthLevel::Elevated,
            "Some collectors are failing",
            problems,
        )
    } else if problems.is_empty() {
        indicator(
            HealthArea::SensorAvailability,
            HealthLevel::Normal,
            "All sources readable",
            vec![],
        )
    } else {
        indicator(
            HealthArea::SensorAvailability,
            HealthLevel::Normal,
            "Some sources are not available on this machine",
            problems,
        )
    });
    out
}
