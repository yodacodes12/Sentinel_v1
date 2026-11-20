//! Discovery tests: every module against synthetic fixture trees, the full
//! pipeline, failure isolation, cancellation, and the real machine.
//!
//! Fixture results are synthetic. The `real_machine_*` tests read this
//! computer's actual `/sys` and `/proc` (read-only) and check the inventory
//! against independent readings rather than against fixed values.

use std::collections::BTreeSet;

use sentinel_hardware::{CancelToken, DiscoveryContext, Stopped, SysRoot, discover};
use sentinel_inventory::{
    Availability, BootMode, ChassisType, Confidence, CoreKind, GpuKind, GpuMemoryKind,
    IdentificationLevel, ModuleId, ModuleOutcome, NetworkKind, StorageInterface, StorageKind,
    SystemInventory,
};
use sentinel_tests::hwfixture::{self, HwFixture, OS_ARCH};

mod explorer;

fn scan(root: SysRoot) -> SystemInventory {
    let ctx = DiscoveryContext::new(root, 1_800_000_000, CancelToken::new());
    let outcome = discover(&ctx, &mut |_| {});
    assert_eq!(outcome.stopped, None);
    outcome.inventory
}

fn scan_fixture(fixture: &HwFixture) -> SystemInventory {
    scan(SysRoot::at(fixture.root()))
}

fn outcome(inv: &SystemInventory, module: ModuleId) -> ModuleOutcome {
    inv.discovery_status
        .iter()
        .find(|r| r.module == module)
        .unwrap()
        .outcome
}

fn v<T: Clone>(field: &sentinel_inventory::Field<T>) -> T {
    field.value().cloned().unwrap_or_else(|| {
        panic!(
            "field not detected: {:?} {:?}",
            field.availability(),
            field.notes()
        )
    })
}

// ------------------------------------------------------------ AMD laptop

#[test]
fn amd_laptop_identity_firmware_and_board() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    assert!(
        inv.root_override.is_some(),
        "fixture inventories are labelled"
    );
    let s = inv.system.as_ref().unwrap();
    assert_eq!(v(&s.manufacturer), "Example Corp");
    assert_eq!(v(&s.product_name), "Example Book 14 AMD");
    assert_eq!(s.manufacturer.confidence(), Confidence::FirmwareReported);
    assert_eq!(
        s.manufacturer.source(),
        Some("/sys/class/dmi/id/sys_vendor")
    );
    assert_eq!(v(&s.chassis), ChassisType::Laptop);
    assert_eq!(v(&s.architecture), "x86_64");
    assert_eq!(v(&s.os.id), "ubuntu");
    assert_eq!(s.os.id_like, ["debian"]);
    let fw = inv.firmware.as_ref().unwrap();
    assert_eq!(v(&fw.bios_version), "1.2.3");
    assert_eq!(v(&fw.boot_mode), BootMode::Uefi);
    let b = inv.motherboard.as_ref().unwrap();
    assert_eq!(v(&b.board_name), "EXB14-A");
    assert_eq!(b.identification.level, IdentificationLevel::BoardIdentified);
    assert!(!b.identification.externally_verified);
    for module in [
        ModuleId::SystemIdentity,
        ModuleId::Firmware,
        ModuleId::Motherboard,
    ] {
        assert_eq!(
            outcome(&inv, module),
            ModuleOutcome::Completed,
            "{module:?}"
        );
    }
}

#[test]
fn amd_laptop_eight_core_topology() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    assert_eq!(inv.processors.len(), 1);
    let cpu = &inv.processors[0];
    assert_eq!(v(&cpu.vendor), "AMD");
    assert_eq!(v(&cpu.physical_cores), 8);
    assert_eq!(v(&cpu.logical_processors), 16);
    assert_eq!(cpu.cores.len(), 8);
    for (i, core) in cpu.cores.iter().enumerate() {
        assert_eq!(core.index as usize, i);
        assert_eq!(core.logical_cpus, [2 * i as u32, 2 * i as u32 + 1]);
        assert_eq!(core.kind, CoreKind::Uniform);
    }
    assert_eq!(v(&cpu.microarchitecture), "Zen 2");
    assert_eq!(cpu.microarchitecture.confidence(), Confidence::Inferred);
    assert!(!v(&cpu.hybrid));
    assert_eq!(v(&cpu.max_frequency_khz), 4_800_000);
    let l1d = cpu
        .caches
        .iter()
        .find(|c| c.level == 1 && c.kind == sentinel_inventory::CacheKind::Data)
        .unwrap();
    assert_eq!((l1d.size_bytes, l1d.instances), (Some(48 * 1024), 8));
    let l3 = cpu.caches.iter().find(|c| c.level == 3).unwrap();
    assert_eq!(l3.instances, 1);
}

#[test]
fn amd_laptop_memory_without_firmware_records() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    let m = inv.memory.as_ref().unwrap();
    assert_eq!(v(&m.usable_bytes), 15_728_640 * 1024);
    assert_eq!(v(&m.kernel_physical_bytes), 0x800_0000 * 128);
    assert_eq!(v(&m.swap_total_bytes), 4_194_304 * 1024);
    // No SMBIOS records in this fixture: installed RAM is not invented.
    assert_eq!(m.installed_bytes.value(), None);
    assert_ne!(m.installed_bytes.availability(), Availability::Detected);
    assert_eq!(m.modules.value(), None);
    assert_eq!(v(&m.snapshot.used_bytes), (15_728_640 - 9_000_000) * 1024);
    assert_eq!(m.snapshot.used_bytes.confidence(), Confidence::Derived);
    assert_eq!(outcome(&inv, ModuleId::Memory), ModuleOutcome::Partial);
}

#[test]
fn amd_laptop_integrated_gpu_with_reserved_memory() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    assert_eq!(inv.graphics.len(), 1);
    let g = &inv.graphics[0];
    assert_eq!(g.pci_address, "0000:03:00.0");
    assert_eq!((g.vendor_id, g.device_id), (0x1002, 0x1638));
    assert_eq!(v(&g.kind), GpuKind::Integrated);
    assert_eq!(g.kind.confidence(), Confidence::Inferred);
    assert_eq!(v(&g.model_name), "Example Integrated GPU");
    assert_eq!(g.model_name.confidence(), Confidence::Database);
    assert_eq!(v(&g.subsystem_name), "Example Laptop Subsystem");
    assert_eq!(v(&g.driver), "amdgpu");
    assert_eq!(
        v(&g.memory).kind,
        GpuMemoryKind::ReservedSystemMemory,
        "shared memory is never called VRAM"
    );
    assert!(v(&g.boot_display));
    assert_eq!(g.drm_nodes, ["card0", "renderD128"]);
}

#[test]
fn amd_laptop_storage_partitions_and_usb_storage() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    let nvme = inv.storage.iter().find(|d| d.name == "nvme0n1").unwrap();
    assert_eq!(nvme.kind, StorageKind::Ssd);
    assert_eq!(v(&nvme.interface), StorageInterface::Nvme);
    assert_eq!(v(&nvme.capacity_bytes), 1_000_215_216 * 512);
    assert_eq!(v(&nvme.model), "Example NVMe 512GB");
    assert_eq!(v(&nvme.firmware_revision), "EXFW01");
    assert_eq!(nvme.controller_pci_address.as_deref(), Some("0000:01:00.0"));
    let parts: Vec<&str> = nvme.partitions.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        parts,
        ["nvme0n1p1", "nvme0n1p2"],
        "partitions are children, not devices"
    );
    assert!(!inv.storage.iter().any(|d| d.name.starts_with("nvme0n1p")));

    let usb = inv.storage.iter().find(|d| d.name == "sda").unwrap();
    assert_eq!(v(&usb.interface), StorageInterface::Usb);
    assert_eq!(usb.kind, StorageKind::Flash);
    assert!(v(&usb.removable));

    // An unused loop device is skipped; a used one is listed as virtual.
    assert!(!inv.storage.iter().any(|d| d.name == "loop0"));
    assert_eq!(
        inv.storage.iter().find(|d| d.name == "loop1").unwrap().kind,
        StorageKind::Virtual
    );
}

#[test]
fn amd_laptop_battery_network_usb_pci_sensors() {
    let inv = scan_fixture(&hwfixture::amd_laptop());
    let bat = &inv.batteries[0];
    assert_eq!(bat.name, "BAT0");
    assert_eq!(v(&bat.energy_full_uwh), 50_000_000);
    assert_eq!(v(&bat.health_percent), 89.3);
    assert_eq!(
        bat.charge_full_uah.value(),
        None,
        "energy-reporting battery has no charge values"
    );
    assert_eq!(v(&bat.cycle_count), 120);
    let ac = inv.power_supplies.iter().find(|p| p.name == "AC").unwrap();
    assert!(!v(&ac.online));

    let wlan = inv.network.iter().find(|n| n.name == "wlan0").unwrap();
    assert_eq!(wlan.kind, NetworkKind::Wireless);
    assert_eq!(v(&wlan.driver), "iwlwifi");
    assert_eq!(v(&wlan.device_bus), "PCI 0000:02:00.0");
    assert_eq!(
        inv.network.iter().find(|n| n.name == "lo").unwrap().kind,
        NetworkKind::Loopback
    );
    assert_eq!(
        inv.network
            .iter()
            .find(|n| n.name == "docker0")
            .unwrap()
            .kind,
        NetworkKind::Bridge
    );

    let root = inv
        .usb_devices
        .iter()
        .find(|d| d.sysfs_name == "usb1")
        .unwrap();
    assert!(root.is_root_hub);
    assert_eq!(root.controller_pci_address.as_deref(), Some("0000:03:00.3"));
    assert_eq!(root.ports.len(), 2);
    assert_eq!(root.ports[0].device.as_deref(), Some("1-1"));
    assert_eq!(
        root.ports[1].device, None,
        "an empty Linux port is not a device"
    );
    let stick = inv
        .usb_devices
        .iter()
        .find(|d| d.sysfs_name == "1-1")
        .unwrap();
    assert_eq!(stick.parent.as_deref(), Some("usb1"));
    assert_eq!(stick.port_number, Some(1));
    assert_eq!(stick.interface_classes, ["Mass storage"]);
    assert_eq!(v(&stick.product_name), "Example Flash Drive");

    let gpu_pci = inv
        .pci_devices
        .iter()
        .find(|d| d.address == "0000:03:00.0")
        .unwrap();
    assert_eq!(gpu_pci.parent.as_deref(), Some("0000:00:08.1"));
    assert_eq!(gpu_pci.role, sentinel_inventory::PciRole::Graphics);

    let temps: Vec<&str> = inv.sensors.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(temps, ["hwmon0/temp1", "hwmon1/temp1", "hwmon1/temp2"]);
    assert_eq!(v(&inv.sensors[0].label), "Tctl");
    assert_eq!(v(&inv.sensors[0].device), "PCI 0000:00:18.3");
    assert_eq!(inv.sensors[2].label.value(), None);
    assert_eq!(inv.thermal_zones.len(), 1);
}

// ---------------------------------------------------------- Intel hybrid

#[test]
fn intel_hybrid_topology_and_core_kinds() {
    let inv = scan_fixture(&hwfixture::intel_hybrid_laptop());
    let cpu = &inv.processors[0];
    assert_eq!(v(&cpu.physical_cores), 12);
    assert_eq!(v(&cpu.logical_processors), 16);
    assert!(v(&cpu.hybrid));
    let p: Vec<_> = cpu
        .cores
        .iter()
        .filter(|c| c.kind == CoreKind::Performance)
        .collect();
    let e: Vec<_> = cpu
        .cores
        .iter()
        .filter(|c| c.kind == CoreKind::Efficiency)
        .collect();
    assert_eq!((p.len(), e.len()), (4, 8));
    assert!(p.iter().all(|c| c.logical_cpus.len() == 2));
    assert!(e.iter().all(|c| c.logical_cpus.len() == 1));
    assert_eq!(e[0].max_frequency_khz, Some(3_400_000));
    assert_eq!(v(&cpu.vendor), "Intel");
    assert_eq!(v(&cpu.microarchitecture), "Alder Lake");
}

#[test]
fn intel_hybrid_graphics_memory_modules_batteries_and_distro() {
    let inv = scan_fixture(&hwfixture::intel_hybrid_laptop());
    let kinds: Vec<GpuKind> = inv.graphics.iter().map(|g| v(&g.kind)).collect();
    assert_eq!(
        kinds,
        [GpuKind::Integrated, GpuKind::Dedicated],
        "hybrid graphics"
    );
    let report = inv
        .discovery_status
        .iter()
        .find(|r| r.module == ModuleId::Gpu)
        .unwrap();
    assert!(report.notes.iter().any(|n| n.contains("Hybrid graphics")));

    let m = inv.memory.as_ref().unwrap();
    assert_eq!(v(&m.installed_bytes), 32 * 1024 * 1024 * 1024);
    assert_eq!(m.installed_bytes.confidence(), Confidence::FirmwareReported);
    assert_eq!(v(&m.memory_type), "DDR5");
    assert_eq!(v(&m.speed_mts), 4800);
    assert_eq!((v(&m.slots_total), v(&m.slots_populated)), (2, 2));
    assert_eq!(v(&m.modules).len(), 2);
    assert_eq!(outcome(&inv, ModuleId::Memory), ModuleOutcome::Completed);

    assert_eq!(inv.batteries.len(), 2, "multiple batteries");
    let bat = &inv.batteries[0];
    assert_eq!(v(&bat.charge_full_uah), 4_000_000);
    assert_eq!(
        bat.energy_full_uwh.value(),
        None,
        "charge-reporting battery has no energy values"
    );
    assert_eq!(v(&bat.health_percent), 95.2);
    assert_eq!(
        v(&bat.status),
        sentinel_inventory::BatteryStatus::NotCharging
    );

    let sata = inv.storage.iter().find(|d| d.name == "sda").unwrap();
    assert_eq!(v(&sata.interface), StorageInterface::Sata);
    assert_eq!(sata.kind, StorageKind::Ssd);

    let eth = inv.network.iter().find(|n| n.name == "enp3s0").unwrap();
    assert_eq!(eth.kind, NetworkKind::Ethernet);
    assert_eq!(v(&eth.speed_mbps), 1000);

    // Fedora ships os-release only under /usr/lib.
    let os = &inv.system.as_ref().unwrap().os;
    assert_eq!(v(&os.id), "fedora");
    assert_eq!(os.name.source(), Some("/usr/lib/os-release"));
}

// -------------------------------------------------------------- desktop

#[test]
fn desktop_placeholders_missing_battery_and_restricted_sensor() {
    let inv = scan_fixture(&hwfixture::desktop_missing_dmi());
    let s = inv.system.as_ref().unwrap();
    assert_eq!(
        s.manufacturer.availability(),
        Availability::Invalid,
        "placeholder is not a vendor"
    );
    assert_eq!(s.product_name.availability(), Availability::Invalid);
    assert_eq!(s.product_family.availability(), Availability::Unsupported);
    assert_eq!(v(&s.chassis), ChassisType::Desktop);
    assert_eq!(v(&s.os.id), "arch");
    let b = inv.motherboard.as_ref().unwrap();
    assert_eq!(v(&b.board_name), "EX-B650");
    assert_eq!(b.board_revision.availability(), Availability::Invalid);
    assert_eq!(
        v(&inv.firmware.as_ref().unwrap().boot_mode),
        BootMode::Legacy
    );

    let cpu = &inv.processors[0];
    assert_eq!(
        (v(&cpu.physical_cores), v(&cpu.logical_processors)),
        (16, 32),
        "sixteen-core CPU"
    );
    assert_eq!(v(&cpu.microarchitecture), "Zen 4");

    let m = inv.memory.as_ref().unwrap();
    assert_eq!((v(&m.slots_total), v(&m.slots_populated)), (8, 4));
    assert_eq!(v(&m.installed_bytes), 64 * 1024 * 1024 * 1024);
    assert_eq!(
        m.kernel_physical_bytes.availability(),
        Availability::Unsupported
    );

    let g = &inv.graphics[0];
    assert_eq!(v(&g.kind), GpuKind::Dedicated);
    assert_eq!(v(&g.memory).kind, GpuMemoryKind::DedicatedVram);
    // No PCI ID database installed: IDs only, names unavailable.
    assert_eq!(g.model_name.availability(), Availability::Unavailable);
    assert_eq!(outcome(&inv, ModuleId::Pci), ModuleOutcome::Partial);

    let hdd = inv.storage.iter().find(|d| d.name == "sdb").unwrap();
    assert_eq!(hdd.kind, StorageKind::Hdd);
    assert_eq!(v(&hdd.physical_sector_bytes), 4096);

    assert!(inv.batteries.is_empty(), "missing battery is not an error");
    assert_eq!(outcome(&inv, ModuleId::Battery), ModuleOutcome::Completed);

    let restricted = inv.sensors.iter().find(|s| s.id == "hwmon0/temp1").unwrap();
    if sentinel_core::platform::current_uid() != 0 {
        assert_eq!(
            v(&restricted.access),
            sentinel_inventory::SensorAccess::PermissionDenied
        );
        assert_eq!(outcome(&inv, ModuleId::Sensors), ModuleOutcome::Partial);
    }
    let fan = inv.sensors.iter().find(|s| s.id == "hwmon0/fan1").unwrap();
    assert_eq!(fan.kind, sentinel_inventory::SensorKind::Fan);
    assert_eq!(v(&fan.label), "CPU fan");
}

#[test]
fn arm_board_without_dmi_pci_or_cpufreq() {
    let inv = scan_fixture(&hwfixture::arm_board());
    assert_eq!(
        outcome(&inv, ModuleId::SystemIdentity),
        ModuleOutcome::Partial
    );
    let s = inv.system.as_ref().unwrap();
    assert_eq!(s.manufacturer.availability(), Availability::Unsupported);
    assert_eq!(v(&s.architecture), "aarch64");
    assert_eq!(
        inv.motherboard.as_ref().unwrap().identification.level,
        IdentificationLevel::Unidentified
    );
    let cpu = &inv.processors[0];
    assert_eq!(v(&cpu.physical_cores), 4);
    assert_eq!(v(&cpu.vendor), "ARM");
    assert_eq!(cpu.vendor.confidence(), Confidence::Inferred);
    assert_eq!(
        cpu.max_frequency_khz.availability(),
        Availability::Unsupported
    );
    assert!(inv.graphics.is_empty());
    assert_eq!(outcome(&inv, ModuleId::Pci), ModuleOutcome::Unavailable);
    assert_eq!(outcome(&inv, ModuleId::Gpu), ModuleOutcome::Unavailable);
    let mmc = inv.storage.iter().find(|d| d.name == "mmcblk0").unwrap();
    assert_eq!(v(&mmc.interface), StorageInterface::Mmc);
    assert_eq!(mmc.partitions.len(), 2);
}

#[test]
fn distribution_identification_variants() {
    let f = HwFixture::new();
    f.os_release("etc/os-release", OS_ARCH);
    let inv = scan_fixture(&f);
    let os = &inv.system.as_ref().unwrap().os;
    assert_eq!(v(&os.pretty_name), "Arch Linux");
    assert_eq!(
        os.version_id.availability(),
        Availability::Unavailable,
        "rolling release has no version"
    );
}

// ---------------------------------------------------- pipeline behaviour

#[test]
fn an_empty_root_yields_honest_empty_results_not_errors() {
    let f = HwFixture::new();
    let inv = scan_fixture(&f);
    assert_eq!(inv.discovery_status.len(), 12);
    assert!(inv.processors.is_empty() && inv.graphics.is_empty() && inv.storage.is_empty());
    for report in &inv.discovery_status {
        assert_ne!(report.outcome, ModuleOutcome::Failed, "{:?}", report.module);
    }
    assert_eq!(outcome(&inv, ModuleId::Cpu), ModuleOutcome::Unavailable);
    let json = serde_json::to_value(&inv).unwrap();
    // Unavailable memory is absent, not zero.
    assert!(json["memory"]["usable_bytes"].get("value").is_none());
}

#[test]
fn one_broken_module_does_not_stop_the_others() {
    let f = hwfixture::amd_laptop();
    // Battery directory made unreadable, cpuinfo removed.
    f.chmod("sys/class/power_supply", 0o000);
    std::fs::remove_file(f.root().join("proc/cpuinfo")).unwrap();
    let inv = scan_fixture(&f);
    f.chmod("sys/class/power_supply", 0o755);
    if sentinel_core::platform::current_uid() != 0 {
        assert_eq!(outcome(&inv, ModuleId::Battery), ModuleOutcome::Unavailable);
    }
    // CPU topology still works without /proc/cpuinfo; identity is missing.
    let cpu = &inv.processors[0];
    assert_eq!(v(&cpu.physical_cores), 8);
    assert_eq!(cpu.model_name.availability(), Availability::Unavailable);
    assert_eq!(outcome(&inv, ModuleId::Cpu), ModuleOutcome::Partial);
    assert_eq!(outcome(&inv, ModuleId::Storage), ModuleOutcome::Completed);
    assert_eq!(inv.graphics.len(), 1);
}

#[test]
fn cancellation_marks_remaining_modules_and_reports_stop() {
    let f = hwfixture::amd_laptop();
    let token = CancelToken::new();
    let ctx = DiscoveryContext::new(SysRoot::at(f.root()), 1, token.clone());
    let mut seen = 0;
    let outcome = discover(&ctx, &mut |event| {
        if let sentinel_hardware::Event::ModuleFinished(_) = event {
            seen += 1;
            if seen == 3 {
                token.cancel();
            }
        }
    });
    assert_eq!(outcome.stopped, Some(Stopped::Cancelled));
    let cancelled = outcome
        .inventory
        .discovery_status
        .iter()
        .filter(|r| r.outcome == ModuleOutcome::Cancelled)
        .count();
    assert_eq!(cancelled, 9);
    assert!(outcome.inventory.graphics.is_empty());

    let expired = CancelToken::new().with_deadline(std::time::Instant::now());
    let ctx = DiscoveryContext::new(SysRoot::at(f.root()), 1, expired);
    assert_eq!(discover(&ctx, &mut |_| {}).stopped, Some(Stopped::TimedOut));
}

#[test]
fn serial_numbers_and_uuids_are_never_read() {
    for fixture in [hwfixture::amd_laptop(), hwfixture::desktop_missing_dmi()] {
        let root = SysRoot::at(fixture.root()).recording();
        let ctx = DiscoveryContext::new(root, 1, CancelToken::new());
        let outcome = discover(&ctx, &mut |_| {});
        let json = serde_json::to_string(&outcome.inventory).unwrap();
        for secret in [
            "FIXTURE-SYSTEM-SERIAL",
            "FIXTURE-BOARD-SERIAL",
            "00000000-1111",
            "FIXTURE-DISK-SERIAL",
            "FIXTURE-BATTERY-SERIAL",
            "FIXTURE-USB-SERIAL",
            "FIXTURE-SERIAL-",
        ] {
            assert!(!json.contains(secret), "{secret} leaked into the inventory");
        }
        let accessed = ctx.root.accessed();
        for path in &accessed {
            let name = path.rsplit('/').next().unwrap_or("");
            assert!(
                !sentinel_hardware::source::SENSITIVE_NAMES.contains(&name),
                "opened {path}"
            );
        }
    }
}

#[test]
fn fingerprint_is_stable_and_ignores_serials() {
    let a = scan_fixture(&hwfixture::amd_laptop()).fingerprint.unwrap();
    let b = scan_fixture(&hwfixture::amd_laptop()).fingerprint.unwrap();
    assert_eq!(a, b);
    let other = scan_fixture(&hwfixture::intel_hybrid_laptop())
        .fingerprint
        .unwrap();
    assert_ne!(a.value, other.value);
    assert!(a.inputs.contains(&"pci_device_ids".to_string()));
    assert!(
        !a.inputs
            .iter()
            .any(|i| i.contains("serial") || i.contains("uuid") || i.contains("mac"))
    );
}

#[test]
fn the_inventory_round_trips_through_the_cache_format() {
    let inv = scan_fixture(&hwfixture::intel_hybrid_laptop());
    let text = sentinel_inventory::cache::encode(&inv);
    let version = sentinel_inventory::INVENTORY_SCHEMA_VERSION;
    let back: SystemInventory = sentinel_inventory::cache::decode(version, &text).unwrap();
    assert_eq!(back, inv);
    // A cache written by an older layout is ignored, never reinterpreted.
    assert!(sentinel_inventory::cache::decode::<SystemInventory>(version - 1, &text).is_err());
}

// ---------------------------------------------------------- real machine

/// Locations discovery is allowed to read. Anything else (home directories,
/// `/dev`, arbitrary files) means it strayed.
const ALLOWED_PREFIXES: &[&str] = &[
    "sys/",
    "proc/cpuinfo",
    "proc/meminfo",
    "proc/mounts",
    "proc/swaps",
    "proc/sys/kernel/",
    "etc/os-release",
    "usr/lib/os-release",
    "usr/share/hwdata/",
    "usr/share/misc/",
    "usr/share/pci.ids",
    "usr/share/usb.ids",
];

#[test]
fn real_machine_discovery_reads_only_expected_locations() {
    let root = SysRoot::host().recording();
    let ctx = DiscoveryContext::new(root, 1, CancelToken::new());
    let started = std::time::Instant::now();
    let outcome = discover(&ctx, &mut |_| {});
    let elapsed = started.elapsed();
    assert_eq!(outcome.stopped, None);
    let accessed = ctx.root.accessed();
    for path in &accessed {
        assert!(
            ALLOWED_PREFIXES.iter().any(|p| path.starts_with(p)),
            "discovery accessed {path}"
        );
    }
    // Bounded work: no filesystem-wide walk.
    assert!(accessed.len() < 50_000, "{} paths accessed", accessed.len());
    assert!(elapsed.as_secs() < 20, "discovery took {elapsed:?}");
    let dirs: BTreeSet<&str> = accessed
        .iter()
        .filter_map(|p| p.split('/').next())
        .collect();
    assert!(
        dirs.iter()
            .all(|d| ["sys", "proc", "etc", "usr"].contains(d)),
        "{dirs:?}"
    );
    eprintln!(
        "real-machine scan: {} paths in {:?}",
        accessed.len(),
        elapsed
    );
}

#[test]
fn real_machine_inventory_agrees_with_independent_readings() {
    let inv = scan(SysRoot::host());
    assert!(inv.root_override.is_none());
    for report in &inv.discovery_status {
        assert_ne!(
            report.outcome,
            ModuleOutcome::Failed,
            "{:?}: {:?}",
            report.module,
            report.notes
        );
    }

    // Logical CPUs: compare with the kernel's online list.
    let online = std::fs::read_to_string("/sys/devices/system/cpu/online").unwrap();
    let expected = sentinel_hardware::parsers::cpu_list(&online).unwrap().len() as u32;
    let logical: u32 = inv
        .processors
        .iter()
        .map(|p| *p.logical_processors.value().unwrap())
        .sum();
    assert_eq!(logical, expected);
    let in_cores: usize = inv
        .processors
        .iter()
        .flat_map(|p| &p.cores)
        .map(|c| c.logical_cpus.len())
        .sum();
    assert_eq!(
        in_cores as u32, expected,
        "every logical CPU belongs to exactly one core"
    );
    // Physical cores never exceed logical CPUs, and equal the number of
    // distinct sibling groups the kernel reports.
    let mut groups = BTreeSet::new();
    for cpu in sentinel_hardware::parsers::cpu_list(&online).unwrap() {
        if let Ok(s) = std::fs::read_to_string(format!(
            "/sys/devices/system/cpu/cpu{cpu}/topology/core_cpus_list"
        )) {
            groups.insert(s.trim().to_string());
        }
    }
    let cores: usize = inv.processors.iter().map(|p| p.cores.len()).sum();
    assert_eq!(cores, groups.len());

    // Usable memory equals MemTotal.
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap();
    let total_kib: u64 = meminfo
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        *inv.memory.as_ref().unwrap().usable_bytes.value().unwrap(),
        total_kib * 1024
    );

    // Every block device with a nonzero size appears, partitions as children.
    for entry in std::fs::read_dir("/sys/block").unwrap().flatten() {
        let name = entry.file_name().into_string().unwrap();
        let size: u64 = std::fs::read_to_string(entry.path().join("size"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        if size > 0 {
            assert!(inv.storage.iter().any(|d| d.name == name), "{name} missing");
        }
    }
    // Every PCI device and network interface appears.
    assert_eq!(
        inv.pci_devices.len(),
        std::fs::read_dir("/sys/bus/pci/devices")
            .map(|d| d.count())
            .unwrap_or(0)
    );
    assert_eq!(
        inv.network.len(),
        std::fs::read_dir("/sys/class/net").unwrap().count()
    );
    // Batteries match power_supply entries of type Battery.
    let batteries = std::fs::read_dir("/sys/class/power_supply")
        .map(|d| {
            d.flatten()
                .filter(|e| {
                    std::fs::read_to_string(e.path().join("type"))
                        .map(|t| t.trim() == "Battery")
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);
    assert_eq!(inv.batteries.len(), batteries);
    // GPU kinds are never claimed with more than inferred confidence.
    for g in &inv.graphics {
        assert!(g.kind.confidence() >= Confidence::Inferred || !g.kind.is_detected());
    }
}

#[test]
fn real_machine_never_reports_restricted_firmware_data_as_detected() {
    let inv = scan(SysRoot::host());
    let readable = std::fs::File::open("/sys/firmware/dmi/entries/17-0/raw").is_ok();
    let m = inv.memory.as_ref().unwrap();
    if !readable {
        assert_eq!(m.installed_bytes.value(), None);
        assert!(matches!(
            m.installed_bytes.availability(),
            Availability::PermissionDenied | Availability::Unsupported
        ));
    }
    // Serial numbers that root can read must not be in the inventory.
    let json = serde_json::to_string(&inv).unwrap();
    for file in [
        "/sys/class/dmi/id/product_serial",
        "/sys/class/dmi/id/board_serial",
        "/sys/class/dmi/id/product_uuid",
    ] {
        if let Ok(secret) = std::fs::read_to_string(file) {
            let secret = secret.trim();
            if secret.len() > 4 {
                assert!(!json.contains(secret), "{file} leaked");
            }
        }
    }
}
