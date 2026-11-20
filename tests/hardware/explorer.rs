//! Hardware explorer: the component index built from discovered inventories.
//!
//! Fixture results are synthetic. `real_machine_*` tests read this
//! computer (read-only) and check relationships, not fixed values.

use std::collections::{BTreeMap, BTreeSet};

use sentinel_hardware::SysRoot;
use sentinel_inventory::{
    Availability, CacheKind, ComponentCategory, ComponentIndex, ComponentKind, Confidence,
    ParentBasis, Relation, SystemInventory, TelemetryRef,
};
use sentinel_tests::hwfixture::{self, HwFixture};

use super::{scan, scan_fixture, v};

fn index(inv: &SystemInventory) -> ComponentIndex {
    let idx = ComponentIndex::build(inv);
    check_structure(&idx);
    idx
}

/// Invariants every index must satisfy, whatever the hardware.
fn check_structure(idx: &ComponentIndex) {
    let ids: BTreeSet<&str> = idx.components.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), idx.components.len(), "identifiers are unique");
    let mut seen = BTreeSet::new();
    for c in &idx.components {
        if let Some(parent) = &c.parent {
            assert!(
                seen.contains(parent.as_str()),
                "{} listed before its parent",
                c.id
            );
            let p = idx.get(parent).unwrap();
            assert!(
                p.children.contains(&c.id),
                "{parent} does not list {}",
                c.id
            );
            assert_eq!(p.category, c.category, "{} crosses categories", c.id);
            assert_ne!(c.parent_basis, ParentBasis::Root);
            assert_ne!(c.parent_basis, ParentBasis::ParentNotFound);
        } else {
            assert!(
                matches!(
                    c.parent_basis,
                    ParentBasis::Root | ParentBasis::ParentNotFound
                ),
                "{} has no parent but basis {:?}",
                c.id,
                c.parent_basis
            );
        }
        for child in &c.children {
            assert_eq!(
                idx.get(child).unwrap().parent.as_deref(),
                Some(c.id.as_str())
            );
        }
        for link in &c.links {
            assert!(
                ids.contains(link.id.as_str()),
                "{} links to missing {}",
                c.id,
                link.id
            );
        }
        assert!(!c.name.trim().is_empty(), "{} has no name", c.id);
        seen.insert(c.id.as_str());
    }
}

fn kinds(idx: &ComponentIndex, kind: ComponentKind) -> Vec<&sentinel_inventory::Component> {
    idx.components.iter().filter(|c| c.kind == kind).collect()
}

fn linked(c: &sentinel_inventory::Component, relation: Relation) -> Vec<&str> {
    c.links
        .iter()
        .filter(|l| l.relation == relation)
        .map(|l| l.id.as_str())
        .collect()
}

#[test]
fn cpu_package_cores_threads_and_caches() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    let idx = index(&inv);
    let pkg = idx.get("cpu:0").unwrap();
    assert_eq!(pkg.kind, ComponentKind::CpuPackage);
    assert_eq!(pkg.name, "Example AMD 8-Core Processor");
    assert_eq!(pkg.telemetry, [TelemetryRef::CpuTotal]);

    let cores = kinds(&idx, ComponentKind::CpuCore);
    assert_eq!(cores.len(), 8);
    for (i, core) in cores.iter().enumerate() {
        assert_eq!(core.id, format!("cpu:0/core:{i}"));
        assert_eq!(core.parent.as_deref(), Some("cpu:0"));
        assert_eq!(core.parent_basis, ParentBasis::KernelTopology);
        let threads = [2 * i as u32, 2 * i as u32 + 1];
        assert_eq!(core.children, threads.map(|t| format!("cpu:0/cpu:{t}")));
        assert_eq!(
            core.telemetry,
            threads.map(|cpu| TelemetryRef::LogicalCpu { cpu })
        );
        // Own L1d, L1i and L2, plus the shared L3.
        assert_eq!(linked(core, Relation::Cache).len(), 4, "{}", core.id);
    }
    for t in kinds(&idx, ComponentKind::LogicalCpu) {
        let cpu: u32 = t.key.parse().unwrap();
        assert_eq!(t.telemetry, [TelemetryRef::LogicalCpu { cpu }]);
    }

    // One instance per core for L1/L2, one L3 shared by all eight cores.
    let caches = kinds(&idx, ComponentKind::Cache);
    let count = |prefix: &str| caches.iter().filter(|c| c.id.starts_with(prefix)).count();
    assert_eq!(count("cpu:0/cache:l1-data-"), 8);
    assert_eq!(count("cpu:0/cache:l1-instruction-"), 8);
    assert_eq!(count("cpu:0/cache:l2-unified-"), 8);
    let l3 = idx.get("cpu:0/cache:l3-unified-0").unwrap();
    assert_eq!(l3.name, "L3 cache");
    // The fixture lists CPUs 0-999 as sharing L3; only existing ones count.
    assert_eq!(linked(l3, Relation::SharedBy).len(), 8);
    let l2 = idx.get("cpu:0/cache:l2-unified-3").unwrap();
    assert_eq!(linked(l2, Relation::SharedBy), ["cpu:0/core:3"]);

    // The inventory keeps every instance's sharing set, in CPU order.
    let l2_info = inv.processors[0]
        .caches
        .iter()
        .find(|c| c.level == 2 && c.kind == CacheKind::Unified)
        .unwrap();
    assert_eq!(l2_info.instance_cpus.len(), 8);
    assert_eq!(l2_info.instance_cpus[3], [6, 7]);
    assert_eq!(l2_info.shared_by, l2_info.instance_cpus[0]);
}

#[test]
fn hybrid_cores_keep_their_types_and_thread_counts() {
    let idx = index(&scan_fixture(&hwfixture::intel_hybrid_laptop()));
    let cores = kinds(&idx, ComponentKind::CpuCore);
    let threads: Vec<usize> = cores.iter().map(|c| c.children.len()).collect();
    assert_eq!(threads, [2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1]);
    assert!(cores[0].name.ends_with("(performance)"));
    assert!(cores[11].name.ends_with("(efficiency)"));
}

#[test]
fn storage_hierarchy_follows_the_kernel_not_a_template() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    let idx = index(&inv);

    let nvme = idx.get("storage:nvme0n1").unwrap();
    assert_eq!(nvme.kind, ComponentKind::Disk);
    assert_eq!(nvme.name, "Example NVMe 512GB");
    assert_eq!(nvme.children, ["storage:nvme0n1p1", "storage:nvme0n1p2"]);
    assert_eq!(linked(nvme, Relation::Controller), ["pci:0000:01:00.0"]);
    assert!(nvme.telemetry.contains(&TelemetryRef::Disk {
        name: "nvme0n1".into()
    }));

    // Partition 2 carries an encrypted volume, which holds the root fs.
    let p2 = idx.get("storage:nvme0n1p2").unwrap();
    assert_eq!(p2.children, ["storage:dm-0"]);
    assert_eq!(linked(p2, Relation::UsedBy), ["storage:dm-0"]);
    let dm = idx.get("storage:dm-0").unwrap();
    assert_eq!(dm.kind, ComponentKind::VirtualBlock);
    assert_eq!(dm.name, "dm-0 (cryptroot)");
    assert_eq!(dm.parent_basis, ParentBasis::KernelTopology);
    assert_eq!(linked(dm, Relation::BuiltOn), ["storage:nvme0n1p2"]);
    assert!(dm.telemetry.contains(&TelemetryRef::Filesystem {
        device: "/dev/mapper/cryptroot".into()
    }));
    assert!(
        !dm.telemetry
            .iter()
            .any(|t| matches!(t, TelemetryRef::Disk { .. })),
        "virtual devices have no disk I/O of their own"
    );

    // Mounts are what the kernel reported, attached to the right device.
    let dev = |name: &str| inv.storage.iter().find(|d| d.name == name).unwrap();
    let root_mounts = v(&dev("dm-0").usage.mounts);
    assert_eq!(root_mounts.len(), 1);
    assert_eq!(
        (
            root_mounts[0].mount_point.as_str(),
            root_mounts[0].fs_type.as_str()
        ),
        ("/", "ext4")
    );
    let nvme_dev = dev("nvme0n1");
    let efi = v(&nvme_dev.partitions[0].usage.mounts);
    assert_eq!(efi[0].mount_point, "/boot/efi");
    assert!(efi[0].read_only);
    // Not mounted: detected as "no mounts", never given a guessed filesystem.
    assert!(v(&nvme_dev.partitions[1].usage.mounts).is_empty());
    assert_eq!(nvme_dev.partitions[1].usage.holders, ["dm-0"]);
    assert!(v(&nvme_dev.usage.mounts).is_empty());
    assert!(
        !v(&nvme_dev.partitions[1].usage.swap),
        "a swap file is not a swap partition"
    );
    let stick = &dev("sda").partitions[0];
    assert_eq!(
        v(&stick.usage.mounts)[0].mount_point,
        "/media/fixture/EXAMPLE STICK"
    );
    assert_eq!(
        nvme_dev.partitions[0].usage.mounts.confidence(),
        Confidence::KernelReported
    );
}

#[test]
fn an_unreadable_mount_table_is_unavailable_not_empty() {
    let f = hwfixture::amd_laptop();
    std::fs::remove_file(f.root().join("proc/mounts")).unwrap();
    std::fs::remove_file(f.root().join("proc/swaps")).unwrap();
    let inv = scan_fixture(&f);
    let p = &inv
        .storage
        .iter()
        .find(|d| d.name == "nvme0n1")
        .unwrap()
        .partitions[0];
    assert_eq!(p.usage.mounts.availability(), Availability::Unavailable);
    assert_eq!(p.usage.swap.availability(), Availability::Unavailable);
}

#[test]
fn usb_tree_uses_reported_parents_and_flags_orphans() {
    let mut inv = scan_fixture(&hwfixture::amd_laptop());
    let idx = index(&inv);
    let root = idx.get("usb:usb1").unwrap();
    assert_eq!(root.kind, ComponentKind::UsbRootHub);
    assert_eq!(root.parent_basis, ParentBasis::Root);
    assert_eq!(root.children, ["usb:1-1"]);
    assert_eq!(linked(root, Relation::Controller), ["pci:0000:03:00.3"]);
    let stick = idx.get("usb:1-1").unwrap();
    assert_eq!(
        (stick.kind, stick.parent_basis),
        (ComponentKind::UsbDevice, ParentBasis::BusTopology)
    );
    assert_eq!(stick.name, "Example Flash");

    // A device whose parent hub was not discovered stays at the top level,
    // marked, instead of being attached to some other hub.
    let stick = inv
        .usb_devices
        .iter_mut()
        .find(|d| d.sysfs_name == "1-1")
        .unwrap();
    stick.parent = Some("9-9".into());
    let idx = index(&inv);
    let stick = idx.get("usb:1-1").unwrap();
    assert_eq!(
        (stick.parent.as_deref(), stick.parent_basis),
        (None, ParentBasis::ParentNotFound)
    );
    assert!(idx.get("usb:usb1").unwrap().children.is_empty());
}

#[test]
fn pci_tree_follows_bridges_and_links_functions() {
    // Without the bridge in the inventory the relationship is unknown.
    let idx = index(&scan_fixture(&hwfixture::amd_laptop()));
    let gpu_fn = idx.get("pci:0000:03:00.0").unwrap();
    assert_eq!(gpu_fn.parent_basis, ParentBasis::ParentNotFound);

    let f = hwfixture::amd_laptop();
    f.pci(
        &[],
        "0000:00:08.1",
        0x060400,
        0x1022,
        0x1484,
        (0x1022, 0x1484),
        Some("pcieport"),
    );
    let idx = index(&scan_fixture(&f));
    let bridge = idx.get("pci:0000:00:08.1").unwrap();
    assert_eq!(bridge.kind, ComponentKind::PciBridge);
    for child in ["pci:0000:03:00.0", "pci:0000:03:00.3", "pci:0000:03:00.5"] {
        assert!(bridge.children.iter().any(|c| c == child), "{child}");
        assert_eq!(
            idx.get(child).unwrap().parent_basis,
            ParentBasis::BusTopology
        );
    }
    let gpu = idx.get("gpu:0000:03:00.0").unwrap();
    assert_eq!(gpu.category, ComponentCategory::Graphics);
    assert_eq!(linked(gpu, Relation::PciFunction), ["pci:0000:03:00.0"]);
    assert_eq!(
        gpu.telemetry,
        [TelemetryRef::Gpu {
            pci_address: "0000:03:00.0".into()
        }]
    );
    let wlan = idx.get("net:wlan0").unwrap();
    assert_eq!(linked(wlan, Relation::PciFunction), ["pci:0000:02:00.0"]);
    assert!(linked(idx.get("net:lo").unwrap(), Relation::PciFunction).is_empty());
}

#[test]
fn memory_devices_come_only_from_firmware_records() {
    // Firmware records unreadable: no memory devices, never a guessed layout.
    let idx = index(&scan_fixture(&hwfixture::amd_laptop()));
    assert!(kinds(&idx, ComponentKind::MemoryDevice).is_empty());
    assert_eq!(idx.get("memory").unwrap().telemetry, [TelemetryRef::Memory]);

    // Firmware lists the memory devices: each shown as the firmware names it.
    let inv = scan_fixture(&hwfixture::intel_hybrid_laptop());
    let idx = index(&inv);
    let devices = kinds(&idx, ComponentKind::MemoryDevice);
    let modules = v(&inv.memory.as_ref().unwrap().modules);
    assert_eq!(devices.len(), modules.len());
    assert!(
        devices
            .iter()
            .all(|d| d.parent_basis == ParentBasis::Firmware)
    );
    for (d, m) in devices.iter().zip(&modules) {
        assert_eq!(Some(d.name.as_str()), m.locator.as_deref());
    }
}

#[test]
fn batteries_power_supplies_and_absent_hardware() {
    let idx = index(&scan_fixture(&hwfixture::amd_laptop()));
    let bat = idx.get("battery:BAT0").unwrap();
    assert_eq!(bat.name, "BAT0 (EXB-BATT)");
    assert_eq!(
        bat.telemetry,
        [TelemetryRef::Battery {
            name: "BAT0".into()
        }]
    );
    assert!(idx.get("power:AC").is_some());

    let desktop = index(&scan_fixture(&hwfixture::desktop_missing_dmi()));
    assert!(kinds(&desktop, ComponentKind::Battery).is_empty());

    // Nothing discovered: an empty index, not an error and not placeholders.
    let empty = index(&scan_fixture(&HwFixture::new()));
    assert!(empty.components.iter().all(|c| !matches!(
        c.kind,
        ComponentKind::CpuCore | ComponentKind::Gpu | ComponentKind::Disk | ComponentKind::Battery
    )));
}

#[test]
fn identifiers_are_stable_across_rescans() {
    let f = hwfixture::amd_laptop();
    let a = index(&scan_fixture(&f));
    let b = index(&scan(SysRoot::at(f.root())));
    let ids = |i: &ComponentIndex| {
        i.components
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&a), ids(&b));
    assert_eq!(a.components, b.components);
}

#[test]
fn every_intel_and_arm_index_is_well_formed() {
    for f in [
        hwfixture::intel_hybrid_laptop(),
        hwfixture::desktop_missing_dmi(),
        hwfixture::arm_board(),
    ] {
        index(&scan_fixture(&f));
    }
}

/// The explorer fixtures the frontend tests load are produced from fixture
/// machines by this crate, so Rust models and TypeScript stay in step.
/// Regenerate with `SENTINEL_UPDATE_FIXTURES=1 cargo test`.
#[test]
fn the_frontend_explorer_fixtures_are_current() {
    for (name, fixture) in [
        ("amd-laptop", hwfixture::amd_laptop()),
        ("intel-hybrid", hwfixture::intel_hybrid_laptop()),
    ] {
        let mut inv = scan_fixture(&fixture);
        // Wall-clock values of the scan itself vary from run to run.
        inv.duration_ms = 0;
        for report in &mut inv.discovery_status {
            report.duration_ms = 0;
            report.finished_at = inv.discovered_at;
        }
        inv.root_override = Some(format!("fixture:{name}"));
        let components = ComponentIndex::build(&inv);
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "inventory": inv,
            "components": components,
        }))
        .unwrap()
            + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../apps/desktop/src/test-fixtures/explorer-{name}.json"
        ));
        if std::env::var_os("SENTINEL_UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &json).unwrap();
        }
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            current == json,
            "{} is out of date; regenerate with SENTINEL_UPDATE_FIXTURES=1 cargo test -p sentinel-tests --test hardware",
            path.display()
        );
    }
}

// ------------------------------------------------------------ real machine

#[test]
fn real_machine_index_is_well_formed_and_matches_live_telemetry() {
    use sentinel_telemetry::Engine;
    use sentinel_telemetry::collectors::SampleTime;
    use sentinel_telemetry::model::CollectorKind;

    let inv = scan(SysRoot::host());
    let idx = index(&inv);

    let mut engine = Engine::for_host();
    for (n, mono) in [(0u64, 1.0), (1, 2.0)] {
        for kind in [
            CollectorKind::Cpu,
            CollectorKind::Memory,
            CollectorKind::Gpu,
            CollectorKind::Storage,
            CollectorKind::Network,
            CollectorKind::Battery,
        ] {
            engine.run(
                kind,
                SampleTime {
                    wall_ms: 1_800_000_000_000 + n * 1000,
                    mono_s: mono,
                    interval_ms: 1000,
                },
            );
        }
        if n == 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    let snap = engine.snapshot();

    // Every logical CPU the explorer shows has exactly one live sample, and
    // every live sample belongs to a component.
    let cpu_refs: BTreeSet<u32> = idx
        .components
        .iter()
        .filter(|c| c.kind == ComponentKind::LogicalCpu)
        .flat_map(|c| c.telemetry.iter())
        .filter_map(|t| match t {
            TelemetryRef::LogicalCpu { cpu } => Some(*cpu),
            _ => None,
        })
        .collect();
    let live: BTreeSet<u32> = snap
        .cpu
        .as_ref()
        .unwrap()
        .cores
        .iter()
        .map(|c| c.cpu)
        .collect();
    assert_eq!(cpu_refs, live, "logical CPUs in the explorer vs telemetry");

    let refs: Vec<&TelemetryRef> = idx
        .components
        .iter()
        .flat_map(|c| c.telemetry.iter())
        .collect();
    let disks: BTreeSet<&str> = refs
        .iter()
        .filter_map(|t| match t {
            TelemetryRef::Disk { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    if let Some(storage) = &snap.storage {
        for d in &storage.devices {
            assert!(
                disks.contains(d.name.as_str()),
                "disk {} has no component",
                d.name
            );
        }
        let fs: BTreeSet<&str> = refs
            .iter()
            .filter_map(|t| match t {
                TelemetryRef::Filesystem { device } => Some(device.as_str()),
                _ => None,
            })
            .collect();
        for f in &storage.filesystems {
            if f.device.starts_with("/dev/loop") {
                continue; // snap images and other loop mounts
            }
            assert!(
                fs.contains(f.device.as_str()),
                "filesystem on {} has no component",
                f.device
            );
        }
    }
    if let Some(gpu) = &snap.gpu {
        for g in &gpu.gpus {
            assert!(
                idx.get(&format!("gpu:{}", g.pci_address)).is_some(),
                "GPU {}",
                g.pci_address
            );
        }
    }
    if let Some(net) = &snap.network {
        for i in &net.interfaces {
            assert!(
                idx.get(&format!("net:{}", i.name)).is_some(),
                "interface {}",
                i.name
            );
        }
    }
    if let Some(bat) = &snap.battery {
        for b in &bat.batteries {
            assert!(
                idx.get(&format!("battery:{}", b.name)).is_some(),
                "battery {}",
                b.name
            );
        }
    }

    // Mounts discovered for the root filesystem agree with telemetry.
    let root_device = snap
        .storage
        .as_ref()
        .and_then(|s| s.filesystems.iter().find(|f| f.mount_point == "/"))
        .map(|f| f.device.clone());
    if let Some(device) = root_device {
        let mounted: BTreeMap<String, Vec<String>> = inv
            .storage
            .iter()
            .flat_map(|d| {
                let own = (
                    format!("/dev/{}", d.name),
                    d.usage.mounts.value().cloned().unwrap_or_default(),
                );
                std::iter::once(own).chain(d.partitions.iter().map(|p| {
                    (
                        format!("/dev/{}", p.name),
                        p.usage.mounts.value().cloned().unwrap_or_default(),
                    )
                }))
            })
            .map(|(dev, m)| (dev, m.into_iter().map(|m| m.mount_point).collect()))
            .collect();
        let with_root: Vec<&String> = mounted
            .iter()
            .filter(|(_, m)| m.iter().any(|p| p == "/"))
            .map(|(d, _)| d)
            .collect();
        assert!(
            with_root.iter().any(|d| **d == device) || device.starts_with("/dev/mapper/"),
            "root on {device}, discovery says {with_root:?}"
        );
    }
    eprintln!(
        "real-machine index: {} components, {} CPU refs, {} disks",
        idx.components.len(),
        cpu_refs.len(),
        disks.len()
    );
}

/// Writes the AMD laptop fixture tree to `$SENTINEL_FIXTURE_DUMP` for the
/// desktop end-to-end scripts (`tests/desktop-e2e/docs_scenarios.py`).
#[test]
#[ignore = "only for the desktop end-to-end scripts"]
fn dump_amd_laptop_fixture() {
    let Some(dest) = std::env::var_os("SENTINEL_FIXTURE_DUMP") else {
        return;
    };
    let f = hwfixture::amd_laptop();
    let status = std::process::Command::new("cp")
        .arg("-a")
        .arg(f.root())
        .arg(&dest)
        .status()
        .unwrap();
    assert!(status.success());
}
