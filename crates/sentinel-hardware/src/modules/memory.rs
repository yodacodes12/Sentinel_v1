//! Memory: installed (firmware), kernel-managed and usable memory, swap,
//! modules and NUMA nodes.
//!
//! Installed RAM is never calculated from usage. It comes only from SMBIOS
//! memory-device records, which most systems expose to root only; without
//! them it is reported as permission denied.

use sentinel_inventory::{
    Availability, Confidence, Field, MemoryInfo, MemoryModule, MemorySnapshot, ModuleOutcome,
    NumaNode,
};

use crate::context::{DiscoveryContext, ModuleRun};
use crate::fields;
use crate::parsers::{cpu_list, hex_u32, meminfo, smbios};
use crate::source::ReadError;

const DMI_ENTRIES: &str = "sys/firmware/dmi/entries";
const MEMORY_BLOCKS: &str = "sys/devices/system/memory";
const NODES: &str = "sys/devices/system/node";

pub fn discover(ctx: &DiscoveryContext) -> ModuleRun<Option<MemoryInfo>> {
    let root = &ctx.root;
    let mut notes = Vec::new();

    let info = match root.read("proc/meminfo") {
        Ok(text) => Some(meminfo(&text)),
        Err(_) => {
            notes.push("/proc/meminfo is not readable".into());
            None
        }
    };
    let src = "/proc/meminfo";
    let mem = |key: &str| match info.as_ref().and_then(|m| m.get(key)) {
        Some(v) => Field::detected(*v, src, Confidence::KernelReported),
        None => Field::missing(Availability::Unavailable, Some(src.into()))
            .with_note(format!("{key} is not listed")),
    };
    let usable = mem("MemTotal");
    let available = mem("MemAvailable");
    let used = match (usable.value(), available.value()) {
        (Some(total), Some(avail)) => {
            Field::detected(total.saturating_sub(*avail), src, Confidence::Derived)
                .with_note("MemTotal minus MemAvailable at scan time")
        }
        _ => Field::missing(Availability::Unavailable, Some(src.into())),
    };
    let snapshot = MemorySnapshot {
        available_bytes: available,
        free_bytes: mem("MemFree"),
        cached_bytes: mem("Cached"),
        used_bytes: used,
    };

    // Online memory blocks: physical memory the kernel manages.
    let kernel_physical = match root
        .read_trimmed(&format!("{MEMORY_BLOCKS}/block_size_bytes"))
        .ok()
        .and_then(|v| {
            hex_u32(&v)
                .map(u64::from)
                .or_else(|| u64::from_str_radix(v.trim(), 16).ok())
        }) {
        Some(block) => {
            let online = root
                .list(MEMORY_BLOCKS)
                .unwrap_or_default()
                .into_iter()
                .filter(|n| n.starts_with("memory") && n[6..].chars().all(|c| c.is_ascii_digit()))
                .filter(|n| {
                    root.read_trimmed(&format!("{MEMORY_BLOCKS}/{n}/online"))
                        .map(|v| v == "1")
                        .unwrap_or(false)
                })
                .count() as u64;
            if online > 0 {
                Field::detected(
                    block * online,
                    format!("/{MEMORY_BLOCKS}"),
                    Confidence::KernelReported,
                )
                .with_note("online memory blocks; excludes memory reserved by firmware")
            } else {
                Field::missing(Availability::Unavailable, Some(format!("/{MEMORY_BLOCKS}")))
            }
        }
        None => Field::missing(Availability::Unsupported, Some(format!("/{MEMORY_BLOCKS}")))
            .with_note("this kernel does not expose memory blocks"),
    };

    let (modules, module_error) = memory_modules(ctx);
    let firmware_src = format!("/{DMI_ENTRIES}/17-*/raw");
    let restricted = |what: &str| -> Field<u64> {
        match &module_error {
            Some(e) => fields::missing(root, &format!("{DMI_ENTRIES}/17-0/raw"), e),
            None => Field::missing(Availability::Unavailable, Some(firmware_src.clone())),
        }
        .with_note(format!(
            "{what}: firmware memory records {}",
            match module_error {
                Some(ReadError::PermissionDenied) =>
                    "are readable only by administrators; Sentinel does not request elevated privileges",
                Some(ReadError::Missing) => "are not provided by this system",
                _ => "are not available",
            }
        ))
    };

    let (installed, memory_type, speed, slots_total, slots_populated, modules_field) =
        match &modules {
            Some(list) if !list.is_empty() => {
                let populated: Vec<&MemoryModule> = list.iter().filter(|m| m.populated).collect();
                let total: u64 = populated.iter().filter_map(|m| m.size_bytes).sum();
                let types: std::collections::BTreeSet<&str> = populated
                    .iter()
                    .filter_map(|m| m.memory_type.as_deref())
                    .collect();
                let speed = populated
                    .iter()
                    .filter_map(|m| m.configured_speed_mts.or(m.speed_mts))
                    .min();
                let fw = Confidence::FirmwareReported;
                (
                    if total > 0 {
                        Field::detected(total, firmware_src.clone(), fw)
                    } else {
                        Field::missing(Availability::Unavailable, Some(firmware_src.clone()))
                    },
                    match types.len() {
                        1 => Field::detected(
                            types.into_iter().next().unwrap_or_default().to_string(),
                            firmware_src.clone(),
                            fw,
                        ),
                        0 => Field::missing(Availability::Unavailable, Some(firmware_src.clone())),
                        _ => Field::missing(Availability::Invalid, Some(firmware_src.clone()))
                            .with_note("modules report different memory types"),
                    },
                    match speed {
                        Some(s) => Field::detected(s, firmware_src.clone(), fw),
                        None => {
                            Field::missing(Availability::Unavailable, Some(firmware_src.clone()))
                        }
                    },
                    Field::detected(list.len() as u32, firmware_src.clone(), fw),
                    Field::detected(populated.len() as u32, firmware_src.clone(), fw),
                    Field::detected(list.clone(), firmware_src.clone(), fw),
                )
            }
            _ => {
                notes.push(
                    "Installed RAM, memory type, speed and modules are not available to this user."
                        .into(),
                );
                (
                    restricted("installed RAM"),
                    restricted("memory type").map(|_| String::new()),
                    restricted("memory speed").map(|_| 0u32),
                    restricted("slots").map(|_| 0u32),
                    restricted("populated slots").map(|_| 0u32),
                    restricted("modules").map(|_| Vec::new()),
                )
            }
        };

    let numa_nodes = numa(ctx);
    let info = MemoryInfo {
        discovered_at: ctx.now,
        installed_bytes: installed,
        kernel_physical_bytes: kernel_physical,
        usable_bytes: usable,
        snapshot,
        swap_total_bytes: mem("SwapTotal"),
        memory_type,
        speed_mts: speed,
        slots_total,
        slots_populated,
        modules: modules_field,
        numa_nodes,
    };
    let outcome = if !info.usable_bytes.is_detected() {
        ModuleOutcome::Unavailable
    } else if notes.is_empty() {
        ModuleOutcome::Completed
    } else {
        ModuleOutcome::Partial
    };
    ModuleRun::new(Some(info), outcome, 1, notes)
}

/// SMBIOS type 17 records. `(None, Some(error))` when they cannot be read.
fn memory_modules(ctx: &DiscoveryContext) -> (Option<Vec<MemoryModule>>, Option<ReadError>) {
    let entries = match ctx.root.list(DMI_ENTRIES) {
        Ok(e) => e,
        Err(e) => return (None, Some(e)),
    };
    let mut modules = Vec::new();
    let mut error = None;
    for entry in entries.iter().filter(|e| e.starts_with("17-")) {
        match ctx.root.read_bytes(&format!("{DMI_ENTRIES}/{entry}/raw")) {
            Ok(raw) => modules.extend(smbios::memory_device(&raw)),
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }
    match error {
        Some(e) => (None, Some(e)),
        None if modules.is_empty() => (None, Some(ReadError::Missing)),
        None => (Some(modules), None),
    }
}

fn numa(ctx: &DiscoveryContext) -> Vec<NumaNode> {
    ctx.root
        .list(NODES)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            let id: u32 = name.strip_prefix("node")?.parse().ok()?;
            let dir = format!("{NODES}/{name}");
            let cpus = ctx
                .root
                .read_trimmed(&format!("{dir}/cpulist"))
                .ok()
                .and_then(|v| cpu_list(&v))
                .unwrap_or_default();
            // "Node 0 MemTotal:  6777356 kB"
            let memory_bytes = ctx
                .root
                .read(&format!("{dir}/meminfo"))
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        let rest = line.split_once("MemTotal:")?.1;
                        let kib: u64 = rest.split_whitespace().next()?.parse().ok()?;
                        Some(kib * 1024)
                    })
                });
            Some(NumaNode {
                id,
                cpus,
                memory_bytes,
            })
        })
        .collect()
}
