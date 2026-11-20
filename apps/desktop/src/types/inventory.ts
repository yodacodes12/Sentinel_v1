// Mirrors the Rust inventory model (crates/sentinel-inventory) as the
// desktop bridge serialises it. Field names and enum strings match serde.

export type Availability = "detected" | "unknown" | "unsupported" | "unavailable" | "permission_denied" | "invalid";
export type Confidence = "direct" | "firmware_reported" | "kernel_reported" | "database" | "derived" | "inferred" | "unknown";

/** A discovered value with provenance. `value` is present only when detected. */
export interface Field<T> {
  value?: T;
  availability: Availability;
  confidence: Confidence;
  source?: string;
  notes?: string;
}

export type DiscoveryJob = "hardware" | "utilities";
export type ModuleId =
  | "system_identity"
  | "firmware"
  | "cpu"
  | "memory"
  | "motherboard"
  | "gpu"
  | "storage"
  | "battery"
  | "network"
  | "usb"
  | "pci"
  | "sensors"
  | "desktop_entries"
  | "packages"
  | "executables"
  | "services";
export type ModuleOutcome = "completed" | "partial" | "unavailable" | "failed" | "cancelled";
export type JobState = "idle" | "queued" | "running" | "completed" | "cancelled" | "timed_out" | "failed";
export type InventoryOrigin = "this_session" | "cache";

export interface ModuleReport {
  module: ModuleId;
  outcome: ModuleOutcome;
  items: number;
  duration_ms: number;
  finished_at: number;
  notes: string[];
}

export interface JobProgress {
  job: DiscoveryJob;
  state: JobState;
  started_at: number | null;
  finished_at: number | null;
  current_module: ModuleId | null;
  modules: ModuleReport[];
  last_successful_scan: number | null;
  error: string | null;
  cache_saved: boolean | null;
}

export interface DiscoveryStatus {
  hardware: JobProgress;
  utilities: JobProgress;
}

export interface OsInfo {
  name: Field<string>;
  pretty_name: Field<string>;
  id: Field<string>;
  id_like: string[];
  version: Field<string>;
  version_id: Field<string>;
}

export interface SystemIdentity {
  discovered_at: number;
  manufacturer: Field<string>;
  product_name: Field<string>;
  product_version: Field<string>;
  product_family: Field<string>;
  product_sku: Field<string>;
  chassis: Field<string>;
  architecture: Field<string>;
  kernel_release: Field<string>;
  kernel_version: Field<string>;
  os: OsInfo;
}

export interface FirmwareInfo {
  discovered_at: number;
  bios_vendor: Field<string>;
  bios_version: Field<string>;
  bios_release_date: Field<string>;
  bios_revision: Field<string>;
  embedded_controller_revision: Field<string>;
  boot_mode: Field<"uefi" | "legacy">;
}

export type IdentificationLevel = "board_identified" | "system_only" | "unidentified";

export interface BoardIdentity {
  discovered_at: number;
  system_manufacturer: Field<string>;
  system_model: Field<string>;
  system_family: Field<string>;
  system_sku: Field<string>;
  board_manufacturer: Field<string>;
  board_name: Field<string>;
  board_revision: Field<string>;
  bios_vendor: Field<string>;
  bios_version: Field<string>;
  bios_release_date: Field<string>;
  identification: { level: IdentificationLevel; explanation: string; externally_verified: boolean };
}

export type CoreKind = "uniform" | "performance" | "efficiency" | "unknown";

export interface CpuCore {
  index: number;
  core_id: number;
  die_id: number | null;
  cluster_id: number | null;
  logical_cpus: number[];
  kind: CoreKind;
  max_frequency_khz: number | null;
}

export interface CacheInfo {
  level: number;
  kind: "data" | "instruction" | "unified" | "other";
  size_bytes: number | null;
  instances: number;
  shared_by: number[];
  /** Logical CPUs of every instance, ordered by lowest CPU. */
  instance_cpus: number[][];
}

export interface ProcessorInfo {
  discovered_at: number;
  package_id: number;
  vendor: Field<string>;
  vendor_id: Field<string>;
  model_name: Field<string>;
  family: Field<number>;
  model: Field<number>;
  stepping: Field<number>;
  microarchitecture: Field<string>;
  architecture: Field<string>;
  physical_cores: Field<number>;
  logical_processors: Field<number>;
  offline_cpus: number[];
  cores: CpuCore[];
  caches: CacheInfo[];
  flags: Field<string[]>;
  min_frequency_khz: Field<number>;
  max_frequency_khz: Field<number>;
  hybrid: Field<boolean>;
}

export interface MemoryModule {
  locator: string | null;
  bank_locator: string | null;
  size_bytes: number | null;
  memory_type: string | null;
  form_factor: string | null;
  speed_mts: number | null;
  configured_speed_mts: number | null;
  manufacturer: string | null;
  part_number: string | null;
  populated: boolean;
}

export interface MemoryInfo {
  discovered_at: number;
  installed_bytes: Field<number>;
  kernel_physical_bytes: Field<number>;
  usable_bytes: Field<number>;
  snapshot: {
    available_bytes: Field<number>;
    free_bytes: Field<number>;
    cached_bytes: Field<number>;
    used_bytes: Field<number>;
  };
  swap_total_bytes: Field<number>;
  memory_type: Field<string>;
  speed_mts: Field<number>;
  slots_total: Field<number>;
  slots_populated: Field<number>;
  modules: Field<MemoryModule[]>;
  numa_nodes: { id: number; cpus: number[]; memory_bytes: number | null }[];
}

export type GpuKind = "integrated" | "dedicated" | "virtual" | "software";

export interface GpuInfo {
  discovered_at: number;
  pci_address: string;
  vendor_id: number;
  device_id: number;
  subsystem_vendor_id: number | null;
  subsystem_device_id: number | null;
  revision: number | null;
  vendor_name: Field<string>;
  model_name: Field<string>;
  subsystem_name: Field<string>;
  driver: Field<string>;
  kind: Field<GpuKind>;
  boot_display: Field<boolean>;
  drm_nodes: string[];
  memory: Field<{ kind: "dedicated_vram" | "reserved_system_memory"; bytes: number }>;
}

export interface Partition {
  name: string;
  device_path: string;
  number: number | null;
  start_sector: number | null;
  size_bytes: number | null;
  read_only: boolean | null;
  usage: BlockUse;
}

/** How the kernel reported a block device in use at scan time. */
export interface BlockUse {
  mounts: Field<Mount[]>;
  swap: Field<boolean>;
  holders: string[];
}

export interface Mount {
  mount_point: string;
  fs_type: string;
  read_only: boolean;
}

export type StorageKind = "ssd" | "hdd" | "flash" | "optical" | "virtual" | "unknown";

export interface StorageDevice {
  discovered_at: number;
  name: string;
  device_path: string;
  sysfs_path: string | null;
  kind: StorageKind;
  interface: Field<string>;
  model: Field<string>;
  vendor: Field<string>;
  firmware_revision: Field<string>;
  capacity_bytes: Field<number>;
  logical_sector_bytes: Field<number>;
  physical_sector_bytes: Field<number>;
  rotational: Field<boolean>;
  removable: Field<boolean>;
  read_only: Field<boolean>;
  controller_pci_address: string | null;
  partitions: Partition[];
  usage: BlockUse;
  slaves: string[];
  mapper_name: string | null;
  mapper_path: string | null;
}

export interface BatteryInfo {
  discovered_at: number;
  name: string;
  scope: Field<string>;
  manufacturer: Field<string>;
  model_name: Field<string>;
  technology: Field<string>;
  present: Field<boolean>;
  status: Field<"charging" | "discharging" | "not_charging" | "full" | "unknown">;
  capacity_percent: Field<number>;
  capacity_level: Field<string>;
  energy_now_uwh: Field<number>;
  energy_full_uwh: Field<number>;
  energy_full_design_uwh: Field<number>;
  charge_now_uah: Field<number>;
  charge_full_uah: Field<number>;
  charge_full_design_uah: Field<number>;
  voltage_now_uv: Field<number>;
  voltage_min_design_uv: Field<number>;
  current_now_ua: Field<number>;
  power_now_uw: Field<number>;
  cycle_count: Field<number>;
  health_percent: Field<number>;
}

export interface PowerSupplyInfo {
  discovered_at: number;
  name: string;
  kind: Field<string>;
  online: Field<boolean>;
}

export type NetworkKind = "ethernet" | "wireless" | "wwan" | "loopback" | "bridge" | "tunnel" | "virtual" | "other";

export interface NetworkInterface {
  discovered_at: number;
  name: string;
  kind: NetworkKind;
  mac_address: Field<string>;
  mac_address_origin: Field<"permanent" | "random" | "stolen" | "set">;
  operational_state: Field<string>;
  carrier: Field<boolean>;
  speed_mbps: Field<number>;
  mtu: Field<number>;
  driver: Field<string>;
  device_bus: Field<string>;
  vendor_id: number | null;
  device_id: number | null;
  wireless_phy: string | null;
}

export interface UsbPort {
  number: number;
  connect_type: string | null;
  device: string | null;
}

export interface UsbDevice {
  discovered_at: number;
  sysfs_name: string;
  bus: number;
  device_number: number;
  vendor_id: number;
  product_id: number;
  manufacturer: Field<string>;
  product: Field<string>;
  vendor_name: Field<string>;
  product_name: Field<string>;
  usb_version: Field<string>;
  speed_kbps: Field<number>;
  device_class: number;
  class_name: string;
  interface_classes: string[];
  is_root_hub: boolean;
  is_hub: boolean;
  parent: string | null;
  port_number: number | null;
  ports: UsbPort[];
  removable: Field<string>;
  controller_pci_address: string | null;
  sysfs_path: string;
}

export interface PciDevice {
  discovered_at: number;
  address: string;
  vendor_id: number;
  device_id: number;
  subsystem_vendor_id: number | null;
  subsystem_device_id: number | null;
  revision: number | null;
  class_code: number;
  class_name: string;
  role: "graphics" | "network" | "usb_controller" | "storage_controller" | "audio" | "bridge" | "other";
  vendor_name: Field<string>;
  device_name: Field<string>;
  driver: Field<string>;
  parent: string | null;
}

export interface SensorInfo {
  discovered_at: number;
  id: string;
  chip: string;
  kind: "temperature" | "fan" | "voltage" | "current" | "power" | "energy" | "frequency" | "humidity" | "other";
  label: Field<string>;
  input_path: string;
  access: Field<"readable" | "permission_denied">;
  device: Field<string>;
}

export interface ThermalZone {
  discovered_at: number;
  name: string;
  zone_type: Field<string>;
  access: Field<"readable" | "permission_denied">;
  trip_points: number;
}

export interface SystemInventory {
  schema_version: number;
  discovered_at: number;
  duration_ms: number;
  root_override?: string;
  fingerprint: { version: number; value: string; inputs: string[] } | null;
  system: SystemIdentity | null;
  firmware: FirmwareInfo | null;
  motherboard: BoardIdentity | null;
  processors: ProcessorInfo[];
  memory: MemoryInfo | null;
  graphics: GpuInfo[];
  storage: StorageDevice[];
  batteries: BatteryInfo[];
  power_supplies: PowerSupplyInfo[];
  network: NetworkInterface[];
  usb_devices: UsbDevice[];
  pci_devices: PciDevice[];
  sensors: SensorInfo[];
  thermal_zones: ThermalZone[];
  discovery_status: ModuleReport[];
}

export type ViewSource = "daemon" | "cache_file";

export interface InventoryView {
  inventory: SystemInventory | null;
  /** Explorer index built from `inventory` (Rust `ComponentIndex`). */
  components: ComponentIndex | null;
  origin: InventoryOrigin | null;
  source: ViewSource;
  progress: JobProgress | null;
  warning: string | null;
}

// ---------------------------------------------------------------- utilities

export type UtilityCategory =
  | "system_monitoring"
  | "hardware_information"
  | "disk_management"
  | "storage_health"
  | "battery_and_power"
  | "sensors_and_temperatures"
  | "network_management"
  | "network_diagnostics"
  | "process_management"
  | "system_logs"
  | "service_management"
  | "package_management"
  | "graphics_and_drivers"
  | "backup_and_recovery"
  | "system_configuration";

export type DiscoverySource = "desktop_entry" | "package_database" | "executable" | "catalogue";
export type PackageManager = "dpkg" | "rpm" | "pacman" | "flatpak" | "snap";

export interface SystemUtility {
  id: string;
  name: string;
  description: string | null;
  category: UtilityCategory;
  interface: "graphical" | "terminal" | "command_line";
  installed: boolean;
  version: string | null;
  executable: string | null;
  desktop_entry: {
    id: string;
    path: string;
    scope: "system" | "user" | "flatpak" | "snap";
    terminal: boolean;
    no_display: boolean;
  } | null;
  package: { manager: PackageManager; name: string; version: string | null } | null;
  icon: string | null;
  sources: DiscoverySource[];
  launch_available: boolean;
  launch_note: string | null;
}

export interface ServiceInfo {
  name: string;
  description: string | null;
  scope: "system" | "user";
  load_state: string | null;
  active_state: string | null;
  sub_state: string | null;
  enabled_state: string | null;
  unit_file_path: string | null;
  relevant_category: UtilityCategory | null;
}

export interface UtilityCatalog {
  schema_version: number;
  discovered_at: number;
  duration_ms: number;
  root_override?: string;
  utilities: SystemUtility[];
  services: ServiceInfo[];
  package_managers: {
    manager: PackageManager;
    database_found: boolean;
    installed_packages: number | null;
    note: string | null;
  }[];
  discovery_status: ModuleReport[];
}

export interface UtilitiesView {
  catalog: UtilityCatalog | null;
  origin: InventoryOrigin | null;
  source: ViewSource;
  progress: JobProgress | null;
  warning: string | null;
}

export interface HardwareCapabilities {
  checked_at: number;
  interfaces: {
    id: string;
    description: string;
    path: string;
    status: "available" | "partial" | "permission_denied" | "missing";
    note?: string;
  }[];
}

// ------------------------------------------------------------ explorer index
// Mirrors sentinel-inventory/src/components.rs.

export type ComponentCategory = "processor" | "memory" | "graphics" | "storage" | "battery" | "network" | "usb" | "pci" | "motherboard";

export type ComponentKind =
  | "cpu_package"
  | "cpu_core"
  | "logical_cpu"
  | "cache"
  | "memory"
  | "memory_device"
  | "numa_node"
  | "gpu"
  | "disk"
  | "partition"
  | "virtual_block"
  | "battery"
  | "power_supply"
  | "network_interface"
  | "usb_root_hub"
  | "usb_hub"
  | "usb_device"
  | "pci_device"
  | "pci_bridge"
  | "board";

export type ParentBasis = "root" | "kernel_topology" | "bus_topology" | "firmware" | "parent_not_found";

export type Relation = "pci_function" | "controller" | "usb_device" | "cache" | "shared_by" | "built_on" | "used_by" | "local_cpu";

export type TelemetryRef =
  | { source: "cpu_total" }
  | { source: "logical_cpu"; cpu: number }
  | { source: "memory" }
  | { source: "gpu"; pci_address: string }
  | { source: "disk"; name: string }
  | { source: "filesystem"; device: string }
  | { source: "network_interface"; name: string }
  | { source: "battery"; name: string };

export interface ComponentNode {
  id: string;
  category: ComponentCategory;
  kind: ComponentKind;
  name: string;
  key: string;
  parent: string | null;
  parent_basis: ParentBasis;
  children: string[];
  links: { id: string; relation: Relation }[];
  telemetry: TelemetryRef[];
}

export interface ComponentIndex {
  version: number;
  inventory_discovered_at: number;
  components: ComponentNode[];
}
