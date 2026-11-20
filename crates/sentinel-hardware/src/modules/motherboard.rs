//! Motherboard identity. The laptop model and the board model are read
//! separately and never substituted for each other.

use sentinel_inventory::{BoardIdentity, IdentificationLevel, ModuleOutcome};

use crate::context::{DiscoveryContext, ModuleRun};
use crate::modules::system_identity::dmi;

pub fn discover(ctx: &DiscoveryContext) -> ModuleRun<Option<BoardIdentity>> {
    let board_manufacturer = dmi(ctx, "board_vendor");
    let board_name = dmi(ctx, "board_name");
    let system_model = dmi(ctx, "product_name");
    let identification = BoardIdentity::assess(&board_manufacturer, &board_name, &system_model);
    let board_revision = dmi(ctx, "board_version");
    let board_revision = if board_revision.is_detected() {
        board_revision.with_note("as reported by firmware; not verified against documentation")
    } else {
        board_revision
    };
    let (outcome, notes) = match identification.level {
        IdentificationLevel::BoardIdentified => (ModuleOutcome::Completed, Vec::new()),
        IdentificationLevel::SystemOnly => (
            ModuleOutcome::Partial,
            vec!["Firmware does not report a usable board name.".to_string()],
        ),
        IdentificationLevel::Unidentified => (
            ModuleOutcome::Unavailable,
            vec!["No usable board or system identifiers.".to_string()],
        ),
    };
    let identity = BoardIdentity {
        discovered_at: ctx.now,
        system_manufacturer: dmi(ctx, "sys_vendor"),
        system_model,
        system_family: dmi(ctx, "product_family"),
        system_sku: dmi(ctx, "product_sku"),
        board_manufacturer,
        board_name,
        board_revision,
        bios_vendor: dmi(ctx, "bios_vendor"),
        bios_version: dmi(ctx, "bios_version"),
        bios_release_date: dmi(ctx, "bios_date"),
        identification,
    };
    ModuleRun::new(Some(identity), outcome, 1, notes)
}
