//! Sensor interfaces: hwmon chips and thermal zones.
//!
//! Only metadata is recorded (chip, kind, label, path, whether the user may
//! read it). No sensor value is read: readings belong to the telemetry
//! phase. Readability is checked by opening the file and closing it again.

use sentinel_inventory::{
    Availability, Confidence, Field, ModuleOutcome, SensorAccess, SensorInfo, SensorKind,
    ThermalZone,
};

use crate::context::{DiscoveryContext, ModuleRun, Stopped, last_pci_address};
use crate::fields;
use crate::source::Access;

const HWMON: &str = "sys/class/hwmon";
const THERMAL: &str = "sys/class/thermal";

/// Sensor inputs and thermal zones.
pub type SensorInventory = (Vec<SensorInfo>, Vec<ThermalZone>);

/// `temp1_input` -> `(Temperature, "temp1")`.
pub fn parse_input(file: &str) -> Option<(SensorKind, String)> {
    let stem = file.strip_suffix("_input")?;
    let prefix = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    if prefix.len() == stem.len() {
        return None;
    }
    let kind = match prefix {
        "temp" => SensorKind::Temperature,
        "fan" => SensorKind::Fan,
        "in" => SensorKind::Voltage,
        "curr" => SensorKind::Current,
        "power" => SensorKind::Power,
        "energy" => SensorKind::Energy,
        "freq" => SensorKind::Frequency,
        "humidity" => SensorKind::Humidity,
        _ => SensorKind::Other,
    };
    Some((kind, stem.to_string()))
}

fn access_field(ctx: &DiscoveryContext, rel: &str) -> Field<SensorAccess> {
    match ctx.root.access(rel) {
        Access::Readable => Field::detected(
            SensorAccess::Readable,
            ctx.root.display(rel),
            Confidence::Direct,
        ),
        Access::PermissionDenied => Field::detected(
            SensorAccess::PermissionDenied,
            ctx.root.display(rel),
            Confidence::Direct,
        )
        .with_note("readable only by administrators"),
        Access::Missing => Field::missing(Availability::Unavailable, Some(ctx.root.display(rel))),
    }
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<SensorInventory>, Stopped> {
    let root = &ctx.root;
    let mut sensors = Vec::new();
    let mut restricted = 0;
    let hwmon_present = root.is_dir(HWMON);
    for chip_dir in root.list(HWMON).unwrap_or_default() {
        ctx.check()?;
        let dir = format!("{HWMON}/{chip_dir}");
        let chip = root
            .read_trimmed(&format!("{dir}/name"))
            .unwrap_or_else(|_| chip_dir.clone());
        let device_path = root.canonical(&format!("{dir}/device"));
        let device = match &device_path {
            Some(path) => {
                let label = match last_pci_address(path) {
                    Some(pci) => format!("PCI {pci}"),
                    None => path.rsplit('/').next().unwrap_or(path).to_string(),
                };
                Field::detected(
                    label,
                    root.display(&format!("{dir}/device")),
                    Confidence::KernelReported,
                )
                .with_note("the device providing the sensor, not necessarily what it measures")
            }
            None => Field::missing(Availability::Unavailable, None),
        };
        let files = root.list(&dir).unwrap_or_default();
        for file in &files {
            let Some((kind, stem)) = parse_input(file) else {
                continue;
            };
            let input = format!("{dir}/{file}");
            let access = access_field(ctx, &input);
            if access.value() == Some(&SensorAccess::PermissionDenied) {
                restricted += 1;
            }
            sensors.push(SensorInfo {
                discovered_at: ctx.now,
                id: format!("{chip_dir}/{stem}"),
                chip: chip.clone(),
                kind,
                label: if files.contains(&format!("{stem}_label")) {
                    fields::text(
                        root,
                        &format!("{dir}/{stem}_label"),
                        Confidence::KernelReported,
                    )
                } else {
                    Field::missing(Availability::Unavailable, None)
                        .with_note("the driver gives no label")
                },
                input_path: root.display(&input),
                access,
                device: device.clone(),
            });
        }
    }

    let mut zones = Vec::new();
    for name in root.list(THERMAL).unwrap_or_default() {
        if !name.starts_with("thermal_zone") {
            continue;
        }
        ctx.check()?;
        let dir = format!("{THERMAL}/{name}");
        let trip_points = root
            .list(&dir)
            .unwrap_or_default()
            .iter()
            .filter(|f| f.starts_with("trip_point_") && f.ends_with("_temp"))
            .count() as u32;
        zones.push(ThermalZone {
            discovered_at: ctx.now,
            zone_type: fields::text(root, &format!("{dir}/type"), Confidence::KernelReported),
            access: access_field(ctx, &format!("{dir}/temp")),
            trip_points,
            name,
        });
    }

    let mut notes = Vec::new();
    if restricted > 0 {
        notes.push(format!(
            "{restricted} sensor(s) are readable only by administrators"
        ));
    }
    let outcome = if !hwmon_present && zones.is_empty() {
        notes.push("No hardware sensor interfaces are exposed.".into());
        ModuleOutcome::Unavailable
    } else if restricted > 0 {
        ModuleOutcome::Partial
    } else {
        ModuleOutcome::Completed
    };
    let count = sensors.len() + zones.len();
    Ok(ModuleRun::new((sensors, zones), outcome, count, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_files() {
        assert_eq!(
            parse_input("temp1_input"),
            Some((SensorKind::Temperature, "temp1".into()))
        );
        assert_eq!(
            parse_input("fan2_input"),
            Some((SensorKind::Fan, "fan2".into()))
        );
        assert_eq!(
            parse_input("in0_input"),
            Some((SensorKind::Voltage, "in0".into()))
        );
        assert_eq!(
            parse_input("power1_input"),
            Some((SensorKind::Power, "power1".into()))
        );
        assert_eq!(parse_input("temp1_label"), None);
        assert_eq!(parse_input("name"), None);
        assert_eq!(parse_input("temp_input"), None);
    }
}
