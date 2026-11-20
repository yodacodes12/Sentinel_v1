// Phase 2 frontend tests: inventory rendering, provenance, honesty.
// All data below is synthetic ("Example Corp"); none of it describes a real machine.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Header } from "../components/Header";
import { HardwarePage, type HardwareCategory } from "../pages/HardwarePage";
import { MOTHERBOARD_STATE, MotherboardPage } from "../pages/MotherboardPage";
import { OverviewPage } from "../pages/OverviewPage";
import { ServicesPage } from "../pages/ServicesPage";
import { UtilitiesPage, filterUtilities } from "../pages/UtilitiesPage";
import type {
  BlockUse,
  CpuCore,
  Field,
  InventoryView,
  JobProgress,
  ProcessorInfo,
  SystemInventory,
  SystemUtility,
  UtilitiesView,
} from "../types/inventory";
import {
  formatBytes,
  formatCapacity,
  hardwareCount,
  isScanning,
  maskMac,
  originLabel,
  show,
  summaryCards,
  topologyLayout,
} from "./inventoryView";

const det = <T,>(value: T, source = "/sys/test", confidence: Field<T>["confidence"] = "kernel_reported"): Field<T> => ({
  value,
  availability: "detected",
  confidence,
  source,
});
const denied = <T,>(source = "/sys/firmware/dmi/entries/17-0/raw"): Field<T> => ({
  availability: "permission_denied",
  confidence: "unknown",
  source,
  notes: "readable only by administrators",
});
const missing = <T,>(): Field<T> => ({ availability: "unsupported", confidence: "unknown" });
const unused = (): BlockUse => ({ mounts: det([], "/proc/mounts"), swap: det(false, "/proc/swaps"), holders: [] });

function cores(count: number, threads: number, kind: CpuCore["kind"] = "uniform"): CpuCore[] {
  return Array.from({ length: count }, (_, i) => ({
    index: i,
    core_id: i,
    die_id: 0,
    cluster_id: null,
    logical_cpus: Array.from({ length: threads }, (_, t) => i * threads + t),
    kind,
    max_frequency_khz: 3_600_000,
  }));
}

function processor(coreList: CpuCore[]): ProcessorInfo {
  return {
    discovered_at: 1_794_900_000,
    package_id: 0,
    vendor: det("Example Silicon"),
    vendor_id: det("ExampleVendor"),
    model_name: det("Example CPU 9000"),
    family: det(6),
    model: det(1),
    stepping: det(0),
    microarchitecture: { availability: "unknown", confidence: "unknown", notes: "not in Sentinel's table" },
    architecture: det("x86_64"),
    physical_cores: det(coreList.length),
    logical_processors: det(coreList.reduce((n, c) => n + c.logical_cpus.length, 0)),
    offline_cpus: [],
    cores: coreList,
    caches: [{ level: 3, kind: "unified", size_bytes: 8 * 1024 * 1024, instances: 1, shared_by: [0, 1], instance_cpus: [[0, 1]] }],
    flags: det(["fpu", "sse2"]),
    min_frequency_khz: det(400_000),
    max_frequency_khz: det(3_600_000),
    hybrid: det(false, "core types", "inferred"),
  };
}

function inventory(overrides: Partial<SystemInventory> = {}): SystemInventory {
  return {
    schema_version: 1,
    discovered_at: 1_794_900_000,
    duration_ms: 80,
    fingerprint: { version: 1, value: "0123456789abcdef", inputs: ["manufacturer"] },
    system: {
      discovered_at: 1_794_900_000,
      manufacturer: det("Example Corp", "/sys/class/dmi/id/sys_vendor", "firmware_reported"),
      product_name: det("Example Book 14", "/sys/class/dmi/id/product_name", "firmware_reported"),
      product_version: missing(),
      product_family: missing(),
      product_sku: missing(),
      chassis: det("laptop"),
      architecture: det("x86_64"),
      kernel_release: det("6.9.0-test"),
      kernel_version: det("#1 SMP"),
      os: { name: det("TestOS"), pretty_name: det("TestOS 1.0"), id: det("testos"), id_like: [], version: det("1.0"), version_id: det("1.0") },
    },
    firmware: {
      discovered_at: 1_794_900_000,
      bios_vendor: det("Example BIOS Co"),
      bios_version: det("1.0.0"),
      bios_release_date: det("01/01/2026"),
      bios_revision: missing(),
      embedded_controller_revision: missing(),
      boot_mode: det("uefi"),
    },
    motherboard: {
      discovered_at: 1_794_900_000,
      system_manufacturer: det("Example Corp"),
      system_model: det("Example Book 14"),
      system_family: missing(),
      system_sku: missing(),
      board_manufacturer: det("Example Corp"),
      board_name: det("EXB-14", "/sys/class/dmi/id/board_name", "firmware_reported"),
      board_revision: det("A01"),
      bios_vendor: det("Example BIOS Co"),
      bios_version: det("1.0.0"),
      bios_release_date: det("01/01/2026"),
      identification: { level: "board_identified", explanation: "Board manufacturer and board name were reported by firmware.", externally_verified: false },
    },
    processors: [processor(cores(4, 2))],
    memory: {
      discovered_at: 1_794_900_000,
      installed_bytes: denied(),
      kernel_physical_bytes: det(8 * 1024 ** 3),
      usable_bytes: det(7 * 1024 ** 3, "/proc/meminfo"),
      snapshot: { available_bytes: det(3 * 1024 ** 3), free_bytes: det(1024 ** 3), cached_bytes: det(1024 ** 3), used_bytes: det(4 * 1024 ** 3, "/proc/meminfo", "inferred") },
      swap_total_bytes: det(2 * 1024 ** 3),
      memory_type: denied(),
      speed_mts: denied(),
      slots_total: denied(),
      slots_populated: denied(),
      modules: denied(),
      numa_nodes: [{ id: 0, cpus: [0, 1, 2, 3, 4, 5, 6, 7], memory_bytes: 7 * 1024 ** 3 }],
    },
    graphics: [
      {
        discovered_at: 1_794_900_000,
        pci_address: "0000:00:02.0",
        vendor_id: 0x8086,
        device_id: 0x1234,
        subsystem_vendor_id: null,
        subsystem_device_id: null,
        revision: 1,
        vendor_name: det("Example Graphics Inc.", "/usr/share/hwdata/pci.ids", "database"),
        model_name: det("Example iGPU", "/usr/share/hwdata/pci.ids", "database"),
        subsystem_name: missing(),
        driver: det("i915"),
        kind: det("integrated", "PCI identity and placement", "inferred"),
        boot_display: det(true),
        drm_nodes: ["card0", "renderD128"],
        memory: missing(),
      },
    ],
    storage: [
      {
        discovered_at: 1_794_900_000,
        name: "nvme0n1",
        device_path: "/dev/nvme0n1",
        sysfs_path: "/sys/devices/x/nvme0n1",
        kind: "ssd",
        interface: det("nvme", "/sys/block/nvme0n1", "inferred"),
        model: det("Example SSD 512"),
        vendor: missing(),
        firmware_revision: det("1.0"),
        capacity_bytes: det(512_110_190_592),
        logical_sector_bytes: det(512),
        physical_sector_bytes: det(512),
        rotational: det(false),
        removable: det(false),
        read_only: det(false),
        controller_pci_address: "0000:01:00.0",
        partitions: [{ name: "nvme0n1p1", device_path: "/dev/nvme0n1p1", number: 1, start_sector: 2048, size_bytes: 1024 ** 3, read_only: false, usage: unused() }],
        usage: unused(),
        slaves: [],
        mapper_name: null,
        mapper_path: null,
      },
      {
        discovered_at: 1_794_900_000,
        name: "loop0",
        device_path: "/dev/loop0",
        sysfs_path: null,
        kind: "virtual",
        interface: det("virtual"),
        model: missing(),
        vendor: missing(),
        firmware_revision: missing(),
        capacity_bytes: det(4096),
        logical_sector_bytes: det(512),
        physical_sector_bytes: det(512),
        rotational: det(false),
        removable: det(false),
        read_only: det(true),
        controller_pci_address: null,
        partitions: [],
        usage: unused(),
        slaves: [],
        mapper_name: null,
        mapper_path: null,
      },
    ],
    batteries: [],
    power_supplies: [{ discovered_at: 1, name: "AC", kind: det("Mains"), online: det(true) }],
    network: [
      {
        discovered_at: 1,
        name: "wlan0",
        kind: "wireless",
        mac_address: det("02:00:00:aa:bb:cc"),
        mac_address_origin: det("permanent"),
        operational_state: det("up"),
        carrier: det(true),
        speed_mbps: missing(),
        mtu: det(1500),
        driver: det("examplewifi"),
        device_bus: det("PCI 0000:02:00.0"),
        vendor_id: 0x1234,
        device_id: 0x5678,
        wireless_phy: "phy0",
      },
    ],
    usb_devices: [
      {
        discovered_at: 1,
        sysfs_name: "usb1",
        bus: 1,
        device_number: 1,
        vendor_id: 0x1d6b,
        product_id: 0x0002,
        manufacturer: det("Linux"),
        product: det("xHCI Host Controller"),
        vendor_name: det("Linux Foundation", "usb.ids", "database"),
        product_name: det("2.0 root hub", "usb.ids", "database"),
        usb_version: det("2.00"),
        speed_kbps: det(480_000),
        device_class: 9,
        class_name: "Hub",
        interface_classes: ["Hub"],
        is_root_hub: true,
        is_hub: true,
        parent: null,
        port_number: null,
        ports: [
          { number: 1, connect_type: "hotplug", device: "1-1" },
          { number: 2, connect_type: "hotplug", device: null },
        ],
        removable: missing(),
        controller_pci_address: "0000:00:14.0",
        sysfs_path: "/sys/devices/x/usb1",
      },
      {
        discovered_at: 1,
        sysfs_name: "1-1",
        bus: 1,
        device_number: 2,
        vendor_id: 0x1111,
        product_id: 0x2222,
        manufacturer: det("Example"),
        product: det("Example Mouse"),
        vendor_name: missing(),
        product_name: missing(),
        usb_version: det("2.00"),
        speed_kbps: det(12_000),
        device_class: 0,
        class_name: "Defined per interface",
        interface_classes: ["Human interface device"],
        is_root_hub: false,
        is_hub: false,
        parent: "usb1",
        port_number: 1,
        ports: [],
        removable: det("removable"),
        controller_pci_address: null,
        sysfs_path: "/sys/devices/x/usb1/1-1",
      },
    ],
    pci_devices: [],
    sensors: [
      {
        discovered_at: 1,
        id: "hwmon0/temp1",
        chip: "examplechip",
        kind: "temperature",
        label: det("Package"),
        input_path: "/sys/class/hwmon/hwmon0/temp1_input",
        access: det("permission_denied", "/sys/class/hwmon/hwmon0/temp1_input", "direct"),
        device: det("PCI 0000:00:18.3"),
      },
    ],
    thermal_zones: [],
    discovery_status: [
      { module: "memory", outcome: "partial", items: 1, duration_ms: 1, finished_at: 1, notes: ["Installed RAM is not available to this user."] },
    ],
    ...overrides,
  };
}

const progress: JobProgress = {
  job: "hardware",
  state: "completed",
  started_at: 1,
  finished_at: 2,
  current_module: null,
  modules: [],
  last_successful_scan: 1_794_900_000,
  error: null,
  cache_saved: true,
};

const view = (inv: SystemInventory | null, origin: InventoryView["origin"] = "this_session", source: InventoryView["source"] = "daemon"): InventoryView => ({
  inventory: inv,
  components: null,
  origin: inv ? origin : null,
  source,
  progress,
  warning: null,
});

const text = (html: string) => html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ");
const noop = () => {};

describe("field display", () => {
  it("never turns a missing value into zero or an empty string", () => {
    expect(show(denied<number>(), formatBytes)).toEqual({ text: "Administrator only", present: false });
    expect(show(missing<number>()).text).toBe("Not provided by this system");
    expect(show({ availability: "invalid", confidence: "unknown" }).text).toContain("placeholder");
    expect(show(null).present).toBe(false);
    expect(show(det(0)).text).toBe("0"); // a real zero stays a zero
  });

  it("formats units", () => {
    expect(formatBytes(7 * 1024 ** 3)).toBe("7.0 GiB");
    expect(formatCapacity(512_110_190_592)).toBe("512.1 GB");
    expect(maskMac("02:00:00:aa:bb:cc")).toBe("••:••:••:••:bb:cc");
  });

  it("labels cached data as cached and never as live", () => {
    expect(originLabel("this_session", "daemon", 1).cached).toBe(false);
    expect(originLabel("this_session", "daemon", 1).detail).toContain("not live");
    expect(originLabel("cache", "daemon", 1)).toMatchObject({ label: "Cached", cached: true });
    expect(originLabel("cache", "cache_file", 1).detail).toContain("not running");
  });

  it("knows when a scan is active", () => {
    expect(isScanning({ ...progress, state: "running" })).toBe(true);
    expect(isScanning({ ...progress, state: "queued" })).toBe(true);
    expect(isScanning(progress)).toBe(false);
    expect(isScanning(null)).toBe(false);
  });
});

describe("CPU topology", () => {
  it.each([
    [4, 1],
    [4, 2],
    [8, 2],
    [16, 2],
  ])("draws exactly %i cores with %i threads each", (count, threads) => {
    const layout = topologyLayout(processor(cores(count, threads)));
    expect(layout.cores).toHaveLength(count);
    expect(layout.cores.every((box) => box.threads.length === threads)).toBe(true);
    const cpus = layout.cores.flatMap((box) => box.threads.map((t) => t.cpu));
    expect(new Set(cpus).size).toBe(count * threads);
    expect(layout.cores.every((box) => box.x >= 0 && box.x + box.width <= layout.width)).toBe(true);
  });

  it("supports hybrid layouts with different thread counts per core", () => {
    const hybrid = [...cores(2, 2, "performance"), ...cores(4, 1, "efficiency").map((c, i) => ({ ...c, index: 2 + i, logical_cpus: [4 + i] }))];
    const layout = topologyLayout(processor(hybrid));
    expect(layout.cores.map((b) => b.threads.length)).toEqual([2, 2, 1, 1, 1, 1]);
  });

  it("draws nothing when topology is missing", () => {
    const html = renderToStaticMarkup(<HardwarePage inventory={view(inventory({ processors: [processor([])] }))} progress={progress} serviceRunning onScan={noop} onCancel={noop} />);
    expect(html).toContain("did not expose this processor");
  });
});

describe("overview with an inventory", () => {
  const render = (v: InventoryView | null) =>
    renderToStaticMarkup(
      <OverviewPage phase="ready" overview={null} config={null} saving={false} onPowerMode={noop} inventory={v} progress={progress} onScan={noop} onCancel={noop} onOpenHardware={noop} />,
    );

  it("shows the discovered identity and summary", () => {
    const html = text(render(view(inventory())));
    for (const expected of ["Example Corp Example Book 14", "EXB-14", "Example CPU 9000", "4 cores · 8 threads", "7.0 GiB usable", "Example SSD 512", "Example iGPU", "No battery", "Last scan", "This session"]) {
      expect(html).toContain(expected);
    }
  });

  it("does not claim installed RAM it could not read", () => {
    const html = text(render(view(inventory())));
    expect(html).toContain("Installed size: administrator only");
    expect(html).not.toMatch(/\d+(\.\d+)? GiB installed/);
  });

  it("marks cached inventory and test roots", () => {
    expect(text(render(view(inventory(), "cache", "cache_file")))).toContain("Cached");
    expect(render(view(inventory({ root_override: "/tmp/fixture" })))).toContain("not from this computer");
  });

  it("shows no live CPU or memory usage", () => {
    const html = text(render(view(inventory())));
    expect(html).not.toMatch(/\d+(\.\d+)?\s?%/);
    expect(html).toContain("Monitoring off · no live data");
  });

  it("counts hardware", () => {
    // 1 cpu + 1 gpu + 1 physical drive + 1 wireless + 1 usb device + memory
    expect(hardwareCount(inventory())).toBe(6);
    expect(summaryCards(null).every((c) => !c.primary.present)).toBe(true);
  });
});

describe("hardware page", () => {
  const categories: HardwareCategory[] = ["processor", "memory", "motherboard", "graphics", "storage", "battery", "network", "usb", "pci", "sensors"];
  const render = (category: HardwareCategory, inv = inventory()) =>
    text(renderToStaticMarkup(<HardwarePage inventory={view(inv)} progress={progress} serviceRunning onScan={noop} onCancel={noop} initial={category} />));

  it("renders every category", () => {
    for (const category of categories) expect(render(category).length).toBeGreaterThan(100);
  });

  it("shows sources and confidence", () => {
    const html = render("processor");
    expect(html).toContain("/sys/test");
    expect(html).toContain("Kernel");
    expect(render("graphics")).toContain("Inferred");
    expect(render("graphics")).toContain("ID database");
  });

  it("explains unavailable memory modules instead of inventing a layout", () => {
    const html = render("memory");
    expect(html).toContain("Administrator only");
    expect(html).toContain("shows no slot layout rather than guessing");
    expect(html).toContain("not live");
    expect(html).toContain("Memory: Partial");
  });

  it("draws memory modules only from discovered records", () => {
    const inv = inventory();
    inv.memory!.modules = det([
      { locator: "DIMM A", bank_locator: null, size_bytes: 8 * 1024 ** 3, memory_type: "DDR4", form_factor: "SODIMM", speed_mts: 3200, configured_speed_mts: 3200, manufacturer: "ExampleMem", part_number: "EX-1", populated: true },
      { locator: "DIMM B", bank_locator: null, size_bytes: null, memory_type: null, form_factor: null, speed_mts: null, configured_speed_mts: null, manufacturer: null, part_number: null, populated: false },
    ], "/sys/firmware/dmi/entries/17-0/raw", "firmware_reported");
    const html = render("memory", inv);
    expect(html).toContain("DIMM A");
    expect(html).toContain("Empty slot");
  });

  it("hides virtual block devices by default and masks MAC addresses", () => {
    expect(render("storage")).not.toContain("loop0 ·");
    expect(render("storage")).toContain("Show 1 virtual block devices");
    const net = render("network");
    expect(net).toContain("••:••:••:••:bb:cc");
    expect(net).not.toContain("02:00:00:aa:bb:cc");
  });

  it("shows the USB tree with Linux ports, not physical sockets", () => {
    const html = render("usb");
    expect(html).toContain("Example Mouse");
    expect(html).toContain("No device detected");
    expect(html).toContain("not physical sockets");
  });

  it("reports sensors without readings", () => {
    const html = render("sensors");
    expect(html).toContain("examplechip");
    expect(html).toContain("Administrator only");
    expect(html).not.toMatch(/°C/);
  });

  it("shows an honest empty state before a scan", () => {
    const html = text(renderToStaticMarkup(<HardwarePage inventory={view(null)} progress={null} serviceRunning={false} onScan={noop} onCancel={noop} />));
    expect(html).toContain("Not scanned yet");
    expect(html).toContain("Start the Sentinel service");
  });
});

describe("motherboard page", () => {
  it("states identity and draws no board", () => {
    const html = renderToStaticMarkup(<MotherboardPage inventory={view(inventory())} progress={progress} serviceRunning onScan={noop} onCancel={noop} />);
    expect(text(html)).toContain(MOTHERBOARD_STATE);
    expect(text(html)).toContain("EXB-14");
    expect(text(html)).toContain("Externally verified No");
    expect(html).not.toContain("<svg");
  });
});

describe("utilities", () => {
  const utility = (overrides: Partial<SystemUtility>): SystemUtility => ({
    id: "tool",
    name: "Tool",
    description: "A tool",
    category: "system_monitoring",
    interface: "graphical",
    installed: true,
    version: "1.0",
    executable: null,
    desktop_entry: { id: "tool.desktop", path: "/usr/share/applications/tool.desktop", scope: "system", terminal: false, no_display: false },
    package: null,
    icon: null,
    sources: ["desktop_entry"],
    launch_available: true,
    launch_note: null,
    ...overrides,
  });
  const list = [
    utility({ id: "gui", name: "Graphical Monitor" }),
    utility({ id: "cli", name: "Cli Tool", category: "hardware_information", desktop_entry: null, sources: ["executable"], launch_available: false, launch_note: "Command-line tool: run it yourself in a terminal. Sentinel does not run it." }),
    utility({ id: "absent", name: "Absent Tool", installed: false, desktop_entry: null, sources: ["catalogue"], launch_available: false, launch_note: "Not installed on this system." }),
  ];
  const utilities: UtilitiesView = {
    catalog: {
      schema_version: 1,
      discovered_at: 1_794_900_000,
      duration_ms: 1,
      utilities: list,
      services: [
        { name: "NetworkManager.service", description: "Network Manager", scope: "system", load_state: "loaded", active_state: "active", sub_state: "running", enabled_state: "enabled", unit_file_path: "/usr/lib/systemd/system/NetworkManager.service", relevant_category: "network_management" },
        { name: "other.service", description: "Unrelated", scope: "user", load_state: "loaded", active_state: "inactive", sub_state: "dead", enabled_state: null, unit_file_path: null, relevant_category: null },
      ],
      package_managers: [],
      discovery_status: [],
    },
    origin: "this_session",
    source: "daemon",
    progress: null,
    warning: null,
  };

  it("filters by search, category and installation", () => {
    expect(filterUtilities(list, { search: "", category: "all", installedOnly: true }).map((u) => u.id)).toEqual(["gui", "cli"]);
    expect(filterUtilities(list, { search: "", category: "all", installedOnly: false })).toHaveLength(3);
    expect(filterUtilities(list, { search: "graph", category: "all", installedOnly: true }).map((u) => u.id)).toEqual(["gui"]);
    expect(filterUtilities(list, { search: "", category: "hardware_information", installedOnly: true }).map((u) => u.id)).toEqual(["cli"]);
  });

  it("offers Open only for installed applications with a desktop entry", () => {
    const html = renderToStaticMarkup(<UtilitiesPage utilities={utilities} progress={null} serviceRunning launching={null} onScan={noop} onCancel={noop} onLaunch={noop} />);
    expect(html.match(/>Open</g)?.length).toBe(1);
    expect(text(html)).toContain("Sentinel does not run it");
    expect(text(html)).toContain("never installs");
    expect(text(html)).not.toContain("Absent Tool"); // installed-only by default
  });

  it("lists relevant services read-only", () => {
    const html = text(renderToStaticMarkup(<ServicesPage utilities={utilities} progress={null} serviceRunning onScan={noop} onCancel={noop} />));
    expect(html).toContain("NetworkManager.service");
    expect(html).not.toContain("other.service");
    expect(html).toContain("never starts, stops, enables or disables");
  });
});

describe("header", () => {
  const render = (device: { name: string | null; cached: boolean } | null) =>
    text(
      renderToStaticMarkup(
        <Header
          version="0.1.0"
          device={device}
          service={{ label: "Running", tone: "ok", detail: "" }}
          monitoring={{ label: "Not available", tone: "idle", detail: "" }}
          serviceOn
          serviceBusy={null}
          serviceControllable
          onServiceToggle={noop}
        />,
      ),
    );
  it("shows the detected device, and says when it is cached or missing", () => {
    expect(render({ name: "Example Corp Example Book 14", cached: false })).toContain("Example Corp Example Book 14");
    expect(render({ name: "Example Corp Example Book 14", cached: true })).toContain("cached");
    expect(render({ name: null, cached: false })).toContain("Not identified by firmware");
    expect(render(null)).toContain("Not scanned");
  });
});

describe("no hardware is hard-coded", () => {
  const files = (dir: string): string[] =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name);
      return statSync(path).isDirectory() ? files(path) : [path];
    });
  it("sources contain no real product names", () => {
    const sources = files(join(import.meta.dirname, "..")).filter((p) => /\.(ts|tsx)$/.test(p) && !p.includes(".test."));
    for (const path of sources) {
      const code = readFileSync(path, "utf8");
      for (const name of ["Ryzen", "Radeon", "Inspiron", "Dell", "KIOXIA", "Intel Core", "GeForce", "ThinkPad", "Mendocino"]) {
        expect(code.includes(name), `${path} hard-codes ${name}`).toBe(false);
      }
    }
  });
});
