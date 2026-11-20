import { useMemo, useState } from "react";

import { MODULE_LABEL } from "../app/catalog";
import {
  OUTCOME_TEXT,
  formatBytes,
  formatCapacity,
  formatGhz,
  hex4,
  kindLabel,
  maskMac,
  show,
  value,
} from "../app/inventoryView";
import { indexMaps, resolveSelection, type ExplorerTelemetry, type IndexMaps } from "../app/explorer";
import { ComponentTree } from "../components/ComponentTree";
import { CpuTopology } from "../components/CpuTopology";
import { BatteryCapacity, CacheHierarchy, DiskLayout, ExplorerOverview, GpuDiagram, MemoryMap, ProcessorDiagram } from "../components/ExplorerVisuals";
import { Inspector } from "../components/Inspector";
import { ReliabilityLegend } from "../components/Reliability";
import { DiscoveryPanel, OriginBadge } from "../components/DiscoveryPanel";
import { FieldRow, PlainRow } from "../components/FieldRow";
import { EmptyState, Panel, Tag } from "../components/primitives";
import type { ComponentCategory, ComponentNode, InventoryView, JobProgress, ModuleId, SystemInventory, UsbDevice } from "../types/inventory";
import type { DocumentsView } from "../types/documents";

export type HardwareCategory =
  | "processor"
  | "memory"
  | "motherboard"
  | "graphics"
  | "storage"
  | "battery"
  | "network"
  | "usb"
  | "pci"
  | "sensors";

const CATEGORIES: readonly { id: HardwareCategory; label: string; modules: ModuleId[] }[] = [
  { id: "processor", label: "Processor", modules: ["cpu"] },
  { id: "memory", label: "Memory", modules: ["memory"] },
  { id: "motherboard", label: "Motherboard", modules: ["motherboard", "system_identity", "firmware"] },
  { id: "graphics", label: "Graphics", modules: ["gpu"] },
  { id: "storage", label: "Storage", modules: ["storage"] },
  { id: "battery", label: "Battery", modules: ["battery"] },
  { id: "network", label: "Network", modules: ["network"] },
  { id: "usb", label: "USB", modules: ["usb"] },
  { id: "pci", label: "PCI devices", modules: ["pci"] },
  { id: "sensors", label: "Sensors", modules: ["sensors"] },
];

function count(inv: SystemInventory, id: HardwareCategory): number {
  switch (id) {
    case "processor":
      return inv.processors.length;
    case "memory":
      return inv.memory ? 1 : 0;
    case "motherboard":
      return inv.motherboard ? 1 : 0;
    case "graphics":
      return inv.graphics.length;
    case "storage":
      return inv.storage.filter((d) => d.kind !== "virtual").length;
    case "battery":
      return inv.batteries.length;
    case "network":
      return inv.network.length;
    case "usb":
      return inv.usb_devices.filter((d) => !d.is_root_hub).length;
    case "pci":
      return inv.pci_devices.length;
    case "sensors":
      return inv.sensors.length + inv.thermal_zones.length;
  }
}

/** Category shown when the explorer opens. */
export type ExplorerCategory = "overview" | HardwareCategory;

const COMPONENT_CATEGORY: Partial<Record<HardwareCategory, ComponentCategory>> = {
  processor: "processor",
  memory: "memory",
  motherboard: "motherboard",
  graphics: "graphics",
  storage: "storage",
  battery: "battery",
  network: "network",
  usb: "usb",
  pci: "pci",
};

/** Per-category selection, kept by the app so it survives navigation. */
export type ExplorerSelection = Partial<Record<HardwareCategory, string>>;

export function HardwarePage({
  inventory,
  progress,
  serviceRunning,
  onScan,
  onCancel,
  initial = "processor",
  telemetry = null,
  category: controlledCategory,
  onCategory,
  selection: controlledSelection,
  onSelection,
  documents = null,
  onOpenDocuments,
}: {
  inventory: InventoryView | null;
  progress: JobProgress | null;
  serviceRunning: boolean;
  onScan: (force: boolean) => void;
  onCancel: () => void;
  initial?: ExplorerCategory;
  /** The app-wide telemetry; the explorer never polls on its own. */
  telemetry?: ExplorerTelemetry | null;
  category?: ExplorerCategory;
  onCategory?: (c: ExplorerCategory) => void;
  selection?: ExplorerSelection;
  onSelection?: (s: ExplorerSelection) => void;
  /** Phase 6 documentation results for the detected board. */
  documents?: DocumentsView | null;
  onOpenDocuments?: () => void;
}) {
  const [localCategory, setLocalCategory] = useState<ExplorerCategory>(initial);
  const [localSelection, setLocalSelection] = useState<ExplorerSelection>({});
  const active = controlledCategory ?? localCategory;
  const setActive = onCategory ?? setLocalCategory;
  const selection = controlledSelection ?? localSelection;
  const setSelection = onSelection ?? setLocalSelection;
  const inv = inventory?.inventory ?? null;
  // Rebuilt only when a new inventory arrives, never per telemetry update.
  const maps = useMemo(() => indexMaps(inventory?.components), [inventory?.components]);
  const hasIndex = !!inventory?.components && inv !== null;
  const category = active === "overview" ? null : (CATEGORIES.find((c) => c.id === active) ?? CATEGORIES[0]!);
  const reports = inv?.discovery_status.filter((r) => category?.modules.includes(r.module)) ?? [];
  const componentCategory = category ? COMPONENT_CATEGORY[category.id] : undefined;
  const roots = componentCategory ? (maps.roots.get(componentCategory) ?? []) : [];
  const explicit = category ? selection[category.id] : undefined;
  const selectedId = explicit ?? roots[0]?.id ?? null;
  const resolved = resolveSelection(maps, selectedId);
  const select = (id: string) => {
    const node = maps.byId.get(id);
    const target = node ? (CATEGORIES.find((c) => COMPONENT_CATEGORY[c.id] === node.category)?.id ?? null) : null;
    if (!target) return;
    setSelection({ ...selection, [target]: id });
    if (target !== active) setActive(target);
  };
  const withInspector = hasIndex && componentCategory !== undefined;

  return (
    <div className="grid min-w-[980px] grid-cols-12 gap-3">
      <Panel title="Hardware explorer" className="col-span-3" bodyClassName="p-0" aside={inventory && inv ? <OriginBadge origin={inventory.origin} source={inventory.source} scannedAt={inv.discovered_at} /> : undefined}>
        <nav aria-label="Hardware categories">
          <ul>
            {[{ id: "overview" as const, label: "Overview" }, ...CATEGORIES].map((c) => (
              <li key={c.id}>
                <button
                  type="button"
                  onClick={() => setActive(c.id)}
                  aria-current={c.id === active ? "page" : undefined}
                  className={`flex w-full items-center justify-between border-b border-line px-3 py-2 text-left text-xs ${
                    c.id === active ? "bg-cyan/10 text-cyan" : "text-muted hover:bg-raised hover:text-fg"
                  }`}
                >
                  <span>{c.label}</span>
                  <span className="font-mono text-[11px] text-dim">{c.id === "overview" ? "" : inv ? count(inv, c.id) : "—"}</span>
                </button>
              </li>
            ))}
          </ul>
        </nav>
        {withInspector && roots.length > 0 ? (
          <div className="max-h-[420px] overflow-auto border-b border-line">
            <ComponentTree
              key={componentCategory}
              maps={maps}
              roots={roots}
              selectedId={resolved.node ? selectedId : null}
              onSelect={select}
              label={`${category!.label} components`}
              defaultDepth={componentCategory === "processor" ? 1 : componentCategory === "pci" || componentCategory === "usb" ? 3 : 2}
              detail={(n) => treeDetail(n, inv!)}
            />
          </div>
        ) : null}
        <div className="p-3">
          <DiscoveryPanel job="hardware" progress={progress} serviceRunning={serviceRunning} hasResult={inv !== null} onScan={onScan} onCancel={onCancel} />
        </div>
      </Panel>

      <div className={`${withInspector ? "col-span-6" : "col-span-9"} flex min-w-0 flex-col gap-3`}>
        {inv?.root_override ? (
          <div className="border border-orange/60 bg-orange/10 px-3 py-2 text-xs text-orange">Read from a test directory ({inv.root_override}), not from this computer.</div>
        ) : null}
        {inv ? (
          <>
            {reports.map((r) =>
              r.outcome !== "completed" ? (
                <div key={r.module} className="border border-line bg-panel px-3 py-2 text-[11px] text-muted">
                  <span className="mr-2 font-mono uppercase text-orange">{MODULE_LABEL[r.module]}: {OUTCOME_TEXT[r.outcome]}</span>
                  {r.notes.join(" ")}
                </div>
              ) : null,
            )}
            {active === "overview" ? (
              <>
                <ExplorerOverview inv={inv} telemetry={telemetry} onOpen={(c) => setActive(c as HardwareCategory)} />
                <Panel title="Reliability labels">
                  <ReliabilityLegend />
                </Panel>
              </>
            ) : hasIndex ? (
              <>
                {category!.id === "motherboard" ? <BoardDocuments view={documents} onOpen={onOpenDocuments} /> : null}
                <Visual inv={inv} category={category!.id} maps={maps} selectedId={selectedId} onSelect={select} telemetry={telemetry} />
                <details className="border border-line bg-panel" open={!componentCategory}>
                  <summary className="label cursor-pointer px-3 py-2 text-muted">All recorded fields</summary>
                  <div className="flex flex-col gap-3 p-3 pt-0">
                    <Detail inv={inv} category={category!.id} explorer />
                  </div>
                </details>
              </>
            ) : (
              <Detail inv={inv} category={category!.id} />
            )}
          </>
        ) : (
          <Panel title={category?.label ?? "Hardware overview"} className="h-[420px]">
            <EmptyState title="Not scanned yet">
              {serviceRunning
                ? "Run a hardware scan. Sentinel reads read-only Linux interfaces and shows only what it actually finds."
                : "Start the Sentinel service, then run a hardware scan."}
            </EmptyState>
          </Panel>
        )}
      </div>

      {withInspector ? (
        <div className="col-span-3 min-w-0">
          <Inspector node={resolved.node} lost={resolved.lost} inventory={inv!} maps={maps} telemetry={telemetry} onSelect={select} className="sticky top-0 max-h-[calc(100vh-120px)]" />
        </div>
      ) : null}
    </div>
  );
}

/** Documentation found for this board (Phase 6), linking to the library. */
function BoardDocuments({ view, onOpen }: { view: DocumentsView | null; onOpen?: () => void }) {
  const count = (c: string) => view?.candidates.filter((d) => d.assessment.class === c).length ?? 0;
  return (
    <Panel title="Board documentation" aside={<span className="label">{view?.online ? "Online discovery on" : "Offline"}</span>}>
      {view?.identity ? (
        <p className="text-xs text-muted">
          {view.candidates.length === 0
            ? view.last_search_at
              ? "No documents were found for this board."
              : "No search has been made for this board yet."
            : `${count("exact")} exact, ${count("high_confidence")} high-confidence and ${count("related_model")} related document(s) found. Exact matches still deserve a manual check.`}
        </p>
      ) : (
        <p className="text-xs text-dim">{view?.identity_problem ?? "Documentation needs the service and a hardware scan."}</p>
      )}
      {onOpen ? (
        <button type="button" onClick={onOpen} className="mt-2 border border-cyan/60 px-2 py-1 text-[11px] text-cyan hover:bg-cyan/10">
          Open the Schematic Library
        </button>
      ) : null}
    </Panel>
  );
}

/** Short secondary text in the component tree. */
function treeDetail(n: ComponentNode, inv: SystemInventory): string {
  switch (n.kind) {
    case "cpu_core":
      return `${n.children.length} thread${n.children.length === 1 ? "" : "s"}`;
    case "partition": {
      const p = inv.storage.flatMap((d) => d.partitions).find((x) => x.name === n.key);
      const m = p ? value(p.usage.mounts) : null;
      return m?.length ? m[0]!.mount_point : p?.size_bytes ? formatBytes(p.size_bytes) : "";
    }
    case "disk":
    case "virtual_block": {
      const d = inv.storage.find((x) => x.name === n.key);
      const c = d ? value(d.capacity_bytes) : null;
      const m = d ? value(d.usage.mounts) : null;
      return m?.length ? m[0]!.mount_point : c ? formatCapacity(c) : "";
    }
    case "usb_device":
    case "usb_hub":
    case "usb_root_hub": {
      const d = inv.usb_devices.find((x) => x.sysfs_name === n.key);
      return d ? `${hex4(d.vendor_id)}:${hex4(d.product_id)}` : "";
    }
    case "pci_device":
    case "pci_bridge":
      return n.key.replace(/^0000:/, "");
    case "network_interface":
      return kindLabel(inv.network.find((x) => x.name === n.key)?.kind ?? "other");
    default:
      return "";
  }
}

function Visual({ inv, category, maps, selectedId, onSelect, telemetry }: { inv: SystemInventory; category: HardwareCategory; maps: IndexMaps; selectedId: string | null; onSelect: (id: string) => void; telemetry: ExplorerTelemetry | null }) {
  switch (category) {
    case "processor":
      return inv.processors.length === 0 ? (
        <Panel title="Processor"><EmptyState title="No processor information" /></Panel>
      ) : (
        <>
          {inv.processors.map((cpu) => (
            <div key={cpu.package_id} className="flex flex-col gap-3">
              <ProcessorDiagram cpu={cpu} maps={maps} selectedId={selectedId} onSelect={onSelect} telemetry={telemetry} />
              <CacheHierarchy cpu={cpu} maps={maps} selectedId={selectedId} onSelect={onSelect} />
            </div>
          ))}
          {inv.memory?.numa_nodes.length ? null : <p className="text-[10px] text-dim">No NUMA topology reported: all memory is equally close to every CPU, or the kernel does not say.</p>}
        </>
      );
    case "memory":
      return inv.memory ? <MemoryMap memory={inv.memory} telemetry={telemetry} /> : <Panel title="Memory"><EmptyState title="Memory information is not available" /></Panel>;
    case "graphics":
      return inv.graphics.length === 0 ? (
        <Panel title="Graphics"><EmptyState title="No PCI graphics device detected" /></Panel>
      ) : (
        <>{inv.graphics.map((g) => <GpuDiagram key={g.pci_address} gpu={g} maps={maps} selectedId={selectedId} onSelect={onSelect} telemetry={telemetry} />)}</>
      );
    case "storage": {
      const disks = inv.storage.filter((d) => d.kind !== "virtual");
      return disks.length ? <>{disks.map((d) => <DiskLayout key={d.name} disk={d} maps={maps} selectedId={selectedId} onSelect={onSelect} telemetry={telemetry} />)}</> : <Panel title="Storage"><EmptyState title="No physical storage device detected" /></Panel>;
    }
    case "battery":
      return inv.batteries.length ? <BatteryCapacity inv={inv} telemetry={telemetry} /> : <Panel title="Battery"><EmptyState title="No battery detected">This is normal for desktop computers.</EmptyState></Panel>;
    case "network":
    case "usb":
    case "pci":
    case "motherboard": {
      const text: Record<string, string> = {
        network: "Adapters and software interfaces. Select one for its driver, link and live traffic. Traffic is counted by the kernel; Sentinel captures nothing.",
        usb: "USB topology as Linux reports it: host controllers, hubs and devices. Port numbers are Linux's, not physical sockets.",
        pci: "PCI functions under the bridges the kernel reports. This is bus topology, not a picture of the motherboard.",
        motherboard: "Board identity from firmware. Board layout and wiring are outside this release.",
      };
      return (
        <Panel title="About this view">
          <p className="text-[11px] text-muted">{text[category]}</p>
          {category === "usb" || category === "pci" ? <p className="mt-1 text-[11px] text-dim">Use the tree on the left; arrow keys move, right and left expand and collapse.</p> : null}
        </Panel>
      );
    }
    case "sensors":
      return null;
  }
}

function Detail({ inv, category, explorer = false }: { inv: SystemInventory; category: HardwareCategory; explorer?: boolean }) {
  switch (category) {
    case "processor":
      return <ProcessorDetail inv={inv} explorer={explorer} />;
    case "memory":
      return <MemoryDetail inv={inv} />;
    case "motherboard":
      return <MotherboardDetail inv={inv} />;
    case "graphics":
      return (
        <>
          {inv.graphics.length === 0 ? <Panel title="Graphics"><EmptyState title="No PCI graphics device detected" /></Panel> : null}
          {inv.graphics.map((g) => (
            <Panel key={g.pci_address} title={`Graphics · ${g.pci_address}`}>
              <FieldRow label="Model" field={g.model_name} />
              <FieldRow label="Vendor" field={g.vendor_name} />
              <FieldRow label="Type" field={g.kind} render={kindLabel} />
              <FieldRow label="Driver" field={g.driver} />
              <FieldRow
                label="Graphics memory"
                field={g.memory}
                render={(m) => `${formatBytes(m.bytes)} ${m.kind === "dedicated_vram" ? "dedicated VRAM" : "reserved from system RAM (not separate memory)"}`}
              />
              <FieldRow label="Boot display" field={g.boot_display} render={(b) => (b ? "Yes" : "No")} />
              <FieldRow label="Subsystem" field={g.subsystem_name} />
              <PlainRow label="PCI IDs">{`${hex4(g.vendor_id)}:${hex4(g.device_id)}${g.subsystem_vendor_id !== null ? ` (subsystem ${hex4(g.subsystem_vendor_id)}:${hex4(g.subsystem_device_id ?? 0)})` : ""}`}</PlainRow>
              <PlainRow label="DRM nodes">{g.drm_nodes.join(", ") || "None"}</PlainRow>
            </Panel>
          ))}
        </>
      );
    case "storage":
      return <StorageDetail inv={inv} />;
    case "battery":
      return (
        <>
          {inv.batteries.length === 0 ? <Panel title="Battery"><EmptyState title="No battery detected">This is normal for desktop computers.</EmptyState></Panel> : null}
          {inv.batteries.map((b) => (
            <Panel key={b.name} title={`Battery · ${b.name}`} aside={<span className="label">Changing values are the reading at scan time</span>}>
              <FieldRow label="Manufacturer" field={b.manufacturer} />
              <FieldRow label="Model" field={b.model_name} />
              <FieldRow label="Technology" field={b.technology} />
              <FieldRow label="Status at scan" field={b.status} render={kindLabel} />
              <FieldRow label="Charge at scan" field={b.capacity_percent} render={(p) => `${p}%`} />
              <FieldRow label="Health" field={b.health_percent} render={(h) => `${h.toFixed(1)}% of design capacity`} />
              <FieldRow label="Full (energy)" field={b.energy_full_uwh} render={(v) => `${(v / 1e6).toFixed(2)} Wh`} />
              <FieldRow label="Design (energy)" field={b.energy_full_design_uwh} render={(v) => `${(v / 1e6).toFixed(2)} Wh`} />
              <FieldRow label="Full (charge)" field={b.charge_full_uah} render={(v) => `${(v / 1e3).toFixed(0)} mAh`} />
              <FieldRow label="Design (charge)" field={b.charge_full_design_uah} render={(v) => `${(v / 1e3).toFixed(0)} mAh`} />
              <FieldRow label="Cycle count" field={b.cycle_count} />
              <FieldRow label="Min. design voltage" field={b.voltage_min_design_uv} render={(v) => `${(v / 1e6).toFixed(2)} V`} />
            </Panel>
          ))}
          {inv.power_supplies.length > 0 ? (
            <Panel title="Power supplies">
              {inv.power_supplies.map((p) => (
                <FieldRow key={p.name} label={`${p.name} (${show(p.kind).text})`} field={p.online} render={(o) => (o ? "Supplying power at scan time" : "Not supplying power at scan time")} />
              ))}
            </Panel>
          ) : null}
        </>
      );
    case "network":
      return <NetworkDetail inv={inv} />;
    case "usb":
      return <UsbDetail inv={inv} />;
    case "pci":
      return (
        <Panel title={`PCI devices · ${inv.pci_devices.length}`} bodyClassName="max-h-[640px] overflow-auto p-0">
          <table className="w-full text-left text-[11px]">
            <thead className="sticky top-0 bg-panel text-dim">
              <tr className="border-b border-line">
                {["Address", "Class", "Device", "Vendor", "IDs", "Driver", "Upstream"].map((h) => (
                  <th key={h} className="label px-2 py-1.5 font-normal">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {inv.pci_devices.map((d) => (
                <tr key={d.address} className="border-b border-line">
                  <td className="selectable px-2 py-1 font-mono text-muted">{d.address}</td>
                  <td className="px-2 py-1 text-muted">{d.class_name}</td>
                  <td className="selectable px-2 py-1 text-fg">{show(d.device_name).text}</td>
                  <td className="px-2 py-1 text-muted">{show(d.vendor_name).text}</td>
                  <td className="selectable px-2 py-1 font-mono text-dim">{`${hex4(d.vendor_id)}:${hex4(d.device_id)}`}</td>
                  <td className="px-2 py-1 font-mono text-muted">{value(d.driver) ?? "—"}</td>
                  <td className="px-2 py-1 font-mono text-dim">{d.parent ?? "root"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="p-2 text-[10px] text-dim">Names come from the PCI ID database installed on this system. Upstream links are bus topology, not board wiring.</p>
        </Panel>
      );
    case "sensors":
      return (
        <Panel title={`Sensor interfaces · ${inv.sensors.length}`} aside={<span className="label">Discovered only · no readings taken</span>} bodyClassName="max-h-[640px] overflow-auto p-0">
          <table className="w-full text-left text-[11px]">
            <thead className="sticky top-0 bg-panel">
              <tr className="border-b border-line">
                {["Chip", "Sensor", "Kind", "Label", "Provided by", "Access"].map((h) => (
                  <th key={h} className="label px-2 py-1.5 font-normal">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {inv.sensors.map((s) => (
                <tr key={s.id} className="border-b border-line">
                  <td className="px-2 py-1 font-mono text-fg">{s.chip}</td>
                  <td className="selectable px-2 py-1 font-mono text-muted" title={s.input_path}>{s.id}</td>
                  <td className="px-2 py-1 text-muted">{kindLabel(s.kind)}</td>
                  <td className="px-2 py-1 text-muted">{value(s.label) ?? "—"}</td>
                  <td className="px-2 py-1 font-mono text-dim">{value(s.device) ?? "—"}</td>
                  <td className={`px-2 py-1 ${value(s.access) === "permission_denied" ? "text-orange" : "text-muted"}`}>{value(s.access) === "readable" ? "Readable" : value(s.access) === "permission_denied" ? "Administrator only" : "Unknown"}</td>
                </tr>
              ))}
              {inv.thermal_zones.map((z) => (
                <tr key={z.name} className="border-b border-line">
                  <td className="px-2 py-1 font-mono text-fg">thermal</td>
                  <td className="px-2 py-1 font-mono text-muted">{z.name}</td>
                  <td className="px-2 py-1 text-muted">Thermal zone</td>
                  <td className="px-2 py-1 text-muted">{value(z.zone_type) ?? "—"}</td>
                  <td className="px-2 py-1 font-mono text-dim">{z.trip_points} trip points</td>
                  <td className="px-2 py-1 text-muted">{value(z.access) === "readable" ? "Readable" : "Administrator only"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="p-2 text-[10px] text-dim">A sensor's providing device is the chip's driver attachment; it does not say which component is measured.</p>
        </Panel>
      );
  }
}

function ProcessorDetail({ inv, explorer = false }: { inv: SystemInventory; explorer?: boolean }) {
  const [selected, setSelected] = useState<number | null>(null);
  if (inv.processors.length === 0) return <Panel title="Processor"><EmptyState title="No processor information" /></Panel>;
  return (
    <>
      {inv.processors.map((cpu) => {
        const core = cpu.cores.find((c) => c.index === selected) ?? null;
        return (
          <div key={cpu.package_id} className="flex flex-col gap-3">
            <Panel title={`Processor · package ${cpu.package_id}`}>
              <FieldRow label="Model" field={cpu.model_name} />
              <FieldRow label="Vendor" field={cpu.vendor} />
              <FieldRow label="Microarchitecture" field={cpu.microarchitecture} />
              <FieldRow label="Architecture" field={cpu.architecture} />
              <FieldRow label="Physical cores" field={cpu.physical_cores} />
              <FieldRow label="Logical processors" field={cpu.logical_processors} />
              <FieldRow label="Hybrid core types" field={cpu.hybrid} render={(h) => (h ? "Yes" : "No")} />
              <FieldRow label="Family / model / stepping" field={cpu.family} render={(f) => `${f} / ${value(cpu.model) ?? "?"} / ${value(cpu.stepping) ?? "?"}`} />
              <FieldRow label="Minimum frequency" field={cpu.min_frequency_khz} render={formatGhz} />
              <FieldRow label="Maximum frequency" field={cpu.max_frequency_khz} render={formatGhz} />
              <PlainRow label="Caches">
                {cpu.caches.map((c) => `L${c.level} ${c.kind}: ${c.instances} × ${c.size_bytes ? formatBytes(c.size_bytes) : "?"}`).join(" · ") || "Not reported"}
              </PlainRow>
              {cpu.offline_cpus.length ? <PlainRow label="Offline CPUs">{cpu.offline_cpus.join(", ")}</PlainRow> : null}
              <FieldRow label="Instruction flags" field={cpu.flags} render={(f) => `${f.length} flags`} />
            </Panel>
            {explorer ? null : <div className="grid grid-cols-3 gap-3">
              <Panel title="Core topology" className="col-span-2 h-[360px]" bodyClassName="p-2" aside={<span className="label">Logical diagram from sysfs</span>}>
                <CpuTopology processor={cpu} selectedCore={selected} onSelect={setSelected} />
              </Panel>
              <Panel title={core ? `Core ${core.index}` : "Selected core"} className="h-[360px]">
                {core ? (
                  <>
                    <PlainRow label="Kernel core ID">{core.core_id}</PlainRow>
                    <PlainRow label="Logical CPUs">{core.logical_cpus.map((c) => `CPU ${c}`).join(", ")}</PlainRow>
                    <PlainRow label="Core type">{core.kind === "uniform" ? "Same as all others" : kindLabel(core.kind)}</PlainRow>
                    <PlainRow label="Die">{core.die_id ?? "Not reported"}</PlainRow>
                    <PlainRow label="Cluster">{core.cluster_id ?? "Not reported"}</PlainRow>
                    <PlainRow label="Max frequency">{core.max_frequency_khz ? formatGhz(core.max_frequency_khz) : "Not reported"}</PlainRow>
                    <p className="pt-2 text-[10px] text-dim">Static identity only. Live per-core utilisation is on the Telemetry page while monitoring is on.</p>
                  </>
                ) : (
                  <EmptyState title="Select a core" />
                )}
              </Panel>
            </div>}
          </div>
        );
      })}
    </>
  );
}

function MemoryDetail({ inv }: { inv: SystemInventory }) {
  const m = inv.memory;
  if (!m) return <Panel title="Memory"><EmptyState title="Memory information is not available" /></Panel>;
  const modules = value(m.modules);
  return (
    <>
      <Panel title="Memory">
        <FieldRow label="Installed RAM" field={m.installed_bytes} render={formatBytes} />
        <FieldRow label="Kernel-managed" field={m.kernel_physical_bytes} render={formatBytes} />
        <FieldRow label="Usable by the system" field={m.usable_bytes} render={formatBytes} />
        <FieldRow label="Swap" field={m.swap_total_bytes} render={formatBytes} />
        <FieldRow label="Memory type" field={m.memory_type} />
        <FieldRow label="Speed" field={m.speed_mts} render={(s) => `${s} MT/s`} />
        <FieldRow label="Slots (populated / total)" field={m.slots_populated} render={(p) => `${p} / ${value(m.slots_total) ?? "?"}`} />
        <PlainRow label="NUMA nodes">{m.numa_nodes.length ? m.numa_nodes.map((n) => `node ${n.id}: CPUs ${n.cpus.length}${n.memory_bytes ? `, ${formatBytes(n.memory_bytes)}` : ""}`).join(" · ") : "Not reported"}</PlainRow>
      </Panel>
      <Panel title="Memory modules">
        {modules && modules.length ? (
          <div className="grid grid-cols-4 gap-2">
            {modules.map((mod, i) => (
              <div key={i} className={`border p-2 ${mod.populated ? "border-purple/60" : "border-line border-dashed"}`}>
                <div className="font-mono text-[11px] text-purple">{mod.locator ?? `Module ${i + 1}`}</div>
                <div className="mt-1 font-mono text-xs text-fg">{mod.populated ? (mod.size_bytes ? formatBytes(mod.size_bytes) : "Size unknown") : "Empty slot"}</div>
                <div className="text-[10px] text-dim">{[mod.memory_type, mod.form_factor, mod.speed_mts ? `${mod.speed_mts} MT/s` : null, mod.manufacturer, mod.part_number].filter(Boolean).join(" · ")}</div>
              </div>
            ))}
          </div>
        ) : (
          <EmptyState title="Module layout not shown">
            {m.modules.availability === "permission_denied"
              ? "The firmware's memory records can be read only by administrators. Sentinel does not ask for elevated privileges, so it shows no slot layout rather than guessing one."
              : "This system does not report its memory modules."}
          </EmptyState>
        )}
      </Panel>
      <Panel title="Usage snapshot" aside={<span className="label">At scan time · not live</span>}>
        <FieldRow label="Available" field={m.snapshot.available_bytes} render={formatBytes} />
        <FieldRow label="Cached" field={m.snapshot.cached_bytes} render={formatBytes} />
        <FieldRow label="In use" field={m.snapshot.used_bytes} render={formatBytes} />
      </Panel>
    </>
  );
}

function MotherboardDetail({ inv }: { inv: SystemInventory }) {
  const b = inv.motherboard;
  const s = inv.system;
  const f = inv.firmware;
  return (
    <>
      <Panel title="Motherboard" aside={<Tag kind="stored">{b ? b.identification.level.replace(/_/g, " ") : "unidentified"}</Tag>}>
        {b ? (
          <>
            <p className="mb-2 text-[11px] text-muted">{b.identification.explanation}</p>
            <FieldRow label="System manufacturer" field={b.system_manufacturer} />
            <FieldRow label="System model" field={b.system_model} />
            <FieldRow label="System family" field={b.system_family} />
            <FieldRow label="Board manufacturer" field={b.board_manufacturer} />
            <FieldRow label="Board name" field={b.board_name} />
            <FieldRow label="Board revision" field={b.board_revision} />
          </>
        ) : (
          <EmptyState title="Board identity not available" />
        )}
      </Panel>
      <Panel title="Firmware">
        <FieldRow label="BIOS vendor" field={f?.bios_vendor} />
        <FieldRow label="BIOS version" field={f?.bios_version} />
        <FieldRow label="BIOS date" field={f?.bios_release_date} />
        <FieldRow label="BIOS revision" field={f?.bios_revision} />
        <FieldRow label="EC revision" field={f?.embedded_controller_revision} />
        <FieldRow label="Boot mode" field={f?.boot_mode} render={(m) => m.toUpperCase()} />
      </Panel>
      <Panel title="Operating system">
        <FieldRow label="Distribution" field={s?.os.pretty_name} />
        <FieldRow label="Kernel" field={s?.kernel_release} />
        <FieldRow label="Architecture" field={s?.architecture} />
        <FieldRow label="Chassis" field={s?.chassis} render={kindLabel} />
        {inv.fingerprint ? <PlainRow label="Hardware fingerprint">{`${inv.fingerprint.value} (local only, from model IDs)`}</PlainRow> : null}
      </Panel>
    </>
  );
}

function StorageDetail({ inv }: { inv: SystemInventory }) {
  const [showVirtual, setShowVirtual] = useState(false);
  const devices = inv.storage.filter((d) => showVirtual || d.kind !== "virtual");
  const virtualCount = inv.storage.filter((d) => d.kind === "virtual").length;
  return (
    <>
      {devices.map((d) => (
        <Panel key={d.name} title={`${d.name} · ${kindLabel(d.kind)}`} aside={<span className="label selectable">{d.device_path}</span>}>
          <FieldRow label="Model" field={d.model} />
          <FieldRow label="Vendor" field={d.vendor} />
          <FieldRow label="Interface" field={d.interface} render={(i) => i.toUpperCase()} />
          <FieldRow label="Capacity" field={d.capacity_bytes} render={(c) => `${formatCapacity(c)} (${formatBytes(c)})`} />
          <FieldRow label="Sector size (logical)" field={d.logical_sector_bytes} render={(v) => `${v} B`} />
          <FieldRow label="Sector size (physical)" field={d.physical_sector_bytes} render={(v) => `${v} B`} />
          <FieldRow label="Rotational" field={d.rotational} render={(r) => (r ? "Yes (spinning disk)" : "No")} />
          <FieldRow label="Removable" field={d.removable} render={(r) => (r ? "Yes" : "No")} />
          <FieldRow label="Read-only" field={d.read_only} render={(r) => (r ? "Yes" : "No")} />
          <FieldRow label="Firmware" field={d.firmware_revision} />
          {d.controller_pci_address ? <PlainRow label="Controller">{`PCI ${d.controller_pci_address}`}</PlainRow> : null}
          {d.partitions.length ? (
            <div className="mt-2">
              <span className="label">Partitions</span>
              <ul className="mt-1 grid grid-cols-2 gap-x-4">
                {d.partitions.map((p) => (
                  <li key={p.name} className="flex justify-between border-b border-line py-1 font-mono text-[11px]">
                    <span className="text-muted">{p.name}</span>
                    <span className="text-fg">{p.size_bytes !== null ? formatBytes(p.size_bytes) : "?"}</span>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </Panel>
      ))}
      {virtualCount > 0 ? (
        <button type="button" onClick={() => setShowVirtual(!showVirtual)} className="self-start text-[11px] text-muted underline">
          {showVirtual ? "Hide" : "Show"} {virtualCount} virtual block devices (loop, zram, device-mapper)
        </button>
      ) : null}
      <p className="text-[10px] text-dim">Sentinel never mounts, unmounts, partitions or tests drives. SMART health is not read in this release.</p>
    </>
  );
}

function NetworkDetail({ inv }: { inv: SystemInventory }) {
  const [revealed, setRevealed] = useState<string | null>(null);
  return (
    <>
      {inv.network.map((n) => {
        const mac = value(n.mac_address);
        return (
          <Panel key={n.name} title={`${n.name} · ${kindLabel(n.kind)}`}>
            <FieldRow
              label="MAC address"
              field={n.mac_address}
              sensitive={
                mac ? (
                  <button type="button" className="font-mono text-xs text-fg" onClick={() => setRevealed(revealed === n.name ? null : n.name)} title="A local identifier. Click to show or hide.">
                    {revealed === n.name ? mac : maskMac(mac)}
                  </button>
                ) : undefined
              }
            />
            <FieldRow label="Address origin" field={n.mac_address_origin} render={kindLabel} />
            <FieldRow label="Link state at scan" field={n.operational_state} />
            <FieldRow label="Link speed" field={n.speed_mbps} render={(s) => `${s} Mb/s`} />
            <FieldRow label="Driver" field={n.driver} />
            <FieldRow label="Device" field={n.device_bus} />
            {n.wireless_phy ? <PlainRow label="Wireless PHY">{n.wireless_phy}</PlainRow> : null}
          </Panel>
        );
      })}
      <p className="text-[10px] text-dim">No traffic is captured or measured. MAC addresses stay on this computer and are never logged.</p>
    </>
  );
}

function usbName(d: UsbDevice): string {
  return value(d.product) ?? value(d.product_name) ?? `${hex4(d.vendor_id)}:${hex4(d.product_id)}`;
}

function UsbDetail({ inv }: { inv: SystemInventory }) {
  const byName = new Map(inv.usb_devices.map((d) => [d.sysfs_name, d]));
  const roots = inv.usb_devices.filter((d) => d.is_root_hub);
  const renderHub = (hub: UsbDevice, depth: number): React.ReactNode => (
    <ul className={depth ? "ml-5 border-l border-line pl-3" : ""}>
      {hub.ports.map((port) => {
        const child = port.device ? byName.get(port.device) : undefined;
        return (
          <li key={port.number} className="py-0.5">
            <span className="font-mono text-[11px] text-dim">{`Port ${port.number}`}</span>
            {port.connect_type ? <span className="ml-2 text-[10px] text-dim">({port.connect_type})</span> : null}
            {child ? (
              <>
                <span className="ml-2 text-xs text-fg">{usbName(child)}</span>
                <span className="ml-2 font-mono text-[10px] text-muted">{`${hex4(child.vendor_id)}:${hex4(child.product_id)} · ${child.interface_classes.join(", ") || child.class_name}${value(child.speed_kbps) ? ` · ${(value(child.speed_kbps)! / 1000).toLocaleString()} Mb/s` : ""}`}</span>
                {child.is_hub ? renderHub(child, depth + 1) : null}
              </>
            ) : (
              <span className="ml-2 text-[11px] text-dim">No device detected</span>
            )}
          </li>
        );
      })}
    </ul>
  );
  return (
    <Panel title="USB topology" aside={<span className="label">Linux port numbers · not physical sockets</span>}>
      {roots.map((root) => (
        <div key={root.sysfs_name} className="border-b border-line py-2 last:border-b-0">
          <div className="font-mono text-xs text-blue">
            {`${root.sysfs_name.toUpperCase()} · ${show(root.product_name).text} · USB ${show(root.usb_version).text}`}
            {root.controller_pci_address ? <span className="ml-2 text-dim">controller PCI {root.controller_pci_address}</span> : null}
          </div>
          {renderHub(root, 0)}
        </div>
      ))}
      <p className="pt-2 text-[10px] text-dim">
        Which socket on the case a port corresponds to is unknown until verified board documentation exists.
      </p>
    </Panel>
  );
}
