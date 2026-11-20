// Hardware explorer tests. The inventory and component index come from the
// Rust side: `test-fixtures/explorer-amd-laptop.json` is produced by
// `tests/hardware/explorer.rs` from a synthetic fixture machine, so these
// tests exercise the real Rust models as the frontend receives them.

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { HardwarePage, type ExplorerCategory, type ExplorerSelection } from "../pages/HardwarePage";
import type { ComponentIndex, InventoryView, JobProgress, SystemInventory } from "../types/inventory";
import type { MonitoringReport, Reading, TelemetrySnapshot } from "../types/telemetry";
import {
  RELIABILITY,
  ancestors,
  indexMaps,
  liveFor,
  liveState,
  reliabilityOf,
  resolveSelection,
  specRows,
  treeKey,
  visibleTree,
  type ExplorerTelemetry,
} from "./explorer";

const FIXTURE = join(import.meta.dirname, "../test-fixtures/explorer-amd-laptop.json");
const load = (file = FIXTURE): { inventory: SystemInventory; components: ComponentIndex } => JSON.parse(readFileSync(file, "utf8"));
const intel = () => load(join(import.meta.dirname, "../test-fixtures/explorer-intel-hybrid.json"));
const text = (html: string) => html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ");

const progress: JobProgress = { job: "hardware", state: "completed", started_at: 1, finished_at: 2, current_module: null, modules: [], last_successful_scan: 1, error: null, cache_saved: true };
const noop = () => {};

function view(data = load()): InventoryView {
  return { inventory: data.inventory, components: data.components, origin: "this_session", source: "daemon", progress, warning: null };
}

function render(category: ExplorerCategory, selection: ExplorerSelection = {}, telemetry: ExplorerTelemetry | null = null, data = load()) {
  return renderToStaticMarkup(
    <HardwarePage inventory={view(data)} progress={progress} serviceRunning onScan={noop} onCancel={noop} category={category} onCategory={noop} selection={selection} onSelection={noop} telemetry={telemetry} />,
  );
}

// ------------------------------------------------------------ telemetry

const ok = <T,>(value: T): Reading<T> => ({ value, state: "ok", quality: "direct" });
const none = <T,>(): Reading<T> => ({ state: "unsupported", quality: "direct" });
const T0 = 1_800_000_000_000;

function snapshot(): TelemetrySnapshot {
  const cores = Array.from({ length: 16 }, (_, cpu) => ({ cpu, utilisation_percent: ok(cpu * 5), frequency_khz: ok(2_000_000 + cpu * 1000) }));
  return {
    cpu: { measured_at: T0, interval_ms: 1000, logical_cpus: 16, total_percent: ok(37.5), times: {} as never, cores, average_frequency_khz: ok(2_400_000) },
    memory: {
      measured_at: T0,
      interval_ms: 2000,
      total_bytes: ok(16e9),
      available_bytes: ok(9e9),
      used_bytes: ok(7e9),
      used_percent: ok(43.75),
      free_bytes: ok(1e9),
      buffers_bytes: ok(1),
      cached_bytes: ok(1),
      active_bytes: ok(1),
      inactive_bytes: ok(1),
      swap_total_bytes: ok(4e9),
      swap_used_bytes: ok(0),
      swap_free_bytes: ok(4e9),
      swap_in_pages_per_s: ok(0),
      swap_out_pages_per_s: ok(0),
      pressure: { some_avg10: ok(0.5), some_avg60: ok(0), full_avg10: ok(0), full_avg60: ok(0) },
    },
    battery: {
      measured_at: T0,
      interval_ms: 5000,
      on_ac: ok(false),
      sources: [],
      batteries: [
        {
          measured_at: T0,
          interval_ms: 5000,
          name: "BAT0",
          state: ok("discharging"),
          percent: ok(76),
          energy_now_wh: ok(38),
          energy_full_wh: ok(50),
          energy_design_wh: ok(56),
          charge_now_ah: none(),
          charge_full_ah: none(),
          charge_design_ah: none(),
          voltage_v: ok(11.8),
          current_a: none(),
          power_w: ok(7.5),
          cycle_count: ok(120),
          health_percent: ok(89.3),
          temperature_c: none(),
          runtime: { state: "calculating", samples: 2, needed: 6 },
          technology: "Li-ion",
          manufacturer: null,
          model: null,
        },
      ],
    },
    thermal: null,
    fans: null,
    gpu: {
      measured_at: T0,
      interval_ms: 2000,
      gpus: [
        { pci_address: "0000:03:00.0", driver: "amdgpu", memory_model: "shared", busy_percent: ok(12), vram_used_bytes: ok(200e6), vram_total_bytes: ok(512e6), gtt_used_bytes: ok(50e6), gtt_total_bytes: ok(8e9), temperature_c: ok(48), power_w: none(), core_clock_mhz: ok(400), note: null },
      ],
    },
    storage: {
      measured_at: T0,
      interval_ms: 5000,
      devices: [{ name: "nvme0n1", read_bps: ok(1024), write_bps: ok(4096), reads_per_s: ok(1), writes_per_s: ok(2), busy_percent: ok(1.5), read_total_bytes: 1, written_total_bytes: 1 }],
      filesystems: [{ mount_point: "/", device: "/dev/mapper/cryptroot", fs_type: "ext4", total_bytes: ok(500e9), used_bytes: ok(100e9), available_bytes: ok(400e9), used_percent: ok(20), read_only: false }],
    },
    network: {
      measured_at: T0,
      interval_ms: 2000,
      interfaces: [{ name: "wlan0", operational_state: "up", loopback: false, rx_bps: ok(2048), tx_bps: ok(512), rx_packets_per_s: ok(3), tx_packets_per_s: ok(1), rx_total_bytes: 1, tx_total_bytes: 1, rx_errors: 0, tx_errors: 0, rx_dropped: 0, tx_dropped: 0 }],
    },
    load: null,
    health: [],
    top_processes: [],
  };
}

const report = (phase: MonitoringReport["phase"]): MonitoringReport => ({
  phase,
  since: 0,
  sampling: { selected: "balanced", effective: "balanced", power_aware: true, reason: null, intervals_ms: [["cpu", 2000]] },
  collectors: [],
  last_sample_at: T0,
  error: null,
  enabled_preference: true,
  history_available: true,
  notifications: { enabled: true, last_delivery_ok: null, last_error: null, delivered: 0, failed: 0 },
});

const live = (stale = false): ExplorerTelemetry => ({ snapshot: snapshot(), report: report("active"), stale });

// ------------------------------------------------------------------ tests

describe("component index from Rust", () => {
  it("reaches the frontend with the hierarchy discovery reported", () => {
    const { inventory, components } = load();
    const maps = indexMaps(components);
    expect(components.version).toBe(1);
    expect(components.inventory_discovered_at).toBe(inventory.discovered_at);
    expect(maps.roots.get("processor")!.map((n) => n.id)).toEqual(["cpu:0"]);
    const logical = components.components.filter((c) => c.kind === "logical_cpu");
    expect(logical).toHaveLength(16);
    expect(maps.roots.get("storage")!.map((n) => n.id).sort()).toEqual(["storage:loop1", "storage:nvme0n1", "storage:sda"]);
    expect(ancestors(maps, "storage:dm-0")).toEqual(["storage:nvme0n1", "storage:nvme0n1p2"]);
    expect(maps.byId.get("usb:1-1")!.parent).toBe("usb:usb1");
  });

  it("keeps selections by stable ID across refreshes and reports lost ones", () => {
    const first = indexMaps(load().components);
    const refreshed = indexMaps(load().components); // a new object, as after a refetch
    for (const id of ["cpu:0/core:3", "storage:dm-0", "usb:1-1", "pci:0000:03:00.0", "net:wlan0"]) {
      expect(resolveSelection(refreshed, id).node?.name).toBe(resolveSelection(first, id).node?.name);
    }
    const data = load();
    data.components.components = data.components.components.filter((c) => c.id !== "usb:1-1");
    expect(resolveSelection(indexMaps(data.components), "usb:1-1")).toEqual({ node: null, lost: true });
    expect(resolveSelection(first, null)).toEqual({ node: null, lost: false });
  });

  it("navigates the tree with the keyboard", () => {
    const maps = indexMaps(load().components);
    const roots = maps.roots.get("storage")!;
    const expanded = new Set<string>();
    const visible = visibleTree(maps, roots, expanded);
    expect(visible).toEqual(roots.map((r) => r.id));
    expect(treeKey(maps, visible, expanded, "storage:nvme0n1", "ArrowRight")).toEqual({ expand: "storage:nvme0n1" });
    const open = new Set(["storage:nvme0n1"]);
    const shown = visibleTree(maps, roots, open);
    expect(shown).toContain("storage:nvme0n1p2");
    expect(treeKey(maps, shown, open, "storage:nvme0n1", "ArrowRight")).toEqual({ focus: "storage:nvme0n1p1" });
    expect(treeKey(maps, shown, open, "storage:nvme0n1p1", "ArrowLeft")).toEqual({ focus: "storage:nvme0n1" });
    expect(treeKey(maps, shown, open, "storage:nvme0n1", "ArrowLeft")).toEqual({ collapse: "storage:nvme0n1" });
    expect(treeKey(maps, shown, open, "storage:nvme0n1p1", "Enter")).toEqual({ select: "storage:nvme0n1p1" });
    expect(treeKey(maps, shown, open, shown[0]!, "ArrowUp")).toBeNull();
  });
});

describe("live telemetry mapping", () => {
  const maps = indexMaps(load().components);
  const node = (id: string) => maps.byId.get(id)!;
  const rows = (id: string, t: ExplorerTelemetry | null = live()) => liveFor(node(id), t).rows;
  const rowText = (id: string, label: string, t?: ExplorerTelemetry | null) => rows(id, t).find((r) => r.label === label)?.text;

  it("maps each logical CPU and core to its own samples", () => {
    expect(rowText("cpu:0/cpu:6", "Utilisation")).toBe("30.0%");
    expect(rowText("cpu:0/cpu:6", "Frequency")).toBe("2006 MHz");
    // Core 3 = CPUs 6 and 7 (30 % and 35 %): the mean, marked derived.
    const core = rows("cpu:0/core:3");
    expect(core[0]).toMatchObject({ label: "Utilisation", text: "32.5% (mean of its logical CPUs)", qualifier: "derived" });
    expect(core.map((r) => r.label)).toEqual(["Utilisation", "CPU 6", "CPU 7"]);
    expect(rowText("cpu:0", "Utilisation")).toBe("37.5%");
    expect(liveFor(node("cpu:0/cpu:6"), live()).measuredAt).toBe(T0);
  });

  it("maps memory, GPU, disks, filesystems, network and battery by their keys", () => {
    expect(rowText("memory", "In use (share)")).toBe("43.8%");
    expect(rowText("gpu:0000:03:00.0", "Busy")).toBe("12.0%");
    expect(rowText("gpu:0000:03:00.0", "Reserved system RAM in use")).toBeDefined();
    expect(rowText("gpu:0000:03:00.0", "Dedicated VRAM in use")).toBeUndefined();
    expect(rowText("gpu:0000:03:00.0", "Power")).toBe("Not provided");
    expect(rowText("storage:nvme0n1", "Write")).toBe("4.0 KiB/s");
    expect(rows("storage:dm-0").map((r) => r.label)).toEqual(["Filesystem at /"]);
    expect(rows("storage:nvme0n1p1")).toEqual([]); // not in the sample: nothing invented
    expect(rowText("net:wlan0", "Receiving")).toBe("2.0 KiB/s");
    expect(rowText("net:docker0", "Traffic")).toBe("Not in the network sample");
    expect(rowText("battery:BAT0", "Power draw")).toBe("7.5 W");
    expect(rows("battery:BAT0").find((r) => r.label === "Runtime estimate")).toMatchObject({ present: false });
    expect(rows("usb:1-1")).toEqual([]);
  });

  it("is live only while monitoring is active and the data is fresh", () => {
    expect(liveState(null)).toBe("off");
    expect(liveState({ snapshot: snapshot(), report: report("off"), stale: false })).toBe("off");
    expect(liveState({ snapshot: snapshot(), report: report("paused"), stale: false })).toBe("stale");
    expect(liveState(live(true))).toBe("stale");
    expect(liveState(live())).toBe("live");
    expect(rows("cpu:0/cpu:6", null)).toEqual([]);
    expect(rows("cpu:0/cpu:6", { snapshot: snapshot(), report: report("off"), stale: false })).toEqual([]);
  });
});

describe("reliability", () => {
  const { inventory, components } = load();
  const maps = indexMaps(components);
  const spec = (id: string, label: string) => specRows(maps.byId.get(id)!, inventory, maps).find((r) => r.label === label)!;

  it("uses one vocabulary for detected and missing values", () => {
    expect(reliabilityOf("detected", "kernel_reported").label).toBe("Kernel");
    expect(reliabilityOf("detected", "derived").label).toBe("Derived");
    expect(reliabilityOf("permission_denied", null).label).toBe("Admin only");
    expect(reliabilityOf("unsupported", "kernel_reported").label).toBe("Unsupported");
    expect(new Set(Object.values(RELIABILITY).map((r) => r.label)).size).toBe(Object.keys(RELIABILITY).length);
  });

  it("marks derived, inferred and restricted values as such", () => {
    expect(spec("battery:BAT0", "Health estimate")).toMatchObject({ confidence: "derived", present: true });
    expect(spec("cpu:0", "Microarchitecture").confidence).toBe("inferred");
    expect(spec("memory", "Installed RAM")).toMatchObject({ availability: "unsupported", present: false });
    expect(spec("cpu:0/cache:l3-unified-0", "Total across instances").confidence).toBe("derived");
    expect(spec("storage:nvme0n1p1", "Filesystem")).toMatchObject({ text: "vfat", confidence: "kernel_reported", source: "/proc/mounts" });
  });

  it("never states what Linux does not report", () => {
    expect(spec("cpu:0/core:0", "Position on the die")).toMatchObject({ present: false, availability: "unsupported" });
    expect(spec("cpu:0", "Package ID").note).toContain("physical socket is not detectable");
    expect(spec("pci:0000:03:00.0", "Position on the board").present).toBe(false);
    expect(spec("usb:1-1", "Physical socket").present).toBe(false);
    expect(spec("storage:nvme0n1", "Health (SMART)").present).toBe(false);
    expect(spec("net:wlan0", "IP configuration").present).toBe(false);
  });
});

describe("explorer page", () => {
  it("opens on an overview of every category, with the reliability legend", () => {
    const html = text(render("overview"));
    for (const s of ["Hardware overview", "Example AMD 8-Core Processor", "16 logical", "Example NVMe 512GB", "BAT0", "Monitoring off · scan values only", "Reliability labels", "Derived"]) {
      expect(html).toContain(s);
    }
    expect(html).not.toMatch(/\d+(\.\d+)?% busy/);
  });

  it("shows CPU topology, a shared cache hierarchy and an inspector", () => {
    const html = render("processor");
    expect(html).toContain('role="tree"');
    expect(html).toContain("TOPOLOGY, NOT A DIE IMAGE");
    expect(html).toContain("L3 cache, 12 MiB, shared by 8 cores");
    expect(html).toContain("L2 cache 3, 1.3 MiB, shared by 1 core");
    expect(text(html)).toContain("Processor package");
    // Monitoring off: no live numbers anywhere.
    expect(text(html)).toContain("Monitoring off");
    expect(text(html)).not.toMatch(/\d+(\.\d+)?%/);
  });

  it("shows a selected core with its threads, caches and live utilisation", () => {
    const html = text(render("processor", { processor: "cpu:0/core:3" }, live()));
    expect(html).toContain("Core 3");
    expect(html).toContain("CPU 6, CPU 7");
    expect(html).toContain("32.5% (mean of its logical CPUs)");
    expect(html).toContain("Not known. Linux reports logical topology only.");
    expect(html).toContain("L3 cache");
    expect(html).toContain("Live utilisation");
  });

  it("labels stale data and keeps showing the selection", () => {
    const html = text(render("processor", { processor: "cpu:0/cpu:6" }, live(true)));
    expect(html).toContain("Stale · last values received");
    expect(html).toContain("30.0%");
  });

  it("says when a selected component is gone instead of moving the selection", () => {
    const html = text(render("processor", { processor: "cpu:0/core:99" }));
    expect(html).toContain("Component no longer detected");
  });

  it("explains memory without inventing modules or channels", () => {
    const html = text(render("memory"));
    expect(html).toContain("Memory capacity");
    expect(html).toContain("This system does not report its memory modules.");
    expect(html).toContain("Not inferred.");
    expect(html).not.toContain("DIMM");
  });

  it("distinguishes shared graphics memory from VRAM", () => {
    const html = text(render("graphics", {}, live()));
    expect(html).toContain("reserved from system RAM");
    expect(html).toContain("Shared memory, not separate VRAM");
    expect(html).not.toContain("dedicated VRAM");
    expect(html).toContain("Not shown. Linux does not report them");
  });

  it("draws storage as disk, partitions, volumes and mounts", () => {
    const html = text(render("storage", { storage: "storage:dm-0" }, live()));
    for (const s of ["dm-0 (cryptroot)", "nvme0n1p2", "/boot/efi", "Mounted at", "ext4", "Filesystem at /", "Built on", "Example Flash"]) {
      expect(html).toContain(s);
    }
    const part = text(render("storage", { storage: "storage:nvme0n1p2" }));
    expect(part).toContain("Used by dm-0");
  });

  it("separates firmware capacity from estimates for the battery", () => {
    const html = text(render("battery", {}, live()));
    expect(html).toContain("Design capacity (firmware)");
    expect(html).toContain("of design (derived)");
    expect(html).toContain("not a measurement of the cells");
    expect(html).toContain("Charge now (live)");
    expect(html).toContain("38.0 Wh");
  });

  it("uses charge capacities for a battery that reports charge, without converting", () => {
    const data = load();
    const b = data.inventory.batteries[0]!;
    const gone = { availability: "unsupported", confidence: "unknown" } as const;
    const det = (v: number) => ({ value: v, availability: "detected", confidence: "kernel_reported", source: "/sys/class/power_supply/BAT0" }) as const;
    Object.assign(b, { energy_full_design_uwh: gone, energy_full_uwh: gone, energy_now_uwh: gone, charge_full_design_uah: det(3_450_000), charge_full_uah: det(3_100_000), charge_now_uah: det(2_000_000) });
    const html = text(render("battery", {}, null, data));
    expect(html).toContain("3.45 Ah");
    expect(html).toContain("3.10 Ah · 89.9% of design (derived)");
    expect(html).toContain("Charge at scan");
    expect(html).not.toMatch(/\d Wh/);
  });

  it("shows network adapters with masked MAC addresses", () => {
    const html = render("network", { network: "net:wlan0" }, live());
    expect(text(html)).toContain("••:••:••:••:44:55");
    expect(html).not.toContain("02:11:22:33:44:55");
    expect(text(html)).toContain("Not collected. Sentinel does not read addresses or routes.");
  });

  it("shows USB and PCI hierarchies with unknown parents marked", () => {
    const usb = text(render("usb", { usb: "usb:1-1" }));
    expect(usb).toContain("usb1 · xHCI Host Controller");
    expect(usb).toContain("Linux port 1");
    expect(usb).toContain("Linux port numbers are not physical sockets.");
    const pci = render("pci", { pci: "pci:0000:03:00.0" });
    expect(text(pci)).toContain("Parent not discovered");
    expect(text(pci)).toContain("Example Integrated GPU");
    expect(text(pci)).toContain("Bus topology, not board wiring.");
  });

  it("shows firmware memory devices as the firmware names them (Intel fixture)", () => {
    const html = text(render("memory", { memory: "memory/device:DIMM 1" }, null, intel()));
    expect(html).toContain("DIMM 0");
    expect(html).toContain("DIMM 1");
    expect(html).toContain("Memory device (firmware record)");
    expect(html).toContain("SODIMM");
    expect(html).toContain("Not known. A firmware record is not a slot location.");
    expect(html).toContain("Firmware");
  });

  it("says when graphics memory configuration is unknown (Intel fixture)", () => {
    const html = text(render("graphics", { graphics: "gpu:0000:01:00.0" }, null, intel()));
    expect(html).toContain("Example Discrete GPU N");
    expect(html).toContain("Configuration unknown: not reported by the driver");
    expect(html).not.toMatch(/\d+(\.\d+)? (GiB|MiB) dedicated VRAM/);
  });

  it("keeps hybrid core types and thread counts (Intel fixture)", () => {
    const html = text(render("processor", { processor: "cpu:0/core:11" }, null, intel()));
    expect(html).toContain("Core 0 (performance)");
    expect(html).toContain("Core 11 (efficiency)");
    expect(html).toContain("2 threads");
    expect(html).toContain("1 thread");
  });

  it("renders every category without an index too (cached Phase 2 view)", () => {
    const data = load();
    const old = renderToStaticMarkup(<HardwarePage inventory={{ ...view(data), components: null }} progress={progress} serviceRunning onScan={noop} onCancel={noop} initial="storage" />);
    expect(old).not.toContain('role="tree"');
    expect(text(old)).toContain("Example NVMe 512GB");
  });
});

describe("explorer sources", () => {
  const dir = join(import.meta.dirname, "..");
  const files = ["app/explorer.ts", "components/ExplorerVisuals.tsx", "components/Inspector.tsx", "components/ComponentTree.tsx", "components/Reliability.tsx", "pages/HardwarePage.tsx"];

  it("start no polling, timers or backend calls of their own", () => {
    for (const f of files) {
      const src = readFileSync(join(dir, f), "utf8");
      expect(src, f).not.toMatch(/setInterval|setTimeout|requestAnimationFrame|services\/bridge|backend\.|invoke\(/);
    }
  });

  it("are all covered by this list", () => {
    const explorerFiles = readdirSync(join(dir, "components")).filter((f) => /Explorer|Inspector|ComponentTree|Reliability/.test(f));
    expect(explorerFiles.sort()).toEqual(["ComponentTree.tsx", "ExplorerVisuals.tsx", "Inspector.tsx", "Reliability.tsx"]);
  });
});
