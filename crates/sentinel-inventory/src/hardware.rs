//! Hardware inventory records.
//!
//! Units are part of every numeric field name (`_bytes`, `_khz`, `_uwh`, ...)
//! and follow what the kernel reports, so nothing is rounded or rescaled on
//! the way in. Display code formats them.

use serde::{Deserialize, Serialize};

use crate::board::BoardIdentity;
use crate::discovery::ModuleReport;
use crate::fingerprint::HardwareFingerprint;
use crate::provenance::Field;

/// Version of the [`SystemInventory`] layout. Cached inventories with another
/// version are ignored, never reinterpreted.
pub const INVENTORY_SCHEMA_VERSION: u32 = 2;

/// Everything Sentinel discovered about the machine in one scan.
///
/// Every module is optional: a module that failed leaves its section empty
/// and explains why in [`SystemInventory::discovery_status`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemInventory {
    pub schema_version: u32,
    /// Unix time (seconds) when the scan started.
    pub discovered_at: u64,
    pub duration_ms: u64,
    /// Set only when discovery read from a directory other than `/` (test
    /// fixtures). Such an inventory does not describe this machine and is
    /// labelled accordingly everywhere it is shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_override: Option<String>,
    pub fingerprint: Option<HardwareFingerprint>,

    pub system: Option<SystemIdentity>,
    pub firmware: Option<FirmwareInfo>,
    pub motherboard: Option<BoardIdentity>,
    pub processors: Vec<ProcessorInfo>,
    pub memory: Option<MemoryInfo>,
    pub graphics: Vec<GpuInfo>,
    pub storage: Vec<StorageDevice>,
    pub batteries: Vec<BatteryInfo>,
    pub power_supplies: Vec<PowerSupplyInfo>,
    pub network: Vec<NetworkInterface>,
    pub usb_devices: Vec<UsbDevice>,
    pub pci_devices: Vec<PciDevice>,
    pub sensors: Vec<SensorInfo>,
    pub thermal_zones: Vec<ThermalZone>,

    pub discovery_status: Vec<ModuleReport>,
}

impl SystemInventory {
    /// Number of hardware items detected, as shown on the overview. Counts
    /// devices, not fields; virtual storage and loopback interfaces are
    /// excluded because they are not hardware.
    pub fn hardware_count(&self) -> usize {
        self.processors.len()
            + self.graphics.len()
            + self.storage.iter().filter(|d| !d.is_virtual()).count()
            + self.batteries.len()
            + self
                .network
                .iter()
                .filter(|n| n.kind != NetworkKind::Loopback && n.kind != NetworkKind::Virtual)
                .count()
            + self.usb_devices.iter().filter(|d| !d.is_root_hub).count()
            + self.pci_devices.len()
            + usize::from(self.memory.is_some())
    }
}

// ------------------------------------------------------------------ system

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemIdentity {
    pub discovered_at: u64,
    pub manufacturer: Field<String>,
    pub product_name: Field<String>,
    pub product_version: Field<String>,
    pub product_family: Field<String>,
    pub product_sku: Field<String>,
    pub chassis: Field<ChassisType>,
    pub architecture: Field<String>,
    pub kernel_release: Field<String>,
    pub kernel_version: Field<String>,
    pub os: OsInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChassisType {
    Desktop,
    Laptop,
    Convertible,
    Tablet,
    Server,
    AllInOne,
    MiniPc,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: Field<String>,
    pub pretty_name: Field<String>,
    pub id: Field<String>,
    /// `ID_LIKE` entries, e.g. `["debian"]` on Ubuntu.
    pub id_like: Vec<String>,
    pub version: Field<String>,
    pub version_id: Field<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FirmwareInfo {
    pub discovered_at: u64,
    pub bios_vendor: Field<String>,
    pub bios_version: Field<String>,
    pub bios_release_date: Field<String>,
    pub bios_revision: Field<String>,
    pub embedded_controller_revision: Field<String>,
    pub boot_mode: Field<BootMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootMode {
    Uefi,
    Legacy,
}

// --------------------------------------------------------------------- cpu

/// One physical processor package (socket).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessorInfo {
    pub discovered_at: u64,
    pub package_id: u32,
    pub vendor: Field<String>,
    /// Raw CPUID vendor string, e.g. `AuthenticAMD`.
    pub vendor_id: Field<String>,
    pub model_name: Field<String>,
    pub family: Field<u32>,
    pub model: Field<u32>,
    pub stepping: Field<u32>,
    pub microarchitecture: Field<String>,
    pub architecture: Field<String>,
    pub physical_cores: Field<u32>,
    pub logical_processors: Field<u32>,
    /// Logical CPUs the kernel lists as present but not online.
    pub offline_cpus: Vec<u32>,
    pub cores: Vec<CpuCore>,
    pub caches: Vec<CacheInfo>,
    pub flags: Field<Vec<String>>,
    pub min_frequency_khz: Field<u64>,
    pub max_frequency_khz: Field<u64>,
    /// True when cores of different kinds (performance/efficiency) were found.
    pub hybrid: Field<bool>,
}

/// One physical core and the logical processors (hardware threads) on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuCore {
    /// Position of this core within its package, 0-based, in the order of
    /// its lowest logical CPU number. Kernel core IDs need not be contiguous.
    pub index: u32,
    pub core_id: u32,
    pub die_id: Option<u32>,
    pub cluster_id: Option<u32>,
    pub logical_cpus: Vec<u32>,
    pub kind: CoreKind,
    pub max_frequency_khz: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreKind {
    /// All cores are the same kind.
    Uniform,
    Performance,
    Efficiency,
    /// The kernel did not say.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheInfo {
    pub level: u8,
    pub kind: CacheKind,
    /// Size of one instance.
    pub size_bytes: Option<u64>,
    /// Number of separate instances in the package.
    pub instances: u32,
    /// Logical CPUs sharing one instance (from the first instance found).
    pub shared_by: Vec<u32>,
    /// Logical CPUs of every instance, one entry per instance, ordered by
    /// lowest CPU number (`shared_cpu_list` of each `cacheN/indexM`). This is
    /// the kernel's sharing map, not a physical position on the die.
    #[serde(default)]
    pub instance_cpus: Vec<Vec<u32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    Data,
    Instruction,
    Unified,
    Other,
}

// ------------------------------------------------------------------ memory

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub discovered_at: u64,
    /// Physical RAM installed, from firmware memory-device records.
    pub installed_bytes: Field<u64>,
    /// Physical memory the kernel manages (online memory blocks). Usually a
    /// little less than installed RAM because firmware and integrated
    /// graphics reserve some.
    pub kernel_physical_bytes: Field<u64>,
    /// `MemTotal`: memory usable by the operating system.
    pub usable_bytes: Field<u64>,
    /// Snapshot values at scan time. Not live, not monitored.
    pub snapshot: MemorySnapshot,
    pub swap_total_bytes: Field<u64>,
    pub memory_type: Field<String>,
    pub speed_mts: Field<u32>,
    pub slots_total: Field<u32>,
    pub slots_populated: Field<u32>,
    pub modules: Field<Vec<MemoryModule>>,
    pub numa_nodes: Vec<NumaNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub available_bytes: Field<u64>,
    pub free_bytes: Field<u64>,
    pub cached_bytes: Field<u64>,
    pub used_bytes: Field<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryModule {
    /// Slot label from firmware, e.g. `DIMM 0` or `ChannelA-DIMM0`.
    pub locator: Option<String>,
    pub bank_locator: Option<String>,
    pub size_bytes: Option<u64>,
    pub memory_type: Option<String>,
    pub form_factor: Option<String>,
    pub speed_mts: Option<u32>,
    pub configured_speed_mts: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub populated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumaNode {
    pub id: u32,
    pub cpus: Vec<u32>,
    pub memory_bytes: Option<u64>,
}

// --------------------------------------------------------------------- gpu

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub discovered_at: u64,
    pub pci_address: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
    pub revision: Option<u8>,
    pub vendor_name: Field<String>,
    pub model_name: Field<String>,
    pub subsystem_name: Field<String>,
    pub driver: Field<String>,
    pub kind: Field<GpuKind>,
    /// Set by firmware on the device used for boot output.
    pub boot_display: Field<bool>,
    /// DRM nodes, e.g. `card1`, `renderD128`.
    pub drm_nodes: Vec<String>,
    pub memory: Field<GpuMemory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuKind {
    Integrated,
    Dedicated,
    Virtual,
    /// A firmware framebuffer or software renderer with no GPU behind it.
    Software,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuMemory {
    pub kind: GpuMemoryKind,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuMemoryKind {
    /// Memory on the graphics card itself.
    DedicatedVram,
    /// System RAM set aside for an integrated GPU (sometimes reported by the
    /// driver as "VRAM"). It is not separate memory.
    ReservedSystemMemory,
}

// ----------------------------------------------------------------- storage

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageDevice {
    pub discovered_at: u64,
    /// Kernel name, e.g. `nvme0n1`.
    pub name: String,
    /// Device node, e.g. `/dev/nvme0n1`. Informational only; Sentinel never
    /// opens it.
    pub device_path: String,
    pub sysfs_path: Option<String>,
    pub kind: StorageKind,
    pub interface: Field<StorageInterface>,
    pub model: Field<String>,
    pub vendor: Field<String>,
    pub firmware_revision: Field<String>,
    pub capacity_bytes: Field<u64>,
    pub logical_sector_bytes: Field<u32>,
    pub physical_sector_bytes: Field<u32>,
    pub rotational: Field<bool>,
    pub removable: Field<bool>,
    pub read_only: Field<bool>,
    /// Controller this device sits behind, when it is a PCI device.
    pub controller_pci_address: Option<String>,
    pub partitions: Vec<Partition>,
    /// How the whole device was in use at scan time (a disk formatted
    /// without a partition table, or a device-mapper volume).
    pub usage: BlockUse,
    /// Block devices this one is built on (sysfs `slaves/`), e.g. the
    /// partition under an encrypted volume. Empty for physical devices.
    pub slaves: Vec<String>,
    /// Device-mapper name (`dm/name`), the name under `/dev/mapper`.
    pub mapper_name: Option<String>,
    /// `/dev/mapper/<mapper_name>`, the node mount tables usually name.
    /// Informational only; never opened.
    pub mapper_path: Option<String>,
}

impl StorageDevice {
    pub fn is_virtual(&self) -> bool {
        self.kind == StorageKind::Virtual
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageKind {
    Ssd,
    Hdd,
    /// Removable flash such as USB sticks and SD cards.
    Flash,
    Optical,
    /// loop, zram, device-mapper, md RAID and similar: not hardware.
    Virtual,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageInterface {
    Nvme,
    Sata,
    Scsi,
    Usb,
    Mmc,
    Virtio,
    Virtual,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Partition {
    pub name: String,
    /// Device node, e.g. `/dev/nvme0n1p2`. Informational only; never opened.
    pub device_path: String,
    pub number: Option<u32>,
    pub start_sector: Option<u64>,
    pub size_bytes: Option<u64>,
    pub read_only: Option<bool>,
    pub usage: BlockUse,
}

/// What the kernel reported using a block device at scan time. Sentinel
/// never reads partition contents, so the filesystem of a device that was
/// not mounted is not known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockUse {
    /// Mounts of this device at scan time, from `/proc/mounts`. Detected
    /// (possibly empty) when the mount table could be read.
    pub mounts: Field<Vec<Mount>>,
    /// Active swap on this device at scan time, from `/proc/swaps`.
    pub swap: Field<bool>,
    /// Block devices built on this one (sysfs `holders/`), e.g. `dm-0`.
    pub holders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    pub mount_point: String,
    pub fs_type: String,
    pub read_only: bool,
}

// ----------------------------------------------------------------- battery

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub discovered_at: u64,
    /// Kernel name, e.g. `BAT0`.
    pub name: String,
    /// `System` for the machine's own battery, `Device` for a peripheral's.
    pub scope: Field<String>,
    pub manufacturer: Field<String>,
    pub model_name: Field<String>,
    pub technology: Field<String>,
    pub present: Field<bool>,
    /// Values below change constantly; they are the reading at scan time.
    pub status: Field<BatteryStatus>,
    pub capacity_percent: Field<u8>,
    pub capacity_level: Field<String>,
    pub energy_now_uwh: Field<u64>,
    pub energy_full_uwh: Field<u64>,
    pub energy_full_design_uwh: Field<u64>,
    pub charge_now_uah: Field<u64>,
    pub charge_full_uah: Field<u64>,
    pub charge_full_design_uah: Field<u64>,
    pub voltage_now_uv: Field<u64>,
    pub voltage_min_design_uv: Field<u64>,
    pub current_now_ua: Field<i64>,
    pub power_now_uw: Field<i64>,
    pub cycle_count: Field<u32>,
    /// Full capacity as a share of design capacity, computed only when both
    /// come from the same kind of measurement (energy or charge).
    pub health_percent: Field<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryStatus {
    Charging,
    Discharging,
    NotCharging,
    Full,
    Unknown,
}

/// A power supply that is not a battery: mains adapters, USB-C ports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerSupplyInfo {
    pub discovered_at: u64,
    pub name: String,
    pub kind: Field<String>,
    /// Whether it supplied power at scan time.
    pub online: Field<bool>,
}

// ----------------------------------------------------------------- network

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub discovered_at: u64,
    pub name: String,
    pub kind: NetworkKind,
    /// A local identifier. Shown to the user on request, never logged, never
    /// sent anywhere.
    pub mac_address: Field<String>,
    pub mac_address_origin: Field<MacOrigin>,
    /// Link state at scan time.
    pub operational_state: Field<String>,
    pub carrier: Field<bool>,
    pub speed_mbps: Field<u32>,
    pub mtu: Field<u32>,
    pub driver: Field<String>,
    /// PCI address or USB path of the device behind the interface.
    pub device_bus: Field<String>,
    pub vendor_id: Option<u16>,
    pub device_id: Option<u16>,
    pub wireless_phy: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkKind {
    Ethernet,
    Wireless,
    Wwan,
    Loopback,
    Bridge,
    Tunnel,
    /// Software-only interfaces: veth, docker bridges, VPN devices.
    Virtual,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacOrigin {
    Permanent,
    Random,
    Stolen,
    Set,
}

// --------------------------------------------------------------------- usb

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsbDevice {
    pub discovered_at: u64,
    /// Kernel device name, e.g. `3-3` or `usb3` for a root hub.
    pub sysfs_name: String,
    pub bus: u16,
    pub device_number: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: Field<String>,
    pub product: Field<String>,
    pub vendor_name: Field<String>,
    pub product_name: Field<String>,
    pub usb_version: Field<String>,
    pub speed_kbps: Field<u64>,
    pub device_class: u8,
    pub class_name: String,
    /// Classes of the device's interfaces, e.g. HID, Mass Storage.
    pub interface_classes: Vec<String>,
    pub is_root_hub: bool,
    pub is_hub: bool,
    /// Parent device's `sysfs_name`; `None` for root hubs.
    pub parent: Option<String>,
    /// Linux port number on the parent hub. This is not a physical socket.
    pub port_number: Option<u32>,
    /// Ports of this hub as Linux numbers them.
    pub ports: Vec<UsbPort>,
    /// Kernel/ACPI view of whether the device is built in.
    pub removable: Field<String>,
    /// PCI address of the host controller (root hubs only).
    pub controller_pci_address: Option<String>,
    pub sysfs_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbPort {
    pub number: u32,
    /// ACPI connect type: `hotplug`, `hardwired`, `not used`, `unknown`.
    pub connect_type: Option<String>,
    /// `sysfs_name` of the device on this port, if any.
    pub device: Option<String>,
}

// --------------------------------------------------------------------- pci

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PciDevice {
    pub discovered_at: u64,
    pub address: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
    pub revision: Option<u8>,
    pub class_code: u32,
    pub class_name: String,
    pub role: PciRole,
    pub vendor_name: Field<String>,
    pub device_name: Field<String>,
    pub driver: Field<String>,
    /// PCI address of the upstream bridge. This is bus topology, not an
    /// electrical wiring diagram.
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PciRole {
    Graphics,
    Network,
    UsbController,
    StorageController,
    Audio,
    Bridge,
    Other,
}

// ----------------------------------------------------------------- sensors

/// A sensor interface. Phase 2 records that it exists and where it is; its
/// readings belong to the telemetry engine and are not collected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorInfo {
    pub discovered_at: u64,
    /// Stable within a boot, e.g. `hwmon4/temp1`. hwmon numbers can change
    /// between boots.
    pub id: String,
    pub chip: String,
    pub kind: SensorKind,
    pub label: Field<String>,
    /// Path of the reading attribute, e.g. `/sys/class/hwmon/hwmon4/temp1_input`.
    pub input_path: String,
    /// Whether the current user may read it.
    pub access: Field<SensorAccess>,
    /// The device the kernel attaches the chip to (e.g. a PCI address). It
    /// says which driver provides the sensor, not which component it measures.
    pub device: Field<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    Temperature,
    Fan,
    Voltage,
    Current,
    Power,
    Energy,
    Frequency,
    Humidity,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorAccess {
    Readable,
    PermissionDenied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThermalZone {
    pub discovered_at: u64,
    pub name: String,
    pub zone_type: Field<String>,
    pub access: Field<SensorAccess>,
    pub trip_points: u32,
}

// ------------------------------------------------------------ capabilities

/// Which Linux information sources exist on this machine and are readable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    pub checked_at: u64,
    pub interfaces: Vec<InterfaceCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceCapability {
    pub id: String,
    pub description: String,
    pub path: String,
    pub status: CapabilityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Available,
    /// Present, but some of it is restricted to administrators.
    Partial,
    PermissionDenied,
    Missing,
}
