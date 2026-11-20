//! Temperatures (hwmon `temp*_input`, thermal zones) and fan speeds (hwmon
//! `fan*_input`). Read only: `pwm*` and other control files are never
//! opened. hwmon temperatures are millidegrees Celsius.
//!
//! A sensor is associated with a component only on reliable evidence: a
//! driver that measures one kind of component (high), the driver's device
//! (medium), or a firmware label such as "CPU" (low). Otherwise "unknown".

use sentinel_hardware::SysRoot;

use super::{RunReport, SampleTime};
use crate::model::{
    Association, AssociationConfidence, Component, FanReading, FanReport, Reading, ReadingState,
    TemperatureReading, ThermalReport, round1,
};

const HWMON: &str = "sys/class/hwmon";
const THERMAL: &str = "sys/class/thermal";

/// Millidegrees to degrees; implausible values are errors, not readings.
pub fn millidegrees(raw: &str) -> Option<f64> {
    let v: f64 = raw.trim().parse::<i64>().ok()? as f64 / 1000.0;
    (-60.0..=250.0).contains(&v).then(|| round1(v))
}

pub fn associate(chip: &str, label: Option<&str>) -> Association {
    let a = |component, confidence, basis: &str| Association {
        component,
        confidence,
        basis: basis.to_string(),
    };
    let lower = chip.to_ascii_lowercase();
    match lower.as_str() {
        "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" | "fam15h_power" => {
            return a(
                Component::Cpu,
                AssociationConfidence::High,
                "the driver measures only the processor",
            );
        }
        "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" => {
            return a(
                Component::Gpu,
                AssociationConfidence::High,
                "the graphics driver's own sensor",
            );
        }
        "nvme" | "drivetemp" => {
            return a(
                Component::Storage,
                AssociationConfidence::High,
                "the drive's own sensor",
            );
        }
        "spd5118" | "jc42" | "ee1004" => {
            return a(
                Component::Memory,
                AssociationConfidence::High,
                "a memory-module sensor",
            );
        }
        "acpitz" => {
            return a(
                Component::Unknown,
                AssociationConfidence::None,
                "an ACPI thermal zone; firmware does not say where it is",
            );
        }
        _ => {}
    }
    if lower.starts_with("bat") {
        return a(
            Component::Battery,
            AssociationConfidence::High,
            "the battery's own sensor",
        );
    }
    if lower.starts_with("iwlwifi")
        || lower.starts_with("ath")
        || lower.starts_with("mt7")
        || lower.starts_with("rtw")
    {
        return a(
            Component::Wireless,
            AssociationConfidence::Medium,
            "the wireless driver's sensor",
        );
    }
    match label.map(|l| l.to_ascii_lowercase()) {
        Some(l)
            if l.contains("cpu")
                || l.contains("package")
                || l.contains("tctl")
                || l.contains("tdie") =>
        {
            a(
                Component::Cpu,
                AssociationConfidence::Low,
                "firmware labels it as the CPU",
            )
        }
        Some(l) if l.contains("gpu") => a(
            Component::Gpu,
            AssociationConfidence::Low,
            "firmware labels it as the GPU",
        ),
        Some(l) if l.contains("dimm") || l.contains("memory") => a(
            Component::Memory,
            AssociationConfidence::Low,
            "firmware labels it as memory",
        ),
        Some(l) if l.contains("ambient") || l.contains("chassis") || l.contains("systin") => a(
            Component::Chassis,
            AssociationConfidence::Low,
            "firmware labels it as ambient/chassis",
        ),
        _ => a(
            Component::Unknown,
            AssociationConfidence::None,
            "no reliable information about what it measures",
        ),
    }
}

fn read_limit(root: &SysRoot, rel: &str) -> Option<f64> {
    root.read_trimmed(rel)
        .ok()
        .and_then(|v| millidegrees(&v))
        .filter(|v| *v > 0.0)
}

pub fn sample_temperatures(root: &SysRoot, t: SampleTime) -> (Option<ThermalReport>, RunReport) {
    let mut temperatures = Vec::new();
    let mut failed = 0;
    for chip_dir in root.list(HWMON).unwrap_or_default() {
        let dir = format!("{HWMON}/{chip_dir}");
        // The kernel mirrors every thermal zone as an hwmon device; the zone
        // itself is read below, with its trip points, so skip the copy.
        if root
            .canonical(&dir)
            .unwrap_or_default()
            .contains("/virtual/thermal/")
        {
            continue;
        }
        let chip = root
            .read_trimmed(&format!("{dir}/name"))
            .unwrap_or_else(|_| chip_dir.clone());
        for file in root.list(&dir).unwrap_or_default() {
            let Some(stem) = file
                .strip_suffix("_input")
                .filter(|s| s.starts_with("temp"))
            else {
                continue;
            };
            let label = root.read_trimmed(&format!("{dir}/{stem}_label")).ok();
            let value_c = match root.read_trimmed(&format!("{dir}/{file}")) {
                Ok(raw) => millidegrees(&raw)
                    .map(Reading::direct)
                    .unwrap_or_else(|| Reading::missing(ReadingState::Error)),
                Err(e) => Reading::missing(super::state_of(&e)),
            };
            if !value_c.is_ok() {
                failed += 1;
            }
            temperatures.push(TemperatureReading {
                id: format!("{chip_dir}/{stem}"),
                association: associate(&chip, label.as_deref()),
                max_c: read_limit(root, &format!("{dir}/{stem}_max")),
                critical_c: read_limit(root, &format!("{dir}/{stem}_crit")),
                chip: chip.clone(),
                label,
                value_c,
                source: root.display(&format!("{dir}/{file}")),
            });
        }
    }
    for zone in root
        .list(THERMAL)
        .unwrap_or_default()
        .into_iter()
        .filter(|z| z.starts_with("thermal_zone"))
    {
        let dir = format!("{THERMAL}/{zone}");
        let kind = root
            .read_trimmed(&format!("{dir}/type"))
            .unwrap_or_default();
        let value_c = match root.read_trimmed(&format!("{dir}/temp")) {
            Ok(raw) => millidegrees(&raw)
                .map(Reading::direct)
                .unwrap_or_else(|| Reading::missing(ReadingState::Error)),
            Err(e) => Reading::missing(super::state_of(&e)),
        };
        if !value_c.is_ok() {
            failed += 1;
        }
        // The zone's critical trip point is a firmware-defined limit.
        let critical_c = (0..8).find_map(|i| {
            let kind = root
                .read_trimmed(&format!("{dir}/trip_point_{i}_type"))
                .ok()?;
            (kind == "critical").then(|| read_limit(root, &format!("{dir}/trip_point_{i}_temp")))?
        });
        temperatures.push(TemperatureReading {
            id: zone.clone(),
            association: associate(&kind, None),
            chip: "thermal".into(),
            label: Some(kind),
            value_c,
            max_c: None,
            critical_c,
            source: root.display(&format!("{dir}/temp")),
        });
    }
    let report = if temperatures.is_empty() {
        RunReport::unsupported("no temperature sensors are exposed")
    } else if failed > 0 {
        RunReport::partial(format!("{failed} sensor(s) could not be read"))
    } else {
        RunReport::ok()
    };
    (
        Some(ThermalReport {
            measured_at: t.wall_ms,
            interval_ms: t.interval_ms,
            temperatures,
        }),
        report,
    )
}

pub fn sample_fans(root: &SysRoot, t: SampleTime) -> (Option<FanReport>, RunReport) {
    let mut fans = Vec::new();
    for chip_dir in root.list(HWMON).unwrap_or_default() {
        let dir = format!("{HWMON}/{chip_dir}");
        let chip = root
            .read_trimmed(&format!("{dir}/name"))
            .unwrap_or_else(|_| chip_dir.clone());
        for file in root.list(&dir).unwrap_or_default() {
            let Some(stem) = file.strip_suffix("_input").filter(|s| s.starts_with("fan")) else {
                continue;
            };
            let num = |attr: &str| {
                root.read_trimmed(&format!("{dir}/{stem}_{attr}"))
                    .ok()
                    .and_then(|v| v.parse::<u32>().ok())
            };
            let rpm = match root.read_trimmed(&format!("{dir}/{file}")) {
                Ok(v) => v
                    .parse::<u32>()
                    .map(Reading::direct)
                    .unwrap_or_else(|_| Reading::missing(ReadingState::Error)),
                Err(e) => Reading::missing(super::state_of(&e)),
            };
            fans.push(FanReading {
                id: format!("{chip_dir}/{stem}"),
                chip: chip.clone(),
                label: root.read_trimmed(&format!("{dir}/{stem}_label")).ok(),
                rpm,
                min_rpm: num("min"),
                max_rpm: num("max"),
                source: root.display(&format!("{dir}/{file}")),
            });
        }
    }
    let report = if fans.is_empty() {
        RunReport::unsupported("no fan sensors are exposed")
    } else {
        RunReport::ok()
    };
    (
        Some(FanReport {
            measured_at: t.wall_ms,
            interval_ms: t.interval_ms,
            fans,
        }),
        report,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_conversion() {
        assert_eq!(millidegrees("45250"), Some(45.3));
        assert_eq!(millidegrees("-5000"), Some(-5.0));
        assert_eq!(millidegrees("999999"), None, "implausible");
        assert_eq!(millidegrees("abc"), None);
    }

    #[test]
    fn associations_need_evidence() {
        assert_eq!(
            associate("k10temp", Some("Tctl")).confidence,
            AssociationConfidence::High
        );
        assert_eq!(associate("amdgpu", Some("edge")).component, Component::Gpu);
        assert_eq!(
            associate("nvme", Some("Composite")).component,
            Component::Storage
        );
        assert_eq!(associate("BAT1", None).component, Component::Battery);
        let fw = associate("dell_smm", Some("CPU"));
        assert_eq!(
            (fw.component, fw.confidence),
            (Component::Cpu, AssociationConfidence::Low)
        );
        assert_eq!(
            associate("dell_smm", Some("SODIMM")).component,
            Component::Memory
        );
        assert_eq!(
            associate("dell_smm", Some("Ambient")).component,
            Component::Chassis
        );
        let zone = associate("acpitz", None);
        assert_eq!(
            (zone.component, zone.confidence),
            (Component::Unknown, AssociationConfidence::None)
        );
        assert_eq!(
            associate("mystery", Some("temp3")).component,
            Component::Unknown
        );
    }
}
