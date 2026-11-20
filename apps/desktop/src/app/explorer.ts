// Pure logic of the hardware explorer. Nothing here reads the system or
// polls: the component index comes from Rust (`ComponentIndex`), live values
// from the app-wide telemetry snapshot. Honesty rules, unit-tested:
// - a missing value says why it is missing, never 0 or "";
// - derived and inferred values say so;
// - relationships carry how they are known; nothing is placed physically;
// - live values are shown only from an active, fresh snapshot.

import type {
  Availability,
  ComponentCategory,
  ComponentIndex,
  ComponentKind,
  ComponentNode,
  Confidence,
  Field,
  ParentBasis,
  Relation,
  SystemInventory,
  TelemetryRef,
} from "../types/inventory";
import type { MonitoringReport, Reading, TelemetrySnapshot } from "../types/telemetry";
import { AVAILABILITY_TEXT, formatBytes, formatCapacity, formatGhz, formatScanTime, hex4, maskMac, show, value } from "./inventoryView";
import { celsius, clock, mhz, pct, rate, reading, runtimeText, val, watts } from "./telemetryView";

// ---------------------------------------------------------------- vocabulary

export interface Reliability {
  label: string;
  /** What the label means, for the legend and tooltips. */
  meaning: string;
  tone: "solid" | "derived" | "inferred" | "missing";
}

export const RELIABILITY: Record<Confidence, Reliability> = {
  direct: { label: "Direct", meaning: "Read directly from the device through a Linux hardware interface.", tone: "solid" },
  firmware_reported: { label: "Firmware", meaning: "Reported by firmware tables (SMBIOS/DMI). Vendors sometimes fill these in carelessly.", tone: "solid" },
  kernel_reported: { label: "Kernel", meaning: "Reported by the Linux kernel or a driver (operating-system reported).", tone: "solid" },
  database: { label: "ID database", meaning: "Name looked up by numeric ID in the hardware database installed on this computer.", tone: "solid" },
  derived: { label: "Derived", meaning: "Calculated from other detected values by a fixed formula. Not itself a reading.", tone: "derived" },
  inferred: { label: "Inferred", meaning: "Concluded from indirect evidence. Can be wrong even when the inputs are right.", tone: "inferred" },
  unknown: { label: "Unknown", meaning: "The source or its reliability could not be established.", tone: "inferred" },
};

export const MISSING_RELIABILITY: Record<Exclude<Availability, "detected">, Reliability> = {
  unknown: { label: "Unknown", meaning: "No source was found and it is not known why.", tone: "missing" },
  unsupported: { label: "Unsupported", meaning: "This computer, kernel or driver does not provide it.", tone: "missing" },
  unavailable: { label: "Unavailable", meaning: "The interface exists but gave no value.", tone: "missing" },
  permission_denied: { label: "Admin only", meaning: "Readable only by administrators. Sentinel never asks for elevated privileges.", tone: "missing" },
  invalid: { label: "Invalid", meaning: "A placeholder or malformed value, not shown as information.", tone: "missing" },
};

export function reliabilityOf(availability: Availability, confidence: Confidence | null): Reliability {
  if (availability !== "detected") return MISSING_RELIABILITY[availability];
  return RELIABILITY[confidence ?? "unknown"];
}

export const PARENT_BASIS_TEXT: Record<ParentBasis, string> = {
  root: "Top level",
  kernel_topology: "Kernel topology",
  bus_topology: "Bus topology reported by the kernel",
  firmware: "Firmware records",
  parent_not_found: "Parent not discovered",
};

export const RELATION_TEXT: Record<Relation, string> = {
  pci_function: "PCI function",
  controller: "Attached through",
  usb_device: "USB device",
  cache: "Uses cache",
  shared_by: "Shared by",
  built_on: "Built on",
  used_by: "Used by",
  local_cpu: "Local CPU",
};

export const KIND_TEXT: Record<ComponentKind, string> = {
  cpu_package: "Processor package",
  cpu_core: "Physical core",
  logical_cpu: "Logical processor",
  cache: "Cache",
  memory: "System memory",
  memory_device: "Memory device (firmware record)",
  numa_node: "NUMA node",
  gpu: "Graphics device",
  disk: "Physical storage device",
  partition: "Partition",
  virtual_block: "Virtual block device",
  battery: "Battery",
  power_supply: "Power supply",
  network_interface: "Network interface",
  usb_root_hub: "USB root hub (host controller)",
  usb_hub: "USB hub",
  usb_device: "USB device",
  pci_device: "PCI device",
  pci_bridge: "PCI bridge",
  board: "Motherboard",
};

// ------------------------------------------------------------------ index

export interface IndexMaps {
  byId: Map<string, ComponentNode>;
  roots: Map<ComponentCategory, ComponentNode[]>;
}

export function indexMaps(index: ComponentIndex | null | undefined): IndexMaps {
  const byId = new Map<string, ComponentNode>();
  const roots = new Map<ComponentCategory, ComponentNode[]>();
  for (const node of index?.components ?? []) {
    byId.set(node.id, node);
    if (node.parent === null) roots.set(node.category, [...(roots.get(node.category) ?? []), node]);
  }
  return { byId, roots };
}

/** Ancestors of `id`, nearest last, for expanding a tree to a selection. */
export function ancestors(maps: IndexMaps, id: string): string[] {
  const out: string[] = [];
  let node = maps.byId.get(id);
  const guard = new Set<string>();
  while (node?.parent && !guard.has(node.parent)) {
    guard.add(node.parent);
    out.unshift(node.parent);
    node = maps.byId.get(node.parent);
  }
  return out;
}

export interface Selection {
  node: ComponentNode | null;
  /** A component was selected but is not in the current inventory. */
  lost: boolean;
}

/**
 * Selections are component IDs, which are built from kernel names and bus
 * addresses, so they survive refreshes and rescans of the same hardware. A
 * selection whose component disappeared is reported, not silently moved.
 */
export function resolveSelection(maps: IndexMaps, id: string | null | undefined): Selection {
  if (!id) return { node: null, lost: false };
  const node = maps.byId.get(id) ?? null;
  return { node, lost: node === null };
}

/** IDs shown in a tree, in display order, given which nodes are expanded. */
export function visibleTree(maps: IndexMaps, roots: ComponentNode[], expanded: ReadonlySet<string>): string[] {
  const out: string[] = [];
  const walk = (node: ComponentNode) => {
    out.push(node.id);
    if (expanded.has(node.id)) for (const c of node.children) {
      const child = maps.byId.get(c);
      if (child) walk(child);
    }
  };
  roots.forEach(walk);
  return out;
}

export type TreeAction = { focus: string } | { expand: string } | { collapse: string } | { select: string } | null;

/** Keyboard behaviour of the component tree (WAI-ARIA tree pattern). */
export function treeKey(maps: IndexMaps, visible: string[], expanded: ReadonlySet<string>, current: string, key: string): TreeAction {
  const i = visible.indexOf(current);
  const node = maps.byId.get(current);
  if (i < 0 || !node) return visible[0] ? { focus: visible[0] } : null;
  switch (key) {
    case "ArrowDown":
      return i + 1 < visible.length ? { focus: visible[i + 1]! } : null;
    case "ArrowUp":
      return i > 0 ? { focus: visible[i - 1]! } : null;
    case "Home":
      return { focus: visible[0]! };
    case "End":
      return { focus: visible[visible.length - 1]! };
    case "ArrowRight":
      if (node.children.length === 0) return null;
      return expanded.has(node.id) ? { focus: node.children[0]! } : { expand: node.id };
    case "ArrowLeft":
      if (node.children.length > 0 && expanded.has(node.id)) return { collapse: node.id };
      return node.parent ? { focus: node.parent } : null;
    case "Enter":
    case " ":
      return { select: node.id };
    default:
      return null;
  }
}

// ---------------------------------------------------------------- live data

export interface ExplorerTelemetry {
  snapshot: TelemetrySnapshot | null;
  report: MonitoringReport | null;
  stale: boolean;
}

export type LiveState = "live" | "stale" | "off";

export function liveState(t: ExplorerTelemetry | null | undefined): LiveState {
  if (!t?.snapshot || !t.report) return "off";
  if (t.report.phase === "active") return t.stale ? "stale" : "live";
  if (t.report.phase === "paused") return "stale";
  return "off";
}

export interface LiveRow {
  label: string;
  text: string;
  present: boolean;
  /** Estimated, or derived by Sentinel from several readings. */
  qualifier: "estimated" | "derived" | null;
}

export interface LiveView {
  state: LiveState;
  /** When the readings were taken (ms), when any. */
  measuredAt: number | null;
  rows: LiveRow[];
}

function row<T>(label: string, r: Reading<T> | null | undefined, render?: (v: T) => string): LiveRow {
  const shown = reading(r, render);
  return { label, text: shown.text, present: shown.present, qualifier: shown.estimated ? "estimated" : null };
}

const absent = (label: string, why: string): LiveRow => ({ label, text: why, present: false, qualifier: null });

/** Per-logical-CPU utilisation from the snapshot, for the topology diagram. */
export function cpuUsage(snapshot: TelemetrySnapshot | null | undefined): Map<number, number | null> {
  return new Map((snapshot?.cpu?.cores ?? []).map((c) => [c.cpu, val(c.utilisation_percent)]));
}

/**
 * Live readings for a component, resolved through its telemetry
 * references. Nothing is collected here; a component whose readings are
 * not in the snapshot says so.
 */
export function liveFor(node: ComponentNode, t: ExplorerTelemetry | null | undefined): LiveView {
  const state = liveState(t);
  const snap = t?.snapshot ?? null;
  if (node.telemetry.length === 0) return { state, measuredAt: null, rows: [] };
  if (state === "off" || !snap) return { state, measuredAt: null, rows: [] };
  const rows: LiveRow[] = [];
  const times: number[] = [];
  const cpuRefs = node.telemetry.filter((r): r is Extract<TelemetryRef, { source: "logical_cpu" }> => r.source === "logical_cpu");
  if (cpuRefs.length > 0) {
    const cpu = snap.cpu;
    if (cpu) times.push(cpu.measured_at);
    const samples = cpuRefs.map((r) => ({ ref: r, s: cpu?.cores.find((c) => c.cpu === r.cpu) }));
    if (cpuRefs.length > 1) {
      const values = samples.map(({ s }) => val(s?.utilisation_percent)).filter((v): v is number => v !== null);
      rows.push(
        values.length === cpuRefs.length
          ? { label: "Utilisation", text: `${pct(values.reduce((a, b) => a + b, 0) / values.length)} (mean of its logical CPUs)`, present: true, qualifier: "derived" }
          : absent("Utilisation", "Not available for every logical CPU"),
      );
    }
    for (const { ref, s } of samples) {
      const label = cpuRefs.length > 1 ? `CPU ${ref.cpu}` : "Utilisation";
      rows.push(s ? row(label, s.utilisation_percent, pct) : absent(label, "No reading for this CPU"));
      if (cpuRefs.length === 1) rows.push(s ? row("Frequency", s.frequency_khz, mhz) : absent("Frequency", "No reading"));
    }
  }
  for (const ref of node.telemetry) {
    switch (ref.source) {
      case "cpu_total": {
        const cpu = snap.cpu;
        if (!cpu) {
          rows.push(absent("Utilisation", "No CPU sample yet"));
          break;
        }
        times.push(cpu.measured_at);
        rows.push(row("Utilisation", cpu.total_percent, pct), row("Average frequency", cpu.average_frequency_khz, mhz));
        break;
      }
      case "memory": {
        const m = snap.memory;
        if (!m) {
          rows.push(absent("Memory", "No memory sample yet"));
          break;
        }
        times.push(m.measured_at);
        rows.push(
          row("In use", m.used_bytes, formatBytes),
          row("In use (share)", m.used_percent, pct),
          row("Available", m.available_bytes, formatBytes),
          row("Usable total", m.total_bytes, formatBytes),
          row("Swap in use", m.swap_used_bytes, formatBytes),
          row("Swap total", m.swap_total_bytes, formatBytes),
          row("Memory pressure (10 s)", m.pressure.some_avg10, pct),
        );
        break;
      }
      case "gpu": {
        const g = snap.gpu?.gpus.find((x) => x.pci_address === ref.pci_address);
        if (snap.gpu) times.push(snap.gpu.measured_at);
        if (!g) {
          rows.push(absent("Graphics", "This device is not in the GPU sample"));
          break;
        }
        rows.push(row("Busy", g.busy_percent, pct), row("Temperature", g.temperature_c, celsius), row("Core clock", g.core_clock_mhz, (v) => `${Math.round(v)} MHz`), row("Power", g.power_w, watts));
        if (g.memory_model === "dedicated") {
          rows.push(row("Dedicated VRAM in use", g.vram_used_bytes, formatBytes), row("Dedicated VRAM total", g.vram_total_bytes, formatBytes));
        } else if (g.memory_model === "shared") {
          rows.push(row("Reserved system RAM in use", g.vram_used_bytes, formatBytes), row("Shared system RAM mapped (GTT)", g.gtt_used_bytes, formatBytes));
        } else {
          rows.push(absent("Graphics memory", "Memory configuration not known"));
        }
        if (g.note) rows.push(absent("Note", g.note));
        break;
      }
      case "disk": {
        const d = snap.storage?.devices.find((x) => x.name === ref.name);
        if (snap.storage) times.push(snap.storage.measured_at);
        if (!d) {
          rows.push(absent("Disk activity", "Not in the storage sample"));
          break;
        }
        rows.push(row("Read", d.read_bps, rate), row("Write", d.write_bps, rate), row("Busy", d.busy_percent, pct));
        break;
      }
      case "filesystem": {
        for (const f of snap.storage?.filesystems.filter((x) => x.device === ref.device) ?? []) {
          rows.push({
            label: `Filesystem at ${f.mount_point}`,
            text: `${reading(f.used_bytes, formatBytes).text} used of ${reading(f.total_bytes, formatBytes).text} (${reading(f.used_percent, pct).text}) · ${f.fs_type}${f.read_only ? " · read-only" : ""}`,
            present: val(f.used_bytes) !== null,
            qualifier: null,
          });
        }
        break;
      }
      case "network_interface": {
        const n = snap.network?.interfaces.find((x) => x.name === ref.name);
        if (snap.network) times.push(snap.network.measured_at);
        if (!n) {
          rows.push(absent("Traffic", "Not in the network sample"));
          break;
        }
        rows.push(row("Receiving", n.rx_bps, rate), row("Sending", n.tx_bps, rate), {
          label: "Link state",
          text: n.operational_state ?? "Not reported",
          present: n.operational_state !== null,
          qualifier: null,
        });
        break;
      }
      case "battery": {
        const b = snap.battery?.batteries.find((x) => x.name === ref.name);
        if (snap.battery) times.push(snap.battery.measured_at);
        if (!b) {
          rows.push(absent("Battery", "Not in the battery sample"));
          break;
        }
        const runtime = runtimeText(b.runtime);
        rows.push(
          row("State", b.state, (s) => s.replace(/_/g, " ")),
          row("Charge", b.percent, (v) => `${Math.round(v)}%`),
          row("Energy now", b.energy_now_wh, (v) => `${v.toFixed(2)} Wh`),
          row("Power draw", b.power_w, watts),
          row("Voltage", b.voltage_v, (v) => `${v.toFixed(2)} V`),
          { label: "Runtime estimate", text: `${runtime.text}${runtime.detail ? ` · ${runtime.detail}` : ""}`, present: b.runtime.state === "estimated", qualifier: b.runtime.state === "estimated" ? "estimated" : null },
        );
        break;
      }
      default:
        break;
    }
  }
  return { state, measuredAt: times.length ? Math.max(...times) : null, rows };
}

// -------------------------------------------------------------- spec rows

export interface SpecRow {
  label: string;
  text: string;
  present: boolean;
  availability: Availability;
  confidence: Confidence | null;
  source: string | null;
  note: string | null;
}

function fromField<T>(label: string, field: Field<T> | null | undefined, render?: (v: T) => string, note?: string): SpecRow {
  const shown = show(field, render);
  return {
    label,
    text: shown.text,
    present: shown.present,
    availability: field?.availability ?? "unknown",
    confidence: field?.confidence ?? null,
    source: field?.source ?? null,
    note: note ?? field?.notes ?? null,
  };
}

/** A plain value with the provenance of the record it came from. */
function fromValue(label: string, v: string | number | null | undefined, basis: Confidence, source: string | null, note: string | null = null, missing: Availability = "unavailable"): SpecRow {
  const present = v !== null && v !== undefined && v !== "";
  return {
    label,
    text: present ? String(v) : missing === "unsupported" ? "Not provided by this system" : AVAILABILITY_TEXT[missing],
    present,
    availability: present ? "detected" : missing,
    confidence: present ? basis : null,
    source,
    note,
  };
}

/** A statement Sentinel makes about what it does not know. */
function limitation(label: string, text: string): SpecRow {
  return { label, text, present: false, availability: "unsupported", confidence: null, source: null, note: null };
}

/** Instruction-set extensions worth naming, as the kernel lists them. */
const NOTABLE_FLAGS = ["sse4_2", "avx", "avx2", "fma", "avx512f", "aes", "sha_ni", "vaes", "vmx", "svm", "lm", "neon", "asimd", "sve"];

const bytesOrUnknown = (b: number | null | undefined) => (b === null || b === undefined ? null : formatBytes(b));
const list = (xs: (string | number)[]) => (xs.length ? xs.join(", ") : null);

export function specRows(node: ComponentNode, inv: SystemInventory, maps: IndexMaps): SpecRow[] {
  const k = "kernel_reported" as const;
  switch (node.kind) {
    case "cpu_package": {
      const cpu = inv.processors.find((p) => String(p.package_id) === node.key);
      if (!cpu) return [];
      const flags = value(cpu.flags) ?? [];
      return [
        fromField("Model", cpu.model_name),
        fromField("Vendor", cpu.vendor),
        fromField("Microarchitecture", cpu.microarchitecture),
        fromField("Architecture", cpu.architecture),
        fromField("Family / model / stepping", cpu.family, (f) => `${f} / ${value(cpu.model) ?? "?"} / ${value(cpu.stepping) ?? "?"}`),
        fromField("Physical cores", cpu.physical_cores),
        fromField("Logical processors", cpu.logical_processors),
        fromField("Hybrid core types", cpu.hybrid, (h) => (h ? "Yes" : "No")),
        fromField("Maximum frequency", cpu.max_frequency_khz, formatGhz),
        fromField("Minimum frequency", cpu.min_frequency_khz, formatGhz),
        fromField("Notable instruction sets", cpu.flags, (f) => f.filter((x) => NOTABLE_FLAGS.includes(x)).join(" ") || "None of the listed extensions", `${flags.length} flags in total`),
        fromValue("Package ID", cpu.package_id, k, null, "The kernel's package numbering. The physical socket is not detectable from sysfs."),
        fromValue("Offline CPUs", list(cpu.offline_cpus), k, null, null, "unavailable"),
      ].filter((r) => r.label !== "Offline CPUs" || r.present);
    }
    case "cpu_core": {
      const cpu = inv.processors.find((p) => node.parent === `cpu:${p.package_id}`);
      const core = cpu?.cores.find((c) => String(c.index) === node.key);
      if (!core) return [];
      const caches = node.links.filter((l) => l.relation === "cache").map((l) => maps.byId.get(l.id)?.name ?? l.id);
      return [
        fromValue("Kernel core ID", core.core_id, k, "/sys/devices/system/cpu/cpu*/topology/core_id"),
        fromValue("Logical processors", core.logical_cpus.map((c) => `CPU ${c}`).join(", "), k, "/sys/devices/system/cpu/cpu*/topology/core_cpus_list"),
        fromValue("Core type", core.kind === "uniform" ? "Same as all others" : core.kind === "unknown" ? null : core.kind, k, null, null, "unknown"),
        fromValue("Die", core.die_id, k, "/sys/devices/system/cpu/cpu*/topology/die_id", null, "unsupported"),
        fromValue("Cluster", core.cluster_id, k, "/sys/devices/system/cpu/cpu*/topology/cluster_id", null, "unsupported"),
        fromValue("Max frequency", core.max_frequency_khz ? formatGhz(core.max_frequency_khz) : null, k, "/sys/devices/system/cpu/cpu*/cpufreq/cpuinfo_max_freq", null, "unsupported"),
        fromValue("Caches used", list(caches), k, "/sys/devices/system/cpu/cpu*/cache"),
        limitation("Position on the die", "Not known. Linux reports logical topology only."),
      ];
    }
    case "logical_cpu": {
      const core = node.parent ? maps.byId.get(node.parent) : undefined;
      const numa = inv.memory?.numa_nodes.find((n) => n.cpus.includes(Number(node.key)));
      return [
        fromValue("Kernel CPU number", node.key, k, "/sys/devices/system/cpu"),
        fromValue("Physical core", core?.name ?? null, k, "/sys/devices/system/cpu/cpu*/topology"),
        fromValue("Hardware threads on this core", core ? core.children.length : null, k, null),
        fromValue("NUMA node", numa ? numa.id : null, k, "/sys/devices/system/node", null, "unsupported"),
      ];
    }
    case "cache": {
      const cpu = inv.processors.find((p) => node.parent === `cpu:${p.package_id}`);
      const [level, kind, instance] = node.key.split(":");
      const info = cpu?.caches.find((c) => String(c.level) === level && c.kind === kind);
      if (!info) return [];
      const cpus = (info.instance_cpus[Number(instance)] ?? info.shared_by).filter((c) => cpu!.cores.some((core) => core.logical_cpus.includes(c)));
      const sharers = node.links.filter((l) => l.relation === "shared_by").map((l) => maps.byId.get(l.id)?.name ?? l.id);
      const src = "/sys/devices/system/cpu/cpu*/cache/index*";
      return [
        fromValue("Level", `L${info.level}`, k, src),
        fromValue("Type", info.kind === "unified" ? "Unified (data and instructions)" : info.kind === "data" ? "Data" : info.kind === "instruction" ? "Instruction" : "Other", k, src),
        fromValue("Size of this instance", bytesOrUnknown(info.size_bytes), k, src),
        fromValue("Instances in the package", info.instances, k, src),
        fromValue("Total across instances", info.size_bytes ? formatBytes(info.size_bytes * info.instances) : null, "derived", null, "size × number of instances"),
        fromValue("Logical CPUs sharing it", list(cpus.map((c) => `CPU ${c}`)), k, `${src}/shared_cpu_list`),
        fromValue("Cores sharing it", list(sharers), k, `${src}/shared_cpu_list`),
      ];
    }
    case "memory": {
      const m = inv.memory;
      if (!m) return [];
      return [
        fromField("Installed RAM", m.installed_bytes, formatBytes),
        fromField("Kernel-managed", m.kernel_physical_bytes, formatBytes),
        fromField("Usable by the system", m.usable_bytes, formatBytes),
        fromField("Swap", m.swap_total_bytes, formatBytes),
        fromField("Memory type", m.memory_type),
        fromField("Speed", m.speed_mts, (s) => `${s} MT/s`),
        fromField("Slots (populated / total)", m.slots_populated, (p) => `${p} / ${value(m.slots_total) ?? "?"}`),
        fromValue("NUMA nodes", m.numa_nodes.length || null, k, "/sys/devices/system/node", null, "unsupported"),
        limitation("Channel layout", "Not inferred. Firmware bank labels are shown per memory device when readable."),
      ];
    }
    case "memory_device": {
      const field = inv.memory?.modules;
      const mod = value(field)?.[Number(node.key)];
      if (!field || !mod) return [];
      const fw = "firmware_reported" as const;
      const src = field.source ?? null;
      return [
        fromValue("Locator", mod.locator, fw, src),
        fromValue("Bank label", mod.bank_locator, fw, src, "Firmware text. Sentinel does not derive channels from it."),
        fromValue("Populated", mod.populated ? "Yes" : "No — the firmware lists this slot as empty", fw, src),
        fromValue("Capacity", bytesOrUnknown(mod.size_bytes), fw, src),
        fromValue("Type", mod.memory_type, fw, src),
        fromValue("Form factor", mod.form_factor, fw, src, "As the firmware reports it. Soldered memory is often reported as “Row of chips”."),
        fromValue("Rated speed", mod.speed_mts ? `${mod.speed_mts} MT/s` : null, fw, src),
        fromValue("Configured speed", mod.configured_speed_mts ? `${mod.configured_speed_mts} MT/s` : null, fw, src),
        fromValue("Manufacturer", mod.manufacturer, fw, src),
        fromValue("Part number", mod.part_number, fw, src),
        limitation("Physical position", "Not known. A firmware record is not a slot location."),
      ];
    }
    case "numa_node": {
      const n = inv.memory?.numa_nodes.find((x) => String(x.id) === node.key);
      if (!n) return [];
      return [fromValue("CPUs", list(n.cpus), k, "/sys/devices/system/node"), fromValue("Memory", bytesOrUnknown(n.memory_bytes), k, "/sys/devices/system/node")];
    }
    case "gpu": {
      const g = inv.graphics.find((x) => x.pci_address === node.key);
      if (!g) return [];
      return [
        fromField("Model", g.model_name),
        fromField("Vendor", g.vendor_name),
        fromField("Type", g.kind, (x) => x),
        fromField("Driver", g.driver),
        g.memory.availability === "detected"
          ? fromField("Graphics memory", g.memory, (m) => (m.kind === "dedicated_vram" ? `${formatBytes(m.bytes)} dedicated VRAM` : `${formatBytes(m.bytes)} reserved from system RAM (not separate memory)`))
          : fromField("Graphics memory", g.memory, undefined, "Memory configuration unknown: the driver does not report it."),
        fromField("Boot display", g.boot_display, (b) => (b ? "Yes" : "No")),
        fromField("Subsystem", g.subsystem_name),
        fromValue("PCI address", g.pci_address, k, "/sys/bus/pci/devices"),
        fromValue("PCI IDs", `${hex4(g.vendor_id)}:${hex4(g.device_id)}`, "direct", "/sys/bus/pci/devices/*/vendor, device"),
        fromValue("DRM nodes", list(g.drm_nodes), k, "/sys/bus/pci/devices/*/drm"),
        limitation("Shader or compute units", "Not shown. Linux does not report them in a vendor-neutral way."),
      ];
    }
    case "disk":
    case "virtual_block": {
      const d = inv.storage.find((x) => x.name === node.key);
      if (!d) return [];
      const rows = [
        fromValue("Kernel name", d.name, k, d.sysfs_path),
        fromField("Model", d.model),
        fromField("Vendor", d.vendor),
        fromField("Interface", d.interface, (i) => i.toUpperCase()),
        fromField("Capacity", d.capacity_bytes, (c) => `${formatCapacity(c)} (${formatBytes(c)})`),
        fromField("Sector size (logical / physical)", d.logical_sector_bytes, (l) => `${l} B / ${value(d.physical_sector_bytes) ?? "?"} B`),
        fromField("Rotational", d.rotational, (r) => (r ? "Yes (spinning disk)" : "No")),
        fromField("Removable", d.removable, (r) => (r ? "Yes" : "No")),
        fromField("Read-only", d.read_only, (r) => (r ? "Yes" : "No")),
        fromField("Firmware", d.firmware_revision),
        ...usageRows(d.usage, d.partitions.length > 0),
      ];
      if (d.kind === "virtual") {
        rows.push(fromValue("Device-mapper name", d.mapper_name, k, `/sys/block/${d.name}/dm/name`, null, "unsupported"));
        rows.push(fromValue("Built on", list(d.slaves), k, `/sys/block/${d.name}/slaves`, null, "unsupported"));
      } else {
        rows.push(fromValue("Controller", d.controller_pci_address ? `PCI ${d.controller_pci_address}` : null, k, d.sysfs_path, null, "unknown"));
        rows.push(limitation("Health (SMART)", "Not read. It needs administrator access to the drive."));
      }
      return rows;
    }
    case "partition": {
      const disk = inv.storage.find((d) => d.partitions.some((p) => p.name === node.key));
      const p = disk?.partitions.find((x) => x.name === node.key);
      if (!disk || !p) return [];
      const src = `/sys/block/${disk.name}/${p.name}`;
      return [
        fromValue("Partition number", p.number, k, `${src}/partition`),
        fromValue("Size", bytesOrUnknown(p.size_bytes), k, `${src}/size`),
        fromValue("Start sector", p.start_sector, k, `${src}/start`),
        fromValue("Read-only", p.read_only === null ? null : p.read_only ? "Yes" : "No", k, `${src}/ro`),
        ...usageRows(p.usage, false),
      ];
    }
    case "battery": {
      const b = inv.batteries.find((x) => x.name === node.key);
      if (!b) return [];
      return [
        fromField("Manufacturer", b.manufacturer),
        fromField("Model", b.model_name),
        fromField("Technology", b.technology),
        fromField("Design capacity", b.energy_full_design_uwh, (v) => `${(v / 1e6).toFixed(2)} Wh`),
        fromField("Full-charge capacity", b.energy_full_uwh, (v) => `${(v / 1e6).toFixed(2)} Wh`, "As the battery's controller reports it, not a measurement of the cells."),
        fromField("Design capacity (charge)", b.charge_full_design_uah, (v) => `${(v / 1e3).toFixed(0)} mAh`),
        fromField("Full-charge capacity (charge)", b.charge_full_uah, (v) => `${(v / 1e3).toFixed(0)} mAh`),
        fromField("Health estimate", b.health_percent, (h) => `${h.toFixed(1)}% of design capacity`),
        fromField("Cycle count", b.cycle_count),
        fromField("Minimum design voltage", b.voltage_min_design_uv, (v) => `${(v / 1e6).toFixed(2)} V`),
        fromField("Charge at scan", b.capacity_percent, (p) => `${p}%`),
        fromField("Status at scan", b.status, (s) => s.replace(/_/g, " ")),
      ];
    }
    case "power_supply": {
      const p = inv.power_supplies.find((x) => x.name === node.key);
      if (!p) return [];
      return [fromField("Kind", p.kind), fromField("Supplying power at scan", p.online, (o) => (o ? "Yes" : "No"))];
    }
    case "network_interface": {
      const n = inv.network.find((x) => x.name === node.key);
      if (!n) return [];
      return [
        fromValue("Interface", n.name, k, `/sys/class/net/${n.name}`),
        fromValue("Type", n.kind, k, `/sys/class/net/${n.name}/type`),
        fromField("MAC address", n.mac_address, (m) => maskMac(m), "Masked here. Reveal it on the Hardware details below if needed."),
        fromField("Address origin", n.mac_address_origin, (o) => o),
        fromField("Link state at scan", n.operational_state),
        fromField("Link speed", n.speed_mbps, (s) => `${s} Mb/s`),
        fromField("MTU", n.mtu),
        fromField("Driver", n.driver),
        fromField("Device", n.device_bus),
        fromValue("PCI IDs", n.vendor_id !== null && n.device_id !== null ? `${hex4(n.vendor_id)}:${hex4(n.device_id)}` : null, "direct", `/sys/class/net/${n.name}/device`, null, "unsupported"),
        fromValue("Wireless PHY", n.wireless_phy, k, `/sys/class/net/${n.name}/phy80211`, null, "unsupported"),
        limitation("IP configuration", "Not collected. Sentinel does not read addresses or routes."),
      ];
    }
    case "usb_root_hub":
    case "usb_hub":
    case "usb_device": {
      const d = inv.usb_devices.find((x) => x.sysfs_name === node.key);
      if (!d) return [];
      const src = d.sysfs_path;
      const parent = d.parent ? maps.byId.get(`usb:${d.parent}`) : undefined;
      return [
        fromValue("Vendor ID : product ID", `${hex4(d.vendor_id)}:${hex4(d.product_id)}`, "direct", src),
        fromField("Manufacturer (device)", d.manufacturer),
        fromField("Product (device)", d.product),
        fromField("Vendor (ID database)", d.vendor_name),
        fromField("Product (ID database)", d.product_name),
        fromValue("Device class", d.class_name, "direct", `${src}/bDeviceClass`),
        fromValue("Interface classes", list(d.interface_classes), "direct", src, null, "unsupported"),
        fromField("USB version", d.usb_version),
        fromField("Negotiated speed", d.speed_kbps, (s) => `${(s / 1000).toLocaleString()} Mb/s`),
        fromValue("Bus / device number", `${d.bus} / ${d.device_number}`, k, src),
        d.is_root_hub
          ? fromValue("Host controller", d.controller_pci_address ? `PCI ${d.controller_pci_address}` : null, k, src, null, "unknown")
          : fromValue(
              "Parent hub",
              parent ? `${parent.name}, Linux port ${d.port_number ?? "?"}` : null,
              k,
              src,
              "Linux port numbers are not physical sockets.",
              "unknown",
            ),
        d.is_hub ? fromValue("Ports", d.ports.length, k, `${src}/maxchild`) : fromField("Built in or removable (ACPI)", d.removable),
        limitation("Physical socket", "Not known until verified board documentation exists."),
      ];
    }
    case "pci_device":
    case "pci_bridge": {
      const d = inv.pci_devices.find((x) => x.address === node.key);
      if (!d) return [];
      const src = `/sys/bus/pci/devices/${d.address}`;
      const hex2 = (v: number) => v.toString(16).padStart(2, "0");
      return [
        fromValue("PCI address", d.address, k, src, "domain:bus:device.function"),
        fromValue("Vendor ID : device ID", `${hex4(d.vendor_id)}:${hex4(d.device_id)}`, "direct", `${src}/vendor, device`),
        fromField("Vendor", d.vendor_name),
        fromField("Device", d.device_name),
        fromValue("Class", `${d.class_name} (${hex2((d.class_code >> 16) & 0xff)})`, "direct", `${src}/class`),
        fromValue("Subclass / programming interface", `${hex2((d.class_code >> 8) & 0xff)} / ${hex2(d.class_code & 0xff)}`, "direct", `${src}/class`),
        fromValue("Subsystem IDs", d.subsystem_vendor_id !== null ? `${hex4(d.subsystem_vendor_id)}:${hex4(d.subsystem_device_id ?? 0)}` : null, "direct", src, null, "unsupported"),
        fromValue("Revision", d.revision !== null ? hex2(d.revision) : null, "direct", `${src}/revision`, null, "unsupported"),
        fromField("Driver", d.driver),
        fromValue("Upstream bridge", d.parent, k, src, "Bus topology, not board wiring.", "unsupported"),
        limitation("Position on the board", "Not known. Bus topology does not reveal physical placement."),
      ];
    }
    case "board": {
      const b = inv.motherboard;
      if (!b) return [];
      return [
        fromField("Board manufacturer", b.board_manufacturer),
        fromField("Board name", b.board_name),
        fromField("Board revision", b.board_revision),
        fromField("System manufacturer", b.system_manufacturer),
        fromField("System model", b.system_model),
        fromField("BIOS version", b.bios_version),
        fromValue("Identification", b.identification.explanation, "derived", null),
      ];
    }
  }
}

function usageRows(u: { mounts: Field<{ mount_point: string; fs_type: string; read_only: boolean }[]>; swap: Field<boolean>; holders: string[] }, partitioned: boolean): SpecRow[] {
  const mounts = value(u.mounts);
  const rows: SpecRow[] = [];
  if (mounts === null) {
    rows.push(fromField("Filesystem", u.mounts));
  } else if (mounts.length === 0) {
    rows.push({
      ...fromField("Filesystem", u.mounts, () => ""),
      text: partitioned ? "On its partitions" : u.holders.length ? `Used by ${u.holders.join(", ")}` : "Not mounted at scan time",
      note: partitioned || u.holders.length ? null : "Sentinel does not read partition contents, so the filesystem of an unmounted device is not known.",
    });
  } else {
    const types = [...new Set(mounts.map((m) => m.fs_type))].join(", ");
    rows.push({ ...fromField("Filesystem", u.mounts, () => types), note: "Mount table at scan time" });
    rows.push({ ...fromField("Mounted at", u.mounts, () => mounts.map((m) => `${m.mount_point}${m.read_only ? " (read-only)" : ""}`).join(", ")), note: null });
  }
  if (value(u.swap) === true) rows.push(fromField("Swap", u.swap, () => "Active swap at scan time"));
  return rows;
}

/** "3 min ago · 19 Nov 2026, 14:02" for the inspector's last-updated line. */
export function updatedText(inv: SystemInventory, live: LiveView): string {
  const scan = `Specifications from the scan ${formatScanTime(inv.discovered_at)}`;
  return live.measuredAt !== null && live.state !== "off" ? `${scan}; live values ${live.state === "stale" ? "last received" : "sampled"} at ${clock(live.measuredAt)}` : scan;
}
