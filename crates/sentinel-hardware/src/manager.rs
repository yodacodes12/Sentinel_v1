//! Runs the discovery modules in order and assembles the inventory.

use std::time::Instant;

use sentinel_inventory::fingerprint::fingerprint;
use sentinel_inventory::{
    INVENTORY_SCHEMA_VERSION, ModuleId, ModuleOutcome, ModuleReport, SystemInventory,
};

use crate::context::{DiscoveryContext, ModuleRun, Stopped};
use crate::modules::*;

/// Progress notifications for the caller (the daemon shows them live).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ModuleStarted(ModuleId),
    ModuleFinished(ModuleReport),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScanOutcome {
    /// Always present; after a stop it holds what was found so far, with
    /// the remaining modules marked cancelled.
    pub inventory: SystemInventory,
    pub stopped: Option<Stopped>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn discover(ctx: &DiscoveryContext, on_event: &mut dyn FnMut(Event)) -> ScanOutcome {
    let started = Instant::now();
    let mut inventory = SystemInventory {
        schema_version: INVENTORY_SCHEMA_VERSION,
        discovered_at: ctx.now,
        duration_ms: 0,
        root_override: (!ctx.root.is_host()).then(|| ctx.root.root().display().to_string()),
        fingerprint: None,
        system: None,
        firmware: None,
        motherboard: None,
        processors: Vec::new(),
        memory: None,
        graphics: Vec::new(),
        storage: Vec::new(),
        batteries: Vec::new(),
        power_supplies: Vec::new(),
        network: Vec::new(),
        usb_devices: Vec::new(),
        pci_devices: Vec::new(),
        sensors: Vec::new(),
        thermal_zones: Vec::new(),
        discovery_status: Vec::new(),
    };
    let mut stopped = None;

    for module in ModuleId::HARDWARE {
        if stopped.is_none() {
            stopped = ctx.check().err();
        }
        if stopped.is_some() {
            let report = cancelled(module);
            on_event(Event::ModuleFinished(report.clone()));
            inventory.discovery_status.push(report);
            continue;
        }
        on_event(Event::ModuleStarted(module));
        let t = Instant::now();
        let result = run(ctx, module, &mut inventory);
        let report = match result {
            Ok(report) => report,
            Err(stop) => {
                stopped = Some(stop);
                cancelled(module)
            }
        };
        let report = ModuleReport {
            duration_ms: t.elapsed().as_millis() as u64,
            finished_at: now_secs(),
            ..report
        };
        on_event(Event::ModuleFinished(report.clone()));
        inventory.discovery_status.push(report);
    }

    inventory.fingerprint = Some(compute_fingerprint(&inventory));
    inventory.duration_ms = started.elapsed().as_millis() as u64;
    ScanOutcome { inventory, stopped }
}

fn cancelled(module: ModuleId) -> ModuleReport {
    ModuleReport {
        module,
        outcome: ModuleOutcome::Cancelled,
        items: 0,
        duration_ms: 0,
        finished_at: now_secs(),
        notes: vec!["the scan stopped before this module ran".into()],
    }
}

/// Run one module and store its result. Returns its report (without
/// timing) or the reason the scan stopped.
fn run(
    ctx: &DiscoveryContext,
    module: ModuleId,
    inv: &mut SystemInventory,
) -> Result<ModuleReport, Stopped> {
    fn take<T>(module: ModuleId, run: ModuleRun<T>, slot: &mut T) -> ModuleReport {
        let report = run.report(module, 0, 0);
        *slot = run.value;
        report
    }
    Ok(match module {
        ModuleId::SystemIdentity => take(module, system_identity::discover(ctx), &mut inv.system),
        ModuleId::Firmware => take(module, firmware::discover(ctx), &mut inv.firmware),
        ModuleId::Motherboard => take(module, motherboard::discover(ctx), &mut inv.motherboard),
        ModuleId::Cpu => take(module, cpu::discover(ctx)?, &mut inv.processors),
        ModuleId::Memory => take(module, memory::discover(ctx), &mut inv.memory),
        ModuleId::Gpu => take(module, gpu::discover(ctx)?, &mut inv.graphics),
        ModuleId::Storage => take(module, storage::discover(ctx)?, &mut inv.storage),
        ModuleId::Battery => {
            let run = battery::discover(ctx)?;
            let report = run.report(module, 0, 0);
            (inv.batteries, inv.power_supplies) = run.value;
            report
        }
        ModuleId::Network => take(module, network::discover(ctx)?, &mut inv.network),
        ModuleId::Usb => take(module, usb::discover(ctx)?, &mut inv.usb_devices),
        ModuleId::Pci => take(module, pci::discover(ctx)?, &mut inv.pci_devices),
        ModuleId::Sensors => {
            let run = sensors::discover(ctx)?;
            let report = run.report(module, 0, 0);
            (inv.sensors, inv.thermal_zones) = run.value;
            report
        }
        _ => unreachable!("not a hardware module"),
    })
}

fn compute_fingerprint(inv: &SystemInventory) -> sentinel_inventory::HardwareFingerprint {
    fn s(f: Option<&sentinel_inventory::Field<String>>) -> Option<&str> {
        f.and_then(|f| f.value()).map(String::as_str)
    }
    let system = inv.system.as_ref();
    let board = inv.motherboard.as_ref();
    let cpu = inv.processors.first();
    fingerprint(
        &[
            ("manufacturer", s(system.map(|x| &x.manufacturer))),
            ("product", s(system.map(|x| &x.product_name))),
            (
                "board_manufacturer",
                s(board.map(|b| &b.board_manufacturer)),
            ),
            ("board_name", s(board.map(|b| &b.board_name))),
            ("board_revision", s(board.map(|b| &b.board_revision))),
            ("cpu_model", s(cpu.map(|c| &c.model_name))),
        ],
        &inv.pci_devices
            .iter()
            .map(|d| (d.vendor_id, d.device_id))
            .collect::<Vec<_>>(),
    )
}
