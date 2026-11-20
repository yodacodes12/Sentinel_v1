//! Which information sources exist here and whether this user can read
//! them. Cheap: only existence and open-for-read checks.

use sentinel_inventory::{CapabilityStatus, HardwareCapabilities, InterfaceCapability};

use crate::source::{Access, SysRoot};

const INTERFACES: &[(&str, &str, &str, Option<&str>)] = &[
    // (id, description, directory, a restricted file to probe)
    (
        "dmi",
        "System and board identity (DMI/SMBIOS)",
        "sys/class/dmi/id",
        Some("sys/firmware/dmi/tables/DMI"),
    ),
    (
        "cpu_topology",
        "CPU topology",
        "sys/devices/system/cpu",
        None,
    ),
    ("cpuinfo", "CPU identification", "proc/cpuinfo", None),
    ("meminfo", "Memory usage counters", "proc/meminfo", None),
    (
        "memory_blocks",
        "Physical memory blocks",
        "sys/devices/system/memory",
        None,
    ),
    (
        "smbios_memory",
        "Memory module records (SMBIOS type 17)",
        "sys/firmware/dmi/entries",
        Some("sys/firmware/dmi/entries/17-0/raw"),
    ),
    ("pci", "PCI devices", "sys/bus/pci/devices", None),
    ("usb", "USB devices", "sys/bus/usb/devices", None),
    ("block", "Block devices", "sys/block", None),
    (
        "power_supply",
        "Batteries and power supplies",
        "sys/class/power_supply",
        None,
    ),
    (
        "hwmon",
        "Hardware monitoring chips",
        "sys/class/hwmon",
        None,
    ),
    ("thermal", "Thermal zones", "sys/class/thermal", None),
    ("net", "Network interfaces", "sys/class/net", None),
    ("drm", "Graphics (DRM)", "sys/class/drm", None),
    (
        "pci_ids",
        "PCI ID database",
        "usr/share/hwdata/pci.ids",
        None,
    ),
    (
        "os_release",
        "Distribution identification",
        "etc/os-release",
        None,
    ),
];

pub fn detect(root: &SysRoot, now: u64) -> HardwareCapabilities {
    let interfaces = INTERFACES
        .iter()
        .map(|(id, description, path, restricted)| {
            let present = root.exists(path);
            let (status, note) = if !present {
                (CapabilityStatus::Missing, None)
            } else {
                match restricted.map(|r| root.access(r)) {
                    Some(Access::PermissionDenied) => (
                        CapabilityStatus::Partial,
                        Some(format!(
                            "{} is readable only by administrators",
                            root.display(restricted.unwrap_or(path))
                        )),
                    ),
                    _ => match root.access(path) {
                        Access::PermissionDenied => (CapabilityStatus::PermissionDenied, None),
                        _ => (CapabilityStatus::Available, None),
                    },
                }
            };
            InterfaceCapability {
                id: (*id).into(),
                description: (*description).into(),
                path: root.display(path),
                status,
                note,
            }
        })
        .collect();
    HardwareCapabilities {
        checked_at: now,
        interfaces,
    }
}
