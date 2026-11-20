//! Block devices from `/sys/block`, with partitions as children.
//!
//! Only sysfs attributes are read. Device nodes under `/dev` are never
//! opened, nothing is mounted, and no SMART or other command is sent to a
//! drive. Serial numbers and WWNs are not read.
//!
//! How each device was in use (mounts, swap, device-mapper layering) comes
//! from `/proc/mounts`, `/proc/swaps` and sysfs `holders/` and `slaves/` at
//! scan time. Partition contents are never read, so the filesystem of a
//! device that is not mounted stays unknown.

use sentinel_inventory::{
    Availability, BlockUse, Confidence, Field, ModuleOutcome, Mount, Partition, StorageDevice,
    StorageInterface, StorageKind,
};

use crate::context::{DiscoveryContext, ModuleRun, Stopped, last_pci_address};
use crate::fields;
use crate::parsers::{self, MountEntry};

const BLOCK: &str = "sys/block";
const SECTOR: u64 = 512; // sysfs `size` is always in 512-byte units
const MOUNTS: &str = "proc/mounts";
const SWAPS: &str = "proc/swaps";

/// The mount and swap tables, read once per scan.
struct UseTables {
    mounts: Option<Vec<MountEntry>>,
    swaps: Option<Vec<String>>,
    mounts_src: String,
    swaps_src: String,
}

impl UseTables {
    fn read(ctx: &DiscoveryContext) -> Self {
        let root = &ctx.root;
        UseTables {
            mounts: root.read(MOUNTS).ok().map(|t| parsers::mounts(&t)),
            swaps: root.read(SWAPS).ok().map(|t| parsers::swap_devices(&t)),
            mounts_src: root.display(MOUNTS),
            swaps_src: root.display(SWAPS),
        }
    }

    /// Use of the block device known by these `/dev` paths.
    fn usage(&self, nodes: &[String], holders: Vec<String>) -> BlockUse {
        let kr = Confidence::KernelReported;
        let mounts = match &self.mounts {
            Some(table) => Field::detected(
                table
                    .iter()
                    .filter(|m| nodes.contains(&m.device))
                    .map(|m| Mount {
                        mount_point: m.mount_point.clone(),
                        fs_type: m.fs_type.clone(),
                        read_only: m.read_only,
                    })
                    .collect(),
                &self.mounts_src,
                kr,
            )
            .with_note("mount table at scan time"),
            None => Field::missing(Availability::Unavailable, Some(self.mounts_src.clone()))
                .with_note("the mount table could not be read"),
        };
        let swap = match &self.swaps {
            Some(list) => {
                Field::detected(list.iter().any(|d| nodes.contains(d)), &self.swaps_src, kr)
            }
            None => Field::missing(Availability::Unavailable, Some(self.swaps_src.clone())),
        };
        BlockUse {
            mounts,
            swap,
            holders,
        }
    }
}

pub fn interface_from_path(path: &str, name: &str) -> StorageInterface {
    if path.contains("/virtual/") {
        StorageInterface::Virtual
    } else if path.contains("/nvme") {
        StorageInterface::Nvme
    } else if path.contains("/usb") {
        StorageInterface::Usb
    } else if path.contains("/mmc_host/") || name.starts_with("mmcblk") {
        StorageInterface::Mmc
    } else if path.contains("/virtio") {
        StorageInterface::Virtio
    } else if path.contains("/ata") {
        StorageInterface::Sata
    } else if path.contains("/host") {
        StorageInterface::Scsi
    } else {
        StorageInterface::Other
    }
}

pub fn kind_for(
    interface: StorageInterface,
    name: &str,
    rotational: Option<bool>,
    removable: Option<bool>,
) -> StorageKind {
    match interface {
        StorageInterface::Virtual => StorageKind::Virtual,
        _ if name.starts_with("sr") => StorageKind::Optical,
        StorageInterface::Mmc => StorageKind::Flash,
        StorageInterface::Usb if removable == Some(true) || rotational == Some(false) => {
            StorageKind::Flash
        }
        _ => match rotational {
            Some(true) => StorageKind::Hdd,
            Some(false) => StorageKind::Ssd,
            None => StorageKind::Unknown,
        },
    }
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<Vec<StorageDevice>>, Stopped> {
    let root = &ctx.root;
    let Ok(names) = root.list(BLOCK) else {
        return Ok(ModuleRun::new(
            Vec::new(),
            ModuleOutcome::Unavailable,
            0,
            vec!["/sys/block is not available".into()],
        ));
    };
    let tables = UseTables::read(ctx);
    let mut devices = Vec::new();
    let mut unused_virtual = 0;
    for name in names {
        ctx.check()?;
        let dir = format!("{BLOCK}/{name}");
        let path = root.canonical(&dir).unwrap_or_default();
        let interface = interface_from_path(&path, &name);
        let sectors = root
            .read_trimmed(&format!("{dir}/size"))
            .ok()
            .and_then(|v| v.parse::<u64>().ok());
        if interface == StorageInterface::Virtual && sectors.unwrap_or(0) == 0 {
            unused_virtual += 1; // unused loop/zram slots
            continue;
        }
        let kr = Confidence::KernelReported;
        let rotational = fields::flag(root, &format!("{dir}/queue/rotational"), kr);
        let removable = fields::flag(root, &format!("{dir}/removable"), kr);
        let rotational = if rotational.is_detected() && interface == StorageInterface::Usb {
            rotational.with_note("USB bridges sometimes report this incorrectly")
        } else {
            rotational
        };
        let kind = kind_for(
            interface,
            &name,
            rotational.cloned_value(),
            removable.cloned_value(),
        );
        let capacity = match sectors {
            Some(s) => Field::detected(s * SECTOR, root.display(&format!("{dir}/size")), kr)
                .validate(|b| *b > 0, "the device reports no capacity (no medium?)"),
            None => Field::missing(
                Availability::Unavailable,
                Some(root.display(&format!("{dir}/size"))),
            ),
        };
        let device_text = |attr: &str| {
            let field = fields::text(root, &format!("{dir}/device/{attr}"), Confidence::Direct);
            if interface == StorageInterface::Virtual {
                field.with_note("virtual device")
            } else {
                field
            }
        };
        let firmware = {
            let nvme = fields::text(
                root,
                &format!("{dir}/device/firmware_rev"),
                Confidence::Direct,
            );
            if nvme.is_detected() {
                nvme
            } else {
                device_text("rev")
            }
        };

        let mut partitions = Vec::new();
        for child in root.list(&dir).unwrap_or_default() {
            if !child.starts_with(&name) || !root.exists(&format!("{dir}/{child}/partition")) {
                continue;
            }
            let read = |attr: &str| root.read_trimmed(&format!("{dir}/{child}/{attr}")).ok();
            let holders = root
                .list(&format!("{dir}/{child}/holders"))
                .unwrap_or_default();
            let node = format!("/dev/{child}");
            partitions.push(Partition {
                usage: tables.usage(std::slice::from_ref(&node), holders),
                device_path: node,
                number: read("partition").and_then(|v| v.parse().ok()),
                start_sector: read("start").and_then(|v| v.parse().ok()),
                size_bytes: read("size")
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|s| s * SECTOR),
                read_only: read("ro").map(|v| v == "1"),
                name: child,
            });
        }
        partitions.sort_by_key(|p| p.number);
        let mapper_name = root
            .read_trimmed(&format!("{dir}/dm/name"))
            .ok()
            .filter(|n| !n.is_empty());
        let mut nodes = vec![format!("/dev/{name}")];
        if let Some(mapper) = &mapper_name {
            nodes.push(format!("/dev/mapper/{mapper}"));
        }
        let usage = tables.usage(
            &nodes,
            root.list(&format!("{dir}/holders")).unwrap_or_default(),
        );

        devices.push(StorageDevice {
            discovered_at: ctx.now,
            device_path: format!("/dev/{name}"),
            sysfs_path: (!path.is_empty()).then(|| format!("/{path}")),
            kind,
            interface: Field::detected(interface, root.display(&dir), Confidence::Inferred)
                .with_note("from the device's position in the kernel device tree"),
            model: device_text("model"),
            vendor: device_text("vendor"),
            firmware_revision: firmware,
            capacity_bytes: capacity,
            logical_sector_bytes: fields::number(
                root,
                &format!("{dir}/queue/logical_block_size"),
                kr,
            ),
            physical_sector_bytes: fields::number(
                root,
                &format!("{dir}/queue/physical_block_size"),
                kr,
            ),
            rotational,
            removable,
            read_only: fields::flag(root, &format!("{dir}/ro"), kr),
            controller_pci_address: last_pci_address(&path),
            partitions,
            usage,
            slaves: root.list(&format!("{dir}/slaves")).unwrap_or_default(),
            mapper_path: mapper_name.as_ref().map(|m| format!("/dev/mapper/{m}")),
            mapper_name,
            name,
        });
    }
    let mut notes = Vec::new();
    if unused_virtual > 0 {
        notes.push(format!(
            "{unused_virtual} unused virtual block device(s) skipped"
        ));
    }
    let physical = devices.iter().filter(|d| !d.is_virtual()).count();
    if physical == 0 {
        notes.push("No physical storage device was found.".into());
    }
    let outcome = if physical == 0 {
        ModuleOutcome::Partial
    } else {
        ModuleOutcome::Completed
    };
    let count = devices.len();
    Ok(ModuleRun::new(devices, outcome, count, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interfaces_from_device_paths() {
        let cases = [
            (
                "devices/pci0000:00/0000:00:02.1/0000:01:00.0/nvme/nvme0/nvme0n1",
                "nvme0n1",
                StorageInterface::Nvme,
            ),
            (
                "devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sda",
                "sda",
                StorageInterface::Sata,
            ),
            (
                "devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1:1.0/host2/target2:0:0/2:0:0:0/block/sdb",
                "sdb",
                StorageInterface::Usb,
            ),
            (
                "devices/virtual/block/loop1",
                "loop1",
                StorageInterface::Virtual,
            ),
            (
                "devices/platform/mmc_host/mmc0/mmc0:0001/block/mmcblk0",
                "mmcblk0",
                StorageInterface::Mmc,
            ),
            (
                "devices/pci0000:00/0000:00:04.0/virtio1/block/vda",
                "vda",
                StorageInterface::Virtio,
            ),
        ];
        for (path, name, expected) in cases {
            assert_eq!(interface_from_path(path, name), expected, "{path}");
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(
            kind_for(StorageInterface::Nvme, "nvme0n1", Some(false), Some(false)),
            StorageKind::Ssd
        );
        assert_eq!(
            kind_for(StorageInterface::Sata, "sda", Some(true), Some(false)),
            StorageKind::Hdd
        );
        assert_eq!(
            kind_for(StorageInterface::Usb, "sdb", Some(true), Some(true)),
            StorageKind::Flash
        );
        assert_eq!(
            kind_for(StorageInterface::Sata, "sr0", Some(true), Some(true)),
            StorageKind::Optical
        );
        assert_eq!(
            kind_for(StorageInterface::Virtual, "loop0", Some(false), None),
            StorageKind::Virtual
        );
        assert_eq!(
            kind_for(StorageInterface::Scsi, "sdc", None, None),
            StorageKind::Unknown
        );
    }
}
