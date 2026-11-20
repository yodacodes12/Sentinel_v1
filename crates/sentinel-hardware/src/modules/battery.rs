//! Batteries and other power supplies from `/sys/class/power_supply`.
//!
//! Some batteries report energy (µWh), others charge (µAh); both are kept
//! as reported. Health is computed only from a matching pair (energy full
//! vs energy design, or charge full vs charge design). Serial numbers,
//! Dell PPIDs and battery temperature are not read.

use sentinel_inventory::{
    Availability, BatteryInfo, BatteryStatus, Confidence, Field, ModuleOutcome, PowerSupplyInfo,
};

use crate::context::{DiscoveryContext, ModuleRun, Stopped};
use crate::fields;

const SUPPLIES: &str = "sys/class/power_supply";

/// Batteries and the other power supplies.
pub type PowerInventory = (Vec<BatteryInfo>, Vec<PowerSupplyInfo>);

pub fn parse_status(text: &str) -> Option<BatteryStatus> {
    Some(match text {
        "Charging" => BatteryStatus::Charging,
        "Discharging" => BatteryStatus::Discharging,
        "Not charging" => BatteryStatus::NotCharging,
        "Full" => BatteryStatus::Full,
        "Unknown" => BatteryStatus::Unknown,
        _ => return None,
    })
}

/// Full capacity as a percentage of design capacity.
pub fn health(full: &Field<u64>, design: &Field<u64>, source: &str) -> Option<Field<f64>> {
    let (full, design) = (*full.value()?, *design.value()?);
    if design == 0 || full == 0 {
        return None;
    }
    let percent = (full as f64 / design as f64 * 1000.0).round() / 10.0;
    Some(
        Field::detected(percent, source, Confidence::Derived)
            .with_note("full capacity divided by design capacity at scan time"),
    )
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<PowerInventory>, Stopped> {
    let root = &ctx.root;
    let Ok(names) = root.list(SUPPLIES) else {
        return Ok(ModuleRun::new(
            (Vec::new(), Vec::new()),
            ModuleOutcome::Unavailable,
            0,
            vec!["/sys/class/power_supply is not available".into()],
        ));
    };
    let mut batteries = Vec::new();
    let mut supplies = Vec::new();
    for name in names {
        ctx.check()?;
        let dir = format!("{SUPPLIES}/{name}");
        let kr = Confidence::KernelReported;
        let kind = fields::text(root, &format!("{dir}/type"), kr);
        if kind.value().map(String::as_str) != Some("Battery") {
            supplies.push(PowerSupplyInfo {
                discovered_at: ctx.now,
                name,
                online: fields::flag(root, &format!("{dir}/online"), kr),
                kind,
            });
            continue;
        }
        let u64f = |attr: &str| fields::number::<u64>(root, &format!("{dir}/{attr}"), kr);
        let i64f = |attr: &str| fields::number::<i64>(root, &format!("{dir}/{attr}"), kr);
        let text = |attr: &str| fields::text(root, &format!("{dir}/{attr}"), kr);
        let energy_full = u64f("energy_full");
        let energy_design = u64f("energy_full_design");
        let charge_full = u64f("charge_full");
        let charge_design = u64f("charge_full_design");
        let health_percent = health(&energy_full, &energy_design, &root.display(&dir))
            .or_else(|| health(&charge_full, &charge_design, &root.display(&dir)))
            .unwrap_or_else(|| {
                Field::missing(Availability::Unavailable, Some(root.display(&dir)))
                    .with_note("full and design capacity are not both reported in the same unit")
            });
        let status = match root.read_trimmed(&format!("{dir}/status")) {
            Ok(raw) => match parse_status(&raw) {
                Some(s) => Field::detected(s, root.display(&format!("{dir}/status")), kr),
                None => Field::missing(
                    Availability::Invalid,
                    Some(root.display(&format!("{dir}/status"))),
                )
                .with_note(format!("unrecognised status {raw:?}")),
            },
            Err(e) => fields::missing(root, &format!("{dir}/status"), &e),
        };
        let cycles = fields::number::<u32>(root, &format!("{dir}/cycle_count"), kr);
        let cycles = if cycles.value() == Some(&0) {
            cycles.with_note("0 often means the firmware does not count cycles")
        } else {
            cycles
        };
        batteries.push(BatteryInfo {
            discovered_at: ctx.now,
            scope: text("scope"),
            manufacturer: fields::text(
                root,
                &format!("{dir}/manufacturer"),
                Confidence::FirmwareReported,
            ),
            model_name: fields::text(
                root,
                &format!("{dir}/model_name"),
                Confidence::FirmwareReported,
            ),
            technology: text("technology").validate(
                |t| t != "Unknown",
                "the battery does not report its chemistry",
            ),
            present: fields::flag(root, &format!("{dir}/present"), kr),
            status,
            capacity_percent: fields::number::<u8>(root, &format!("{dir}/capacity"), kr),
            capacity_level: text("capacity_level"),
            energy_now_uwh: u64f("energy_now"),
            energy_full_uwh: energy_full,
            energy_full_design_uwh: energy_design,
            charge_now_uah: u64f("charge_now"),
            charge_full_uah: charge_full,
            charge_full_design_uah: charge_design,
            voltage_now_uv: u64f("voltage_now"),
            voltage_min_design_uv: u64f("voltage_min_design"),
            current_now_ua: i64f("current_now"),
            power_now_uw: i64f("power_now"),
            cycle_count: cycles,
            health_percent,
            name,
        });
    }
    let mut notes = Vec::new();
    if batteries.is_empty() {
        notes.push("No battery was found (normal for desktops).".into());
    }
    let outcome = ModuleOutcome::Completed;
    let count = batteries.len() + supplies.len();
    Ok(ModuleRun::new((batteries, supplies), outcome, count, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        assert_eq!(
            parse_status("Not charging"),
            Some(BatteryStatus::NotCharging)
        );
        assert_eq!(
            parse_status("Discharging"),
            Some(BatteryStatus::Discharging)
        );
        assert_eq!(parse_status("Exploding"), None);
    }

    #[test]
    fn health_needs_a_matching_pair() {
        let f = |v: u64| Field::detected(v, "x", Confidence::KernelReported);
        let none: Field<u64> = Field::missing(Availability::Unsupported, None);
        assert_eq!(
            health(&f(2_988_000), &f(3_645_000), "b").unwrap().value(),
            Some(&82.0)
        );
        assert!(health(&f(1), &none, "b").is_none());
        assert!(health(&f(1), &f(0), "b").is_none());
    }
}
