//! A navigable index of the components in a [`SystemInventory`].
//!
//! The inventory stays the single description of the hardware. The index
//! adds what an explorer needs on top of it and nothing else:
//!
//! - a stable identifier for every component, built from kernel names and
//!   bus addresses, so a selection survives a refresh or a rescan of the
//!   same hardware;
//! - parent/child relationships as the kernel or firmware reports them,
//!   with how each one is known ([`ParentBasis`]);
//! - links between components in different sections (a GPU and its PCI
//!   function, a disk and its controller, a cache and the cores sharing it);
//! - references into live telemetry ([`TelemetryRef`]), so readings can be
//!   attached to a component without collecting anything twice.
//!
//! Specifications are not copied: each component carries the `key` of its
//! record in the inventory. Building the index reads nothing from the
//! system; it is a pure function of the inventory.
//!
//! Every relationship here is logical (kernel topology, bus topology). None
//! of it describes where anything physically sits on a die or a board.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::hardware::{CacheKind, CoreKind, PciRole, StorageKind, SystemInventory};
use crate::provenance::Field;

/// Version of the [`ComponentIndex`] layout.
pub const COMPONENT_INDEX_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentIndex {
    pub version: u32,
    /// `discovered_at` of the inventory the index was built from.
    pub inventory_discovered_at: u64,
    /// Parents come before their children; otherwise in discovery order.
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentCategory {
    Processor,
    Memory,
    Graphics,
    Storage,
    Battery,
    Network,
    Usb,
    Pci,
    Motherboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    CpuPackage,
    CpuCore,
    LogicalCpu,
    Cache,
    Memory,
    /// A firmware memory-device record: a module, soldered memory or an
    /// empty slot, exactly as the firmware lists it.
    MemoryDevice,
    NumaNode,
    Gpu,
    Disk,
    Partition,
    /// loop, zram, device-mapper, md RAID: not hardware.
    VirtualBlock,
    Battery,
    PowerSupply,
    NetworkInterface,
    UsbRootHub,
    UsbHub,
    UsbDevice,
    PciDevice,
    PciBridge,
    Board,
}

/// How a component's parent is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParentBasis {
    /// A top-level component of its category.
    Root,
    /// Kernel topology: CPU topology in sysfs, partitions under their disk,
    /// device-mapper `slaves/`.
    KernelTopology,
    /// The kernel's device tree for a bus (PCI bridges, USB hubs).
    BusTopology,
    /// Firmware tables (SMBIOS memory devices).
    Firmware,
    /// The inventory names a parent that was not itself discovered, so the
    /// relationship cannot be shown. Such components are listed at the top
    /// level and say so; they are never attached to a guessed parent.
    ParentNotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// The PCI function that implements this component.
    PciFunction,
    /// The controller this device is attached through.
    Controller,
    /// The USB device that implements this component.
    UsbDevice,
    /// A cache this core uses.
    Cache,
    /// A core that uses this cache.
    SharedBy,
    /// A block device this one is built on.
    BuiltOn,
    /// A block device built on this one.
    UsedBy,
    /// A logical CPU local to this NUMA node.
    LocalCpu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    pub relation: Relation,
}

/// Where live readings for a component are found in a telemetry snapshot.
/// The telemetry engine keys its samples the same way discovery names the
/// hardware (kernel CPU numbers, PCI addresses, kernel device names).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum TelemetryRef {
    /// Whole-processor utilisation and average frequency.
    CpuTotal,
    /// One logical CPU's utilisation and frequency.
    LogicalCpu {
        cpu: u32,
    },
    Memory,
    Gpu {
        pci_address: String,
    },
    /// I/O of a physical disk.
    Disk {
        name: String,
    },
    /// Usage of a filesystem mounted from this device node.
    Filesystem {
        device: String,
    },
    NetworkInterface {
        name: String,
    },
    Battery {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    /// Stable identifier, e.g. `cpu:0/core:3`, `pci:0000:03:00.0`.
    pub id: String,
    pub category: ComponentCategory,
    pub kind: ComponentKind,
    /// Display name from detected values; falls back to the kernel's name
    /// or numeric IDs, never to an invented model.
    pub name: String,
    /// Key of the component's record in its inventory section: package ID,
    /// core index, logical CPU number, cache `level:kind:instance`, memory
    /// device position, PCI address, kernel device name.
    pub key: String,
    pub parent: Option<String>,
    pub parent_basis: ParentBasis,
    pub children: Vec<String>,
    pub links: Vec<Link>,
    pub telemetry: Vec<TelemetryRef>,
}

impl ComponentIndex {
    pub fn get(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    /// Builds the index. Pure: reads nothing but the inventory.
    pub fn build(inv: &SystemInventory) -> Self {
        let mut b = Builder::default();
        processors(inv, &mut b);
        memory(inv, &mut b);
        graphics(inv, &mut b);
        storage(inv, &mut b);
        power(inv, &mut b);
        network(inv, &mut b);
        usb(inv, &mut b);
        pci(inv, &mut b);
        board(inv, &mut b);
        b.finish(inv.discovered_at)
    }
}

pub fn cache_kind_code(kind: CacheKind) -> &'static str {
    match kind {
        CacheKind::Data => "data",
        CacheKind::Instruction => "instruction",
        CacheKind::Unified => "unified",
        CacheKind::Other => "other",
    }
}

pub fn pci_id(address: &str) -> String {
    format!("pci:{address}")
}

pub fn usb_id(sysfs_name: &str) -> String {
    format!("usb:{sysfs_name}")
}

pub fn storage_id(name: &str) -> String {
    format!("storage:{name}")
}

// ------------------------------------------------------------------ builder

#[derive(Default)]
struct Builder {
    components: Vec<Component>,
    /// Parent claims that still have to be checked: (child, parent).
    pending: Vec<(usize, String, ParentBasis)>,
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        id: String,
        category: ComponentCategory,
        kind: ComponentKind,
        name: String,
        key: String,
        parent: Option<(String, ParentBasis)>,
        telemetry: Vec<TelemetryRef>,
    ) -> usize {
        let index = self.components.len();
        if let Some((parent, basis)) = parent {
            self.pending.push((index, parent, basis));
        }
        self.components.push(Component {
            id,
            category,
            kind,
            name,
            key,
            parent: None,
            parent_basis: ParentBasis::Root,
            children: Vec::new(),
            links: Vec::new(),
            telemetry,
        });
        index
    }

    fn link(&mut self, index: usize, id: String, relation: Relation) {
        self.components[index].links.push(Link { id, relation });
    }

    fn finish(mut self, discovered_at: u64) -> ComponentIndex {
        // Identifiers are unique by construction; a duplicate would be a
        // bug in the inventory (two devices with one kernel name), and the
        // later one is renamed rather than silently merged.
        let mut seen = BTreeSet::new();
        for c in &mut self.components {
            let mut id = c.id.clone();
            let mut n = 2;
            while !seen.insert(id.clone()) {
                id = format!("{}#{n}", c.id);
                n += 1;
            }
            c.id = id;
        }
        let position: BTreeMap<String, usize> = self
            .components
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id.clone(), i))
            .collect();
        for (child, parent, basis) in std::mem::take(&mut self.pending) {
            match position.get(&parent) {
                Some(&p) if p != child => {
                    let child_id = self.components[child].id.clone();
                    self.components[child].parent = Some(parent);
                    self.components[child].parent_basis = basis;
                    self.components[p].children.push(child_id);
                }
                _ => self.components[child].parent_basis = ParentBasis::ParentNotFound,
            }
        }
        // Links only to components that exist.
        for c in &mut self.components {
            c.links.retain(|l| position.contains_key(&l.id));
        }
        // Parents first: a stable depth-first order of each category.
        let mut ordered = Vec::with_capacity(self.components.len());
        let mut placed = vec![false; self.components.len()];
        fn visit(
            i: usize,
            comps: &[Component],
            position: &BTreeMap<String, usize>,
            placed: &mut [bool],
            out: &mut Vec<usize>,
        ) {
            if placed[i] {
                return;
            }
            placed[i] = true;
            out.push(i);
            for child in &comps[i].children {
                if let Some(&c) = position.get(child) {
                    visit(c, comps, position, placed, out);
                }
            }
        }
        for i in 0..self.components.len() {
            if self.components[i].parent.is_none() {
                visit(i, &self.components, &position, &mut placed, &mut ordered);
            }
        }
        // A cycle cannot come from sysfs, but never drop a component.
        for (i, done) in placed.clone().iter().enumerate() {
            if !done {
                ordered.push(i);
            }
        }
        let mut slots: Vec<Option<Component>> = self.components.into_iter().map(Some).collect();
        ComponentIndex {
            version: COMPONENT_INDEX_VERSION,
            inventory_discovered_at: discovered_at,
            components: ordered
                .into_iter()
                .filter_map(|i| slots[i].take())
                .collect(),
        }
    }
}

fn text(field: &Field<String>) -> Option<String> {
    field
        .value()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// --------------------------------------------------------------- categories

fn processors(inv: &SystemInventory, b: &mut Builder) {
    use ComponentCategory::Processor as Cat;
    for cpu in &inv.processors {
        let pkg = format!("cpu:{}", cpu.package_id);
        b.add(
            pkg.clone(),
            Cat,
            ComponentKind::CpuPackage,
            text(&cpu.model_name).unwrap_or_else(|| format!("Processor {}", cpu.package_id)),
            cpu.package_id.to_string(),
            None,
            vec![TelemetryRef::CpuTotal],
        );
        let known: BTreeSet<u32> = cpu
            .cores
            .iter()
            .flat_map(|c| c.logical_cpus.clone())
            .collect();
        let mut core_index = Vec::new();
        for core in &cpu.cores {
            let id = format!("{pkg}/core:{}", core.index);
            let kind = match core.kind {
                CoreKind::Performance => " (performance)",
                CoreKind::Efficiency => " (efficiency)",
                _ => "",
            };
            let i = b.add(
                id.clone(),
                Cat,
                ComponentKind::CpuCore,
                format!("Core {}{kind}", core.index),
                core.index.to_string(),
                Some((pkg.clone(), ParentBasis::KernelTopology)),
                core.logical_cpus
                    .iter()
                    .map(|&cpu| TelemetryRef::LogicalCpu { cpu })
                    .collect(),
            );
            core_index.push((i, core.logical_cpus.clone()));
            for &logical in &core.logical_cpus {
                b.add(
                    format!("{pkg}/cpu:{logical}"),
                    Cat,
                    ComponentKind::LogicalCpu,
                    format!("CPU {logical}"),
                    logical.to_string(),
                    Some((id.clone(), ParentBasis::KernelTopology)),
                    vec![TelemetryRef::LogicalCpu { cpu: logical }],
                );
            }
        }
        for cache in &cpu.caches {
            let kind = cache_kind_code(cache.kind);
            let instances: Vec<Vec<u32>> = if cache.instance_cpus.is_empty() {
                vec![cache.shared_by.clone()]
            } else {
                cache.instance_cpus.clone()
            };
            for (n, cpus) in instances.iter().enumerate() {
                // The kernel may list CPUs that are not present; only CPUs
                // of this package's cores are linked.
                let cpus: BTreeSet<u32> =
                    cpus.iter().copied().filter(|c| known.contains(c)).collect();
                let label = match cache.kind {
                    CacheKind::Data => "data",
                    CacheKind::Instruction => "instruction",
                    _ => "",
                };
                let name = if instances.len() == 1 {
                    format!("L{} {label} cache", cache.level)
                } else {
                    format!("L{} {label} cache {n}", cache.level)
                };
                let i = b.add(
                    format!("{pkg}/cache:l{}-{kind}-{n}", cache.level),
                    Cat,
                    ComponentKind::Cache,
                    name.split_whitespace().collect::<Vec<_>>().join(" "),
                    format!("{}:{kind}:{n}", cache.level),
                    Some((pkg.clone(), ParentBasis::KernelTopology)),
                    Vec::new(),
                );
                let cache_id = b.components[i].id.clone();
                for (core, logical) in &core_index {
                    if logical.iter().any(|c| cpus.contains(c)) {
                        let core_id = b.components[*core].id.clone();
                        b.link(i, core_id, Relation::SharedBy);
                        b.link(*core, cache_id.clone(), Relation::Cache);
                    }
                }
            }
        }
    }
}

fn memory(inv: &SystemInventory, b: &mut Builder) {
    use ComponentCategory::Memory as Cat;
    let Some(m) = &inv.memory else { return };
    b.add(
        "memory".into(),
        Cat,
        ComponentKind::Memory,
        "System memory".into(),
        "memory".into(),
        None,
        vec![TelemetryRef::Memory],
    );
    if let Some(modules) = m.modules.value() {
        for (n, module) in modules.iter().enumerate() {
            let locator = module
                .locator
                .as_deref()
                .map(str::trim)
                .filter(|l| !l.is_empty());
            b.add(
                format!("memory/device:{}", locator.unwrap_or(&format!("#{n}"))),
                Cat,
                ComponentKind::MemoryDevice,
                locator
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("Memory device {}", n + 1)),
                n.to_string(),
                Some(("memory".into(), ParentBasis::Firmware)),
                Vec::new(),
            );
        }
    }
    let cpu_ids: BTreeMap<u32, String> = inv
        .processors
        .iter()
        .flat_map(|p| {
            p.cores
                .iter()
                .flat_map(|c| c.logical_cpus.clone())
                .map(move |l| (l, format!("cpu:{}/cpu:{l}", p.package_id)))
        })
        .collect();
    for node in &m.numa_nodes {
        let i = b.add(
            format!("memory/numa:{}", node.id),
            Cat,
            ComponentKind::NumaNode,
            format!("NUMA node {}", node.id),
            node.id.to_string(),
            Some(("memory".into(), ParentBasis::KernelTopology)),
            Vec::new(),
        );
        for cpu in &node.cpus {
            if let Some(id) = cpu_ids.get(cpu) {
                b.link(i, id.clone(), Relation::LocalCpu);
            }
        }
    }
}

fn graphics(inv: &SystemInventory, b: &mut Builder) {
    for g in &inv.graphics {
        let i = b.add(
            format!("gpu:{}", g.pci_address),
            ComponentCategory::Graphics,
            ComponentKind::Gpu,
            text(&g.model_name)
                .unwrap_or_else(|| format!("GPU {:04x}:{:04x}", g.vendor_id, g.device_id)),
            g.pci_address.clone(),
            None,
            vec![TelemetryRef::Gpu {
                pci_address: g.pci_address.clone(),
            }],
        );
        b.link(i, pci_id(&g.pci_address), Relation::PciFunction);
    }
}

fn storage(inv: &SystemInventory, b: &mut Builder) {
    use ComponentCategory::Storage as Cat;
    // Mount tables name a filesystem by device node; discovery recorded
    // each device's nodes, so the keys are copied, never constructed.
    let fs = |node: &str| TelemetryRef::Filesystem {
        device: node.to_string(),
    };
    for d in &inv.storage {
        let virtual_dev = d.kind == StorageKind::Virtual;
        let name = if virtual_dev {
            match &d.mapper_name {
                Some(m) => format!("{} ({m})", d.name),
                None => d.name.clone(),
            }
        } else {
            text(&d.model).unwrap_or_else(|| d.name.clone())
        };
        let mut telemetry = vec![fs(&d.device_path)];
        if let Some(mapper) = &d.mapper_path {
            telemetry.push(fs(mapper));
        }
        if !virtual_dev {
            telemetry.insert(
                0,
                TelemetryRef::Disk {
                    name: d.name.clone(),
                },
            );
        }
        // A device built on others (an encrypted volume on a partition) is
        // shown under the first device it is built on.
        let parent = d
            .slaves
            .first()
            .map(|s| (storage_id(s), ParentBasis::KernelTopology));
        let i = b.add(
            storage_id(&d.name),
            Cat,
            if virtual_dev {
                ComponentKind::VirtualBlock
            } else {
                ComponentKind::Disk
            },
            name,
            d.name.clone(),
            parent,
            telemetry,
        );
        if let Some(pci) = &d.controller_pci_address {
            b.link(i, pci_id(pci), Relation::Controller);
        }
        for s in &d.slaves {
            b.link(i, storage_id(s), Relation::BuiltOn);
        }
        for h in &d.usage.holders {
            b.link(i, storage_id(h), Relation::UsedBy);
        }
        for p in &d.partitions {
            let pi = b.add(
                storage_id(&p.name),
                Cat,
                ComponentKind::Partition,
                match p.number {
                    Some(n) => format!("{} (partition {n})", p.name),
                    None => p.name.clone(),
                },
                p.name.clone(),
                Some((storage_id(&d.name), ParentBasis::KernelTopology)),
                vec![fs(&p.device_path)],
            );
            for h in &p.usage.holders {
                b.link(pi, storage_id(h), Relation::UsedBy);
            }
        }
    }
}

fn power(inv: &SystemInventory, b: &mut Builder) {
    for bat in &inv.batteries {
        b.add(
            format!("battery:{}", bat.name),
            ComponentCategory::Battery,
            ComponentKind::Battery,
            match text(&bat.model_name) {
                Some(model) => format!("{} ({model})", bat.name),
                None => bat.name.clone(),
            },
            bat.name.clone(),
            None,
            vec![TelemetryRef::Battery {
                name: bat.name.clone(),
            }],
        );
    }
    for ps in &inv.power_supplies {
        b.add(
            format!("power:{}", ps.name),
            ComponentCategory::Battery,
            ComponentKind::PowerSupply,
            ps.name.clone(),
            ps.name.clone(),
            None,
            Vec::new(),
        );
    }
}

fn network(inv: &SystemInventory, b: &mut Builder) {
    for n in &inv.network {
        let i = b.add(
            format!("net:{}", n.name),
            ComponentCategory::Network,
            ComponentKind::NetworkInterface,
            n.name.clone(),
            n.name.clone(),
            None,
            vec![TelemetryRef::NetworkInterface {
                name: n.name.clone(),
            }],
        );
        match n.device_bus.value().map(String::as_str) {
            Some(bus) if bus.starts_with("PCI ") => {
                b.link(i, pci_id(&bus[4..]), Relation::PciFunction)
            }
            Some(bus) if bus.starts_with("USB ") => {
                b.link(i, usb_id(&bus[4..]), Relation::UsbDevice)
            }
            _ => {}
        }
    }
}

fn usb(inv: &SystemInventory, b: &mut Builder) {
    for d in &inv.usb_devices {
        let name = text(&d.product)
            .or_else(|| text(&d.product_name))
            .unwrap_or_else(|| format!("{:04x}:{:04x}", d.vendor_id, d.product_id));
        let (kind, name) = if d.is_root_hub {
            (
                ComponentKind::UsbRootHub,
                format!("{} · {name}", d.sysfs_name),
            )
        } else if d.is_hub {
            (ComponentKind::UsbHub, name)
        } else {
            (ComponentKind::UsbDevice, name)
        };
        let i = b.add(
            usb_id(&d.sysfs_name),
            ComponentCategory::Usb,
            kind,
            name,
            d.sysfs_name.clone(),
            d.parent
                .as_ref()
                .map(|p| (usb_id(p), ParentBasis::BusTopology)),
            Vec::new(),
        );
        if let Some(pci) = &d.controller_pci_address {
            b.link(i, pci_id(pci), Relation::Controller);
        }
    }
}

fn pci(inv: &SystemInventory, b: &mut Builder) {
    for d in &inv.pci_devices {
        b.add(
            pci_id(&d.address),
            ComponentCategory::Pci,
            if d.role == PciRole::Bridge {
                ComponentKind::PciBridge
            } else {
                ComponentKind::PciDevice
            },
            text(&d.device_name).unwrap_or_else(|| d.class_name.clone()),
            d.address.clone(),
            d.parent
                .as_ref()
                .map(|p| (pci_id(p), ParentBasis::BusTopology)),
            Vec::new(),
        );
    }
}

fn board(inv: &SystemInventory, b: &mut Builder) {
    let Some(board) = &inv.motherboard else {
        return;
    };
    let name = text(&board.board_name)
        .or_else(|| text(&board.system_model))
        .unwrap_or_else(|| "Motherboard".into());
    b.add(
        "board".into(),
        ComponentCategory::Motherboard,
        ComponentKind::Board,
        name,
        "board".into(),
        None,
        Vec::new(),
    );
}
