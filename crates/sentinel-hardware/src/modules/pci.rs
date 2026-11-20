//! PCI devices from `/sys/bus/pci/devices`.
//!
//! The parent of each device is its upstream bridge in the sysfs device
//! path. That is bus topology only; it says nothing about traces on the
//! board.

use sentinel_inventory::{Availability, Confidence, Field, ModuleOutcome, PciDevice};

use crate::context::{DiscoveryContext, ModuleRun, PCI_DEVICES, Stopped, is_pci_address};
use crate::parsers::{classes, hex_u16, hex_u32};

/// Raw identifiers of one PCI function.
#[derive(Debug, Clone)]
pub struct PciRaw {
    pub address: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
    pub revision: Option<u8>,
    pub class_code: u32,
    pub driver: Option<String>,
    pub parent: Option<String>,
    pub dir: String,
}

pub fn read(ctx: &DiscoveryContext, address: &str) -> Option<PciRaw> {
    let dir = format!("{PCI_DEVICES}/{address}");
    let read = |name: &str| ctx.root.read_trimmed(&format!("{dir}/{name}")).ok();
    let parent = ctx.root.canonical(&dir).and_then(|path| {
        let mut parts: Vec<&str> = path.split('/').collect();
        parts.pop();
        parts
            .into_iter()
            .rev()
            .find(|p| is_pci_address(p))
            .map(str::to_string)
    });
    Some(PciRaw {
        address: address.to_string(),
        vendor_id: read("vendor").and_then(|v| hex_u16(&v))?,
        device_id: read("device").and_then(|v| hex_u16(&v))?,
        subsystem_vendor_id: read("subsystem_vendor")
            .and_then(|v| hex_u16(&v))
            .filter(|v| *v != 0),
        subsystem_device_id: read("subsystem_device").and_then(|v| hex_u16(&v)),
        revision: read("revision")
            .and_then(|v| hex_u32(&v))
            .and_then(|v| u8::try_from(v).ok()),
        class_code: read("class").and_then(|v| hex_u32(&v)).unwrap_or(0),
        driver: ctx.root.link_name(&format!("{dir}/driver")),
        parent,
        dir,
    })
}

pub fn addresses(ctx: &DiscoveryContext) -> Option<Vec<String>> {
    ctx.root.list(PCI_DEVICES).ok()
}

pub fn names(
    ctx: &DiscoveryContext,
    raw: &PciRaw,
) -> (Field<String>, Field<String>, Field<String>) {
    let source = ctx.pci_ids_source();
    let Some(names) = ctx.pci_names() else {
        let none = || {
            Field::missing(Availability::Unavailable, Some("pci.ids".into()))
                .with_note("the PCI ID database (hwdata) is not installed")
        };
        return (none(), none(), none());
    };
    let found = |name: Option<&String>| match name {
        Some(n) => Field::detected(n.clone(), source.clone(), Confidence::Database),
        None => Field::missing(Availability::Unknown, Some(source.clone()))
            .with_note("this ID is not in the installed database"),
    };
    let sub = match (raw.subsystem_vendor_id, raw.subsystem_device_id) {
        (Some(sv), Some(sd)) => names
            .subsystems
            .get(&(raw.vendor_id, raw.device_id, sv, sd)),
        _ => None,
    };
    (
        found(names.vendors.get(&raw.vendor_id)),
        found(names.devices.get(&(raw.vendor_id, raw.device_id))),
        found(sub),
    )
}

pub fn driver_field(raw: &PciRaw, ctx: &DiscoveryContext) -> Field<String> {
    match &raw.driver {
        Some(d) => Field::detected(
            d.clone(),
            ctx.root.display(&format!("{}/driver", raw.dir)),
            Confidence::KernelReported,
        ),
        None => Field::missing(Availability::Unavailable, Some(ctx.root.display(&raw.dir)))
            .with_note("no kernel driver is bound"),
    }
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<Vec<PciDevice>>, Stopped> {
    let Some(list) = addresses(ctx) else {
        return Ok(ModuleRun::new(
            Vec::new(),
            ModuleOutcome::Unavailable,
            0,
            vec!["this system has no PCI bus in sysfs".into()],
        ));
    };
    let mut devices = Vec::new();
    let mut unreadable = 0;
    for address in list {
        ctx.check()?;
        let Some(raw) = read(ctx, &address) else {
            unreadable += 1;
            continue;
        };
        let (vendor_name, device_name, _) = names(ctx, &raw);
        devices.push(PciDevice {
            discovered_at: ctx.now,
            address: raw.address.clone(),
            vendor_id: raw.vendor_id,
            device_id: raw.device_id,
            subsystem_vendor_id: raw.subsystem_vendor_id,
            subsystem_device_id: raw.subsystem_device_id,
            revision: raw.revision,
            class_code: raw.class_code,
            class_name: classes::pci_class_name(raw.class_code),
            role: classes::pci_role(raw.class_code),
            vendor_name,
            device_name,
            driver: driver_field(&raw, ctx),
            parent: raw.parent.clone(),
        });
    }
    let mut notes = Vec::new();
    if unreadable > 0 {
        notes.push(format!(
            "{unreadable} PCI device(s) could not be identified"
        ));
    }
    if ctx.pci_names().is_none() {
        notes.push("PCI ID database not installed: devices are shown by numeric ID only".into());
    }
    let outcome = if notes.is_empty() {
        ModuleOutcome::Completed
    } else {
        ModuleOutcome::Partial
    };
    let count = devices.len();
    Ok(ModuleRun::new(devices, outcome, count, notes))
}
