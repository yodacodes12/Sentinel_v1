//! BIOS/UEFI firmware identity from DMI, and the boot mode.

use sentinel_inventory::{Availability, BootMode, Confidence, Field, FirmwareInfo};

use crate::context::{DiscoveryContext, ModuleRun, outcome_for};
use crate::modules::system_identity::{DMI, dmi};

pub fn discover(ctx: &DiscoveryContext) -> ModuleRun<Option<FirmwareInfo>> {
    let boot_mode = if ctx.root.is_dir("sys/firmware/efi") {
        Field::detected(
            BootMode::Uefi,
            "/sys/firmware/efi",
            Confidence::KernelReported,
        )
    } else if ctx.root.is_dir("sys/firmware") {
        Field::detected(BootMode::Legacy, "/sys/firmware", Confidence::Inferred)
            .with_note("/sys/firmware/efi is absent, so the system was not booted through UEFI")
    } else {
        Field::missing(Availability::Unsupported, Some("/sys/firmware".into()))
    };
    let info = FirmwareInfo {
        discovered_at: ctx.now,
        bios_vendor: dmi(ctx, "bios_vendor"),
        bios_version: dmi(ctx, "bios_version"),
        bios_release_date: dmi(ctx, "bios_date"),
        bios_revision: dmi(ctx, "bios_release"),
        embedded_controller_revision: dmi(ctx, "ec_firmware_release"),
        boot_mode,
    };
    let key = [
        info.bios_vendor.is_detected(),
        info.bios_version.is_detected(),
        info.bios_release_date.is_detected(),
    ];
    let missing = key.iter().filter(|k| !**k).count();
    let mut notes = Vec::new();
    if !ctx.root.is_dir(DMI) {
        notes.push("No DMI/SMBIOS firmware information on this system.".into());
    } else if missing > 0 {
        notes.push("Some BIOS fields are missing or placeholders.".into());
    }
    let outcome = if missing == key.len() {
        sentinel_inventory::ModuleOutcome::Unavailable
    } else {
        outcome_for(missing)
    };
    ModuleRun::new(Some(info), outcome, 1, notes)
}
