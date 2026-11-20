// Pure functions that turn inventory data into what the UI shows.
// Honesty rules live here and are unit-tested:
// - a missing value is shown as the reason it is missing, never as 0 or "";
// - cached data is always labelled as cached;
// - inferred values say so.

import type {
  Availability,
  Confidence,
  CpuCore,
  Field,
  InventoryOrigin,
  JobProgress,
  ModuleOutcome,
  ProcessorInfo,
  SystemInventory,
  UtilityCategory,
  ViewSource,
} from "../types/inventory";
import type { Accent } from "./catalog";

export const AVAILABILITY_TEXT: Record<Availability, string> = {
  detected: "Detected",
  unknown: "Unknown",
  unsupported: "Not provided by this system",
  unavailable: "Not available",
  permission_denied: "Administrator only",
  invalid: "Not reported (placeholder or invalid)",
};

export const CONFIDENCE_TEXT: Record<Confidence, string> = {
  direct: "Direct",
  firmware_reported: "Firmware",
  kernel_reported: "Kernel",
  database: "ID database",
  derived: "Derived",
  inferred: "Inferred",
  unknown: "Unknown",
};

export interface Shown {
  /** Text to display. */
  text: string;
  /** True when this is a real value, false when it explains an absence. */
  present: boolean;
}

/** Render a field: its value, or why there is none. */
export function show<T>(field: Field<T> | null | undefined, render: (value: T) => string = String): Shown {
  if (!field) return { text: "Not available", present: false };
  if (field.availability === "detected" && field.value !== undefined && field.value !== null) {
    return { text: render(field.value), present: true };
  }
  return { text: AVAILABILITY_TEXT[field.availability], present: false };
}

export function value<T>(field: Field<T> | null | undefined): T | null {
  return field && field.availability === "detected" && field.value !== undefined ? field.value : null;
}

export function formatBytes(bytes: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${size.toFixed(1)} ${units[unit]}`;
}

/** Drive capacity in decimal gigabytes, as drives are labelled. */
export function formatCapacity(bytes: number): string {
  return bytes >= 1e12 ? `${(bytes / 1e12).toFixed(2)} TB` : `${(bytes / 1e9).toFixed(1)} GB`;
}

export function formatGhz(khz: number): string {
  return `${(khz / 1e6).toFixed(2)} GHz`;
}

export function hex4(value: number): string {
  return value.toString(16).padStart(4, "0");
}

/** "3 min ago" style age, plus the local date and time. */
export function formatScanTime(unixSeconds: number, now: number = Date.now() / 1000): string {
  const age = Math.max(0, now - unixSeconds);
  const rel =
    age < 60 ? "just now" : age < 3600 ? `${Math.floor(age / 60)} min ago` : age < 86_400 ? `${Math.floor(age / 3600)} h ago` : `${Math.floor(age / 86_400)} d ago`;
  const when = new Date(unixSeconds * 1000).toLocaleString([], {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  return `${rel} · ${when}`;
}

export interface OriginLabel {
  label: string;
  detail: string;
  cached: boolean;
}

/** Where the inventory came from. Cached data is never called live. */
export function originLabel(origin: InventoryOrigin | null, source: ViewSource, scannedAt: number | null): OriginLabel {
  const when = scannedAt === null ? "" : ` (${formatScanTime(scannedAt)})`;
  if (origin === "this_session" && source === "daemon") {
    return { label: "This session", detail: `Read from this computer by the running service${when}. It is a snapshot, not live monitoring.`, cached: false };
  }
  if (source === "cache_file") {
    return { label: "Cached", detail: `From an earlier scan${when}. The Sentinel service is not running, so nothing is live.`, cached: true };
  }
  return { label: "Cached", detail: `From an earlier scan${when}. Refresh to scan again.`, cached: true };
}

export const OUTCOME_TEXT: Record<ModuleOutcome, string> = {
  completed: "Completed",
  partial: "Partial",
  unavailable: "Unavailable",
  failed: "Failed",
  cancelled: "Cancelled",
};

export function isScanning(progress: JobProgress | null | undefined): boolean {
  return progress?.state === "queued" || progress?.state === "running";
}

// ------------------------------------------------------------ summaries

export interface SummaryCard {
  id: "cpu" | "memory" | "storage" | "gpu" | "battery" | "network";
  label: string;
  accent: Accent;
  primary: Shown;
  secondary: string;
}

export function summaryCards(inv: SystemInventory | null): SummaryCard[] {
  const notScanned: Shown = { text: "Not scanned", present: false };
  if (!inv) {
    return [
      { id: "cpu", label: "Processor", accent: "cyan", primary: notScanned, secondary: "Run a hardware scan" },
      { id: "memory", label: "Memory", accent: "purple", primary: notScanned, secondary: "Run a hardware scan" },
      { id: "storage", label: "Storage", accent: "blue", primary: notScanned, secondary: "Run a hardware scan" },
      { id: "gpu", label: "Graphics", accent: "green", primary: notScanned, secondary: "Run a hardware scan" },
      { id: "battery", label: "Battery", accent: "orange", primary: notScanned, secondary: "Run a hardware scan" },
      { id: "network", label: "Network", accent: "blue", primary: notScanned, secondary: "Run a hardware scan" },
    ];
  }
  const cpu = inv.processors[0];
  const cores = cpu ? value(cpu.physical_cores) : null;
  const threads = cpu ? value(cpu.logical_processors) : null;
  const mem = inv.memory;
  const installed = mem ? value(mem.installed_bytes) : null;
  const usable = mem ? value(mem.usable_bytes) : null;
  const drives = inv.storage.filter((d) => d.kind !== "virtual");
  const gpu = inv.graphics[0];
  const battery = inv.batteries[0];
  const physicalNet = inv.network.filter((n) => n.kind !== "loopback" && n.kind !== "virtual" && n.kind !== "bridge" && n.kind !== "tunnel");
  return [
    {
      id: "cpu",
      label: "Processor",
      accent: "cyan",
      primary: cpu ? show(cpu.model_name) : { text: "Not detected", present: false },
      secondary:
        cores !== null && threads !== null ? `${cores} cores · ${threads} threads` : threads !== null ? `${threads} logical processors` : "Topology not available",
    },
    {
      id: "memory",
      label: "Memory",
      accent: "purple",
      primary:
        installed !== null
          ? { text: `${formatBytes(installed)} installed`, present: true }
          : usable !== null
            ? { text: `${formatBytes(usable)} usable`, present: true }
            : { text: "Not available", present: false },
      secondary: installed !== null ? `${usable !== null ? formatBytes(usable) : "?"} usable` : "Installed size: administrator only",
    },
    {
      id: "storage",
      label: "Storage",
      accent: "blue",
      primary: drives[0] ? show(drives[0].model) : { text: "No drive detected", present: false },
      secondary: drives.length
        ? `${drives.length} drive${drives.length > 1 ? "s" : ""} · ${drives
            .map((d) => value(d.capacity_bytes))
            .filter((c): c is number => c !== null)
            .map(formatCapacity)
            .join(" + ") || "capacity unknown"}`
        : "",
    },
    {
      id: "gpu",
      label: "Graphics",
      accent: "green",
      primary: gpu ? show(gpu.model_name) : { text: "No GPU detected", present: false },
      secondary: gpu ? `${show(gpu.kind, kindLabel).text}${inv.graphics.length > 1 ? ` · ${inv.graphics.length} GPUs` : ""}` : "",
    },
    {
      id: "battery",
      label: "Battery",
      accent: "orange",
      primary: battery ? { text: "Battery present", present: true } : { text: "No battery", present: false },
      secondary: battery ? `Health ${show(battery.health_percent, (h) => `${h.toFixed(1)}%`).text}` : "Normal for desktops",
    },
    {
      id: "network",
      label: "Network",
      accent: "blue",
      primary: physicalNet.length ? { text: `${physicalNet.length} interface${physicalNet.length > 1 ? "s" : ""}`, present: true } : { text: "None detected", present: false },
      secondary: physicalNet.map((n) => n.kind).join(", "),
    },
  ];
}

export function kindLabel(kind: string): string {
  return kind.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase());
}

/** Devices counted on the overview, mirroring SystemInventory::hardware_count. */
export function hardwareCount(inv: SystemInventory): number {
  return (
    inv.processors.length +
    inv.graphics.length +
    inv.storage.filter((d) => d.kind !== "virtual").length +
    inv.batteries.length +
    inv.network.filter((n) => n.kind !== "loopback" && n.kind !== "virtual").length +
    inv.usb_devices.filter((d) => !d.is_root_hub).length +
    inv.pci_devices.length +
    (inv.memory ? 1 : 0)
  );
}

// ------------------------------------------------------------ CPU topology

export interface CoreBox {
  core: CpuCore;
  x: number;
  y: number;
  width: number;
  height: number;
  threads: { cpu: number; x: number; y: number; width: number; height: number }[];
}

export interface TopologyLayout {
  width: number;
  height: number;
  columns: number;
  cores: CoreBox[];
}

/**
 * Lay out a package's cores in a grid, each with its logical CPUs inside.
 * Computed only from discovered cores: no core is drawn that was not found.
 */
export function topologyLayout(processor: ProcessorInfo, width = 560): TopologyLayout {
  const cores = processor.cores;
  const count = cores.length;
  const columns = count <= 4 ? Math.max(count, 1) : count <= 8 ? 4 : count <= 16 ? 8 : Math.ceil(Math.sqrt(count * 2));
  const gap = 10;
  const pad = 14;
  const boxWidth = (width - pad * 2 - gap * (columns - 1)) / columns;
  const maxThreads = Math.max(1, ...cores.map((c) => c.logical_cpus.length));
  const threadHeight = 18;
  const boxHeight = 30 + maxThreads * (threadHeight + 4);
  const rows = Math.ceil(count / columns);
  const boxes = cores.map((core, i) => {
    const x = pad + (i % columns) * (boxWidth + gap);
    const y = pad + Math.floor(i / columns) * (boxHeight + gap);
    return {
      core,
      x,
      y,
      width: boxWidth,
      height: boxHeight,
      threads: core.logical_cpus.map((cpu, t) => ({ cpu, x: x + 6, y: y + 26 + t * (threadHeight + 4), width: boxWidth - 12, height: threadHeight })),
    };
  });
  return { width, height: pad * 2 + rows * boxHeight + Math.max(0, rows - 1) * gap, columns, cores: boxes };
}

export const CORE_ACCENT: Record<CpuCore["kind"], Accent> = {
  uniform: "cyan",
  performance: "cyan",
  efficiency: "green",
  unknown: "blue",
};

// ---------------------------------------------------------------- utilities

export const CATEGORY_LABEL: Record<UtilityCategory, string> = {
  system_monitoring: "System monitoring",
  hardware_information: "Hardware information",
  disk_management: "Disk management",
  storage_health: "Storage health",
  battery_and_power: "Battery and power",
  sensors_and_temperatures: "Sensors and temperatures",
  network_management: "Network management",
  network_diagnostics: "Network diagnostics",
  process_management: "Process management",
  system_logs: "System logs",
  service_management: "Service management",
  package_management: "Package management",
  graphics_and_drivers: "Graphics and drivers",
  backup_and_recovery: "Backup and recovery",
  system_configuration: "System configuration",
};

export const SOURCE_LABEL = {
  desktop_entry: "Desktop entry",
  package_database: "Package database",
  executable: "Executable on PATH",
  catalogue: "Sentinel catalogue only",
} as const;

/** Mask all but the last two octets of a MAC address until revealed. */
export function maskMac(mac: string): string {
  const parts = mac.split(":");
  if (parts.length !== 6) return "••:••:••:••:••:••";
  return [...parts.slice(0, 4).map(() => "••"), ...parts.slice(4)].join(":");
}
