//! Batteries and power sources from `/sys/class/power_supply`.
//!
//! Units in sysfs: µWh, µAh, µV, µA, µW; `temp` in tenths of a degree.
//! Power draw is `power_now` when exposed, else |current| × voltage (marked
//! calculated). Battery temperature comes only from the battery's own
//! `temp` attribute; if that cannot be read it is reported unavailable, and
//! no other sensor is substituted.

use std::collections::BTreeMap;

use sentinel_hardware::SysRoot;

use super::{RunReport, SampleTime};
use crate::model::{
    BatteryReport, BatterySample, ChargeState, PowerSourceSample, Reading, ReadingState, round1,
    round2,
};
use crate::runtime::{RuntimeEstimator, health_percent};

const SUPPLIES: &str = "sys/class/power_supply";

#[derive(Default)]
pub struct BatteryCollector {
    estimators: BTreeMap<String, RuntimeEstimator>,
}

fn micro(root: &SysRoot, rel: &str) -> Reading<f64> {
    match root.read_trimmed(rel) {
        Ok(v) => match v.parse::<i64>() {
            Ok(n) => Reading::direct(n as f64 / 1e6),
            Err(_) => Reading::missing(ReadingState::Error),
        },
        Err(e) => Reading::missing(super::state_of(&e)),
    }
}

pub fn parse_state(text: &str) -> ChargeState {
    match text {
        "Charging" => ChargeState::Charging,
        "Discharging" => ChargeState::Discharging,
        "Not charging" => ChargeState::NotCharging,
        "Full" => ChargeState::Full,
        _ => ChargeState::Unknown,
    }
}

/// Battery temperature from its own `temp` attribute (tenths of °C).
/// Implausible values are rejected rather than shown.
pub fn battery_temperature(
    raw: Result<String, sentinel_hardware::source::ReadError>,
) -> Reading<f64> {
    match raw {
        Ok(v) => match v.parse::<f64>() {
            Ok(tenths) if (-400.0..=1000.0).contains(&tenths) => {
                Reading::direct(round1(tenths / 10.0))
            }
            _ => Reading::missing(ReadingState::Error),
        },
        Err(e) => Reading::missing(super::state_of(&e)),
    }
}

impl BatteryCollector {
    pub fn sample(&mut self, root: &SysRoot, t: SampleTime) -> (Option<BatteryReport>, RunReport) {
        let Ok(names) = root.list(SUPPLIES) else {
            return (None, RunReport::unsupported("no power-supply interface"));
        };
        let mut batteries = Vec::new();
        let mut sources = Vec::new();
        for name in names {
            let dir = format!("{SUPPLIES}/{name}");
            let kind = root
                .read_trimmed(&format!("{dir}/type"))
                .unwrap_or_default();
            if kind != "Battery" {
                let online = match root.read_trimmed(&format!("{dir}/online")) {
                    Ok(v) => Reading::direct(v == "1"),
                    Err(e) => Reading::missing(super::state_of(&e)),
                };
                sources.push(PowerSourceSample { name, kind, online });
                continue;
            }
            // Scoped "Device" batteries (mice, headsets) are listed too but
            // never drive the computer's runtime estimate.
            let a = |attr: &str| micro(root, &format!("{dir}/{attr}"));
            let state_text = root.read_trimmed(&format!("{dir}/status"));
            let state = match &state_text {
                Ok(s) => Reading::direct(parse_state(s)),
                Err(e) => Reading::missing(super::state_of(e)),
            };
            let percent = match root.read_trimmed(&format!("{dir}/capacity")) {
                Ok(v) => v
                    .parse::<f64>()
                    .map(Reading::direct)
                    .unwrap_or_else(|_| Reading::missing(ReadingState::Error)),
                Err(e) => Reading::missing(super::state_of(&e)),
            };
            let (energy_now, energy_full, energy_design) =
                (a("energy_now"), a("energy_full"), a("energy_full_design"));
            let (charge_now, charge_full, charge_design) =
                (a("charge_now"), a("charge_full"), a("charge_full_design"));
            let voltage = a("voltage_now");
            let current = a("current_now").map(f64::abs);
            let power = match a("power_now").value() {
                Some(p) => Reading::direct(round2(p.abs())),
                None => match (current.value(), voltage.value()) {
                    (Some(i), Some(v)) => Reading::calculated(round2(i * v)),
                    _ => Reading::missing(ReadingState::Unsupported),
                },
            };
            let health = health_percent(energy_full.value(), energy_design.value())
                .or_else(|| health_percent(charge_full.value(), charge_design.value()));
            let discharging = state.value() == Some(ChargeState::Discharging);
            // Energy-reporting batteries: Wh and W. Charge-reporting: Ah and A.
            let (remaining, rate) = if energy_now.is_ok() {
                (energy_now.value(), power.value())
            } else {
                (charge_now.value(), current.value())
            };
            // A charge-reporting battery's average is in amperes; its draw is
            // shown in watts at the present voltage, or not at all.
            let watts_per_unit = if energy_now.is_ok() {
                Some(1.0)
            } else {
                voltage.value()
            };
            let runtime = self
                .estimators
                .entry(name.clone())
                .or_default()
                .update_scaled(t.mono_s, discharging, remaining, rate, watts_per_unit);
            let text = |attr: &str| {
                root.read_trimmed(&format!("{dir}/{attr}"))
                    .ok()
                    .filter(|v| v != "Unknown")
            };
            batteries.push(BatterySample {
                measured_at: t.wall_ms,
                interval_ms: t.interval_ms,
                state,
                percent,
                energy_now_wh: energy_now.map(round2),
                energy_full_wh: energy_full.map(round2),
                energy_design_wh: energy_design.map(round2),
                charge_now_ah: charge_now.map(round2),
                charge_full_ah: charge_full.map(round2),
                charge_design_ah: charge_design.map(round2),
                voltage_v: voltage.map(round2),
                current_a: current.map(round2),
                power_w: power,
                cycle_count: match root
                    .read_trimmed(&format!("{dir}/cycle_count"))
                    .ok()
                    .and_then(|v| v.parse().ok())
                {
                    // 0 is how most firmware says "not counted".
                    Some(0) | None => Reading::missing(ReadingState::Unavailable),
                    Some(n) => Reading::direct(n),
                },
                health_percent: health
                    .map(Reading::estimated)
                    .unwrap_or_else(|| Reading::missing(ReadingState::Unavailable)),
                temperature_c: battery_temperature(root.read_trimmed(&format!("{dir}/temp"))),
                runtime,
                technology: text("technology"),
                manufacturer: text("manufacturer"),
                model: text("model_name"),
                name,
            });
        }
        let ac: Vec<bool> = sources
            .iter()
            .filter(|s| s.kind == "Mains" || s.kind.starts_with("USB"))
            .filter_map(|s| s.online.value())
            .collect();
        let on_ac = if ac.is_empty() {
            if batteries.is_empty() {
                Reading::missing(ReadingState::Unsupported)
            } else {
                // No adapter reported: infer from the battery state.
                match batteries.iter().find_map(|b| b.state.value()) {
                    Some(ChargeState::Discharging) => Reading::calculated(false),
                    Some(ChargeState::Charging | ChargeState::Full) => Reading::calculated(true),
                    _ => Reading::missing(ReadingState::Unknown),
                }
            }
        } else {
            Reading::direct(ac.iter().any(|o| *o))
        };
        let report = if batteries.is_empty() && sources.is_empty() {
            RunReport::unsupported("no batteries or power sources")
        } else {
            RunReport::ok()
        };
        (
            Some(BatteryReport {
                measured_at: t.wall_ms,
                interval_ms: t.interval_ms,
                batteries,
                sources,
                on_ac,
            }),
            report,
        )
    }
}
