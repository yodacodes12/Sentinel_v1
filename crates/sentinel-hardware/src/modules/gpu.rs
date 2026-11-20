//! Graphics devices: PCI display-class functions plus their DRM nodes.
//!
//! Integrated vs dedicated is an inference from PCI identity and placement
//! and is labelled as such. Memory the driver calls "VRAM" on an integrated
//! GPU is system RAM set aside for it, and is reported that way.

use sentinel_inventory::{
    Availability, Confidence, Field, GpuInfo, GpuKind, GpuMemory, GpuMemoryKind, ModuleOutcome,
};

use crate::context::{DiscoveryContext, ModuleRun, Stopped};
use crate::fields;
use crate::modules::pci::{self, PciRaw};

const VENDOR_AMD_GPU: u16 = 0x1002;
const VENDOR_AMD: u16 = 0x1022;
const VENDOR_INTEL: u16 = 0x8086;
const VENDOR_NVIDIA: u16 = 0x10de;
const VIRTUAL_VENDORS: &[u16] = &[0x1af4, 0x15ad, 0x1234, 0x1b36, 0x80ee, 0x1414];

/// Classify a display device. `siblings` are the other functions in the
/// same PCI slot.
pub fn classify(raw: &PciRaw, siblings: &[PciRaw]) -> Option<(GpuKind, &'static str)> {
    let bus = raw.address.get(5..7).unwrap_or("");
    Some(match raw.vendor_id {
        v if VIRTUAL_VENDORS.contains(&v) => {
            (GpuKind::Virtual, "PCI vendor is a virtual-machine device")
        }
        VENDOR_NVIDIA => (
            GpuKind::Dedicated,
            "NVIDIA PCI graphics devices are discrete",
        ),
        VENDOR_INTEL if bus == "00" => (
            GpuKind::Integrated,
            "Intel graphics on the root bus is the processor's integrated GPU",
        ),
        VENDOR_INTEL => (
            GpuKind::Dedicated,
            "Intel graphics behind a PCIe bridge is a discrete card",
        ),
        VENDOR_AMD_GPU if siblings.iter().any(|s| s.vendor_id == VENDOR_AMD) => (
            GpuKind::Integrated,
            "shares its PCI slot with AMD platform functions, as an APU's integrated GPU does",
        ),
        VENDOR_AMD_GPU => (
            GpuKind::Dedicated,
            "no AMD platform functions share its PCI slot, as on a discrete card",
        ),
        _ => return None,
    })
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<Vec<GpuInfo>>, Stopped> {
    let Some(addresses) = pci::addresses(ctx) else {
        return Ok(ModuleRun::new(
            Vec::new(),
            ModuleOutcome::Unavailable,
            0,
            vec!["no PCI bus in sysfs; graphics devices cannot be enumerated".into()],
        ));
    };
    let all: Vec<PciRaw> = addresses.iter().filter_map(|a| pci::read(ctx, a)).collect();
    let mut gpus = Vec::new();
    let mut notes = Vec::new();
    for raw in all.iter().filter(|r| r.class_code >> 16 == 0x03) {
        ctx.check()?;
        let slot = &raw.address[..raw.address.len() - 2];
        let siblings: Vec<PciRaw> = all
            .iter()
            .filter(|o| o.address != raw.address && o.address.starts_with(slot))
            .cloned()
            .collect();
        let kind = match classify(raw, &siblings) {
            Some((kind, why)) => {
                Field::detected(kind, "PCI identity and placement", Confidence::Inferred)
                    .with_note(why)
            }
            None => Field::missing(Availability::Unknown, None)
                .with_note("Sentinel cannot tell whether this GPU is integrated or dedicated"),
        };
        let drm_dir = format!("{}/drm", raw.dir);
        let drm_nodes: Vec<String> = ctx
            .root
            .list(&drm_dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.starts_with("card") || n.starts_with("renderD"))
            .collect();
        let vram = ctx
            .root
            .read_trimmed(&format!("{}/mem_info_vram_total", raw.dir))
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0);
        let memory = match (vram, kind.value()) {
            (Some(bytes), Some(GpuKind::Integrated)) => Field::detected(
                GpuMemory {
                    kind: GpuMemoryKind::ReservedSystemMemory,
                    bytes,
                },
                ctx.root
                    .display(&format!("{}/mem_info_vram_total", raw.dir)),
                Confidence::KernelReported,
            )
            .with_note("system RAM reserved for the integrated GPU, not separate memory"),
            (Some(bytes), Some(GpuKind::Dedicated)) => Field::detected(
                GpuMemory {
                    kind: GpuMemoryKind::DedicatedVram,
                    bytes,
                },
                ctx.root
                    .display(&format!("{}/mem_info_vram_total", raw.dir)),
                Confidence::KernelReported,
            ),
            (Some(_), _) => Field::missing(Availability::Unknown, None)
                .with_note("the driver reports memory, but it is unclear whether it is dedicated"),
            (None, _) => {
                Field::missing(Availability::Unsupported, Some(ctx.root.display(&raw.dir)))
                    .with_note("the driver does not report graphics memory")
            }
        };
        let (vendor_name, model_name, subsystem_name) = pci::names(ctx, raw);
        gpus.push(GpuInfo {
            discovered_at: ctx.now,
            pci_address: raw.address.clone(),
            vendor_id: raw.vendor_id,
            device_id: raw.device_id,
            subsystem_vendor_id: raw.subsystem_vendor_id,
            subsystem_device_id: raw.subsystem_device_id,
            revision: raw.revision,
            vendor_name,
            model_name,
            subsystem_name,
            driver: pci::driver_field(raw, ctx),
            kind,
            boot_display: fields::flag(
                &ctx.root,
                &format!("{}/boot_vga", raw.dir),
                Confidence::FirmwareReported,
            ),
            drm_nodes,
            memory,
        });
    }
    if gpus.is_empty() {
        notes.push("No PCI graphics device was found. The display may use a firmware framebuffer or software rendering.".into());
    }
    let integrated = gpus
        .iter()
        .filter(|g| g.kind.value() == Some(&GpuKind::Integrated))
        .count();
    let dedicated = gpus
        .iter()
        .filter(|g| g.kind.value() == Some(&GpuKind::Dedicated))
        .count();
    if integrated > 0 && dedicated > 0 {
        notes.push("Hybrid graphics: integrated and dedicated GPUs are both present.".into());
    }
    let outcome = if gpus.is_empty() {
        ModuleOutcome::Unavailable
    } else if gpus.iter().all(|g| g.kind.is_detected()) {
        ModuleOutcome::Completed
    } else {
        ModuleOutcome::Partial
    };
    let count = gpus.len();
    Ok(ModuleRun::new(gpus, outcome, count, notes))
}
