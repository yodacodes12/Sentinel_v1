// Logical diagrams for the hardware explorer. Plain CSS/SVG, no layout
// engine. Every shape stands for a discovered component and every
// relationship comes from the component index; nothing is placed where it
// physically sits, and each diagram says so.

import type { ReactNode } from "react";

import { cpuUsage, liveState, type ExplorerTelemetry, type IndexMaps } from "../app/explorer";
import { formatBytes, formatCapacity, show, value } from "../app/inventoryView";
import { celsius, pct, rate, reading, val, watts } from "../app/telemetryView";
import type { ComponentNode, GpuInfo, MemoryInfo, ProcessorInfo, StorageDevice, SystemInventory } from "../types/inventory";
import { CpuTopology } from "./CpuTopology";
import { Panel } from "./primitives";

type Select = (id: string) => void;

function Selectable({ id, selectedId, onSelect, label, className, children, style }: { id: string; selectedId: string | null; onSelect: Select; label: string; className: string; children: ReactNode; style?: React.CSSProperties }) {
  const selected = id === selectedId;
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={selected}
      onClick={() => onSelect(id)}
      style={style}
      className={`${className} text-left outline-none focus-visible:ring-1 focus-visible:ring-cyan ${selected ? "border-cyan bg-cyan/10" : ""}`}
    >
      {children}
    </button>
  );
}

// ------------------------------------------------------------------ CPU

/** Core topology with live per-CPU utilisation, wired to the selection. */
export function ProcessorDiagram({ cpu, maps, selectedId, onSelect, telemetry }: { cpu: ProcessorInfo; maps: IndexMaps; selectedId: string | null; onSelect: Select; telemetry: ExplorerTelemetry | null }) {
  const pkg = `cpu:${cpu.package_id}`;
  const selected = selectedId ? maps.byId.get(selectedId) : undefined;
  const coreNode = selected?.kind === "cpu_core" ? selected : selected?.kind === "logical_cpu" && selected.parent ? maps.byId.get(selected.parent) : undefined;
  const selectedCore = coreNode && coreNode.parent === pkg ? Number(coreNode.key) : null;
  const state = liveState(telemetry);
  const usage = state === "off" ? undefined : cpuUsage(telemetry?.snapshot);
  const measured = telemetry?.snapshot?.cpu?.measured_at;
  return (
    <Panel
      title={`Core topology · package ${cpu.package_id}`}
      className="h-[340px]"
      bodyClassName="p-2"
      aside={
        <span className="label">
          {state === "off" ? "Logical diagram from sysfs" : `${state === "live" ? "Live" : "Stale"} utilisation${measured ? ` · ${new Date(measured).toLocaleTimeString([], { hour12: false })}` : ""}`}
        </span>
      }
    >
      <CpuTopology processor={cpu} selectedCore={selectedCore} onSelect={(index) => onSelect(`${pkg}/core:${index}`)} usage={usage} />
    </Panel>
  );
}

const CACHE_ORDER = ["l1-data", "l1-instruction", "l1-unified", "l2-unified", "l2-data", "l2-instruction", "l3-unified", "l4-unified"];

/**
 * Cache levels as rows and cores as columns. Each cache instance spans the
 * cores the kernel says share it, so a shared L3 is one box across every
 * core rather than a copy per core.
 */
export function CacheHierarchy({ cpu, maps, selectedId, onSelect }: { cpu: ProcessorInfo; maps: IndexMaps; selectedId: string | null; onSelect: Select }) {
  const pkg = maps.byId.get(`cpu:${cpu.package_id}`);
  if (!pkg) return null;
  const cores = pkg.children.map((id) => maps.byId.get(id)).filter((n): n is ComponentNode => n?.kind === "cpu_core");
  const column = new Map(cores.map((c, i) => [c.id, i]));
  const caches = pkg.children.map((id) => maps.byId.get(id)).filter((n): n is ComponentNode => n?.kind === "cache");
  if (cores.length === 0 || caches.length === 0) {
    return (
      <Panel title="Cache hierarchy" className="h-[200px]">
        <p className="text-xs text-dim">The kernel did not report this processor's caches.</p>
      </Panel>
    );
  }
  const rows = new Map<string, ComponentNode[]>();
  for (const c of caches) {
    const group = c.id.slice(c.id.indexOf("/cache:") + 7).replace(/-\d+$/, "");
    rows.set(group, [...(rows.get(group) ?? []), c]);
  }
  const groups = [...rows.keys()].sort((a, b) => (CACHE_ORDER.indexOf(a) + 99) % 99 - (CACHE_ORDER.indexOf(b) + 99) % 99 || a.localeCompare(b));
  const size = (n: ComponentNode) => {
    const [level, kind] = n.key.split(":");
    const info = cpu.caches.find((c) => String(c.level) === level && c.kind === kind);
    return info?.size_bytes ? formatBytes(info.size_bytes).replace(".0 ", " ") : "size ?";
  };
  return (
    <Panel title="Cache hierarchy" aside={<span className="label">Sharing from sysfs · not die positions</span>} bodyClassName="overflow-x-auto p-2">
      <div className="grid gap-1" style={{ gridTemplateColumns: `72px repeat(${cores.length}, minmax(34px, 1fr))` }}>
        <span />
        {cores.map((c) => (
          <span key={c.id} className="text-center font-mono text-[9px] text-dim">{`C${c.key}`}</span>
        ))}
        {groups.map((g) => (
          <div key={g} className="contents">
            <span className="self-center font-mono text-[10px] uppercase text-muted">{g.replace("-", " ").replace("instruction", "instr")}</span>
            {rows.get(g)!.flatMap((cache) => {
              const cols = cache.links.filter((l) => l.relation === "shared_by").map((l) => column.get(l.id)).filter((i): i is number => i !== undefined).sort((a, b) => a - b);
              // Contiguous runs of columns; a non-contiguous sharing set is
              // drawn as several boxes with the same name.
              const runs: [number, number][] = [];
              for (const c of cols) {
                const last = runs[runs.length - 1];
                if (last && c === last[1] + 1) last[1] = c;
                else runs.push([c, c]);
              }
              return runs.map(([start, end], r) => (
                <Selectable
                  key={`${cache.id}-${r}`}
                  id={cache.id}
                  selectedId={selectedId}
                  onSelect={onSelect}
                  label={`${cache.name}, ${size(cache)}, shared by ${cols.length} core${cols.length === 1 ? "" : "s"}`}
                  style={{ gridColumn: `${start + 2} / ${end + 3}`, gridRow: `${groups.indexOf(g) + 2}` }}
                  className="truncate border border-purple/50 px-1 py-0.5 text-center font-mono text-[9.5px] text-purple hover:bg-purple/10"
                >
                  {size(cache)}
                </Selectable>
              ));
            })}
          </div>
        ))}
      </div>
      <p className="mt-2 text-[10px] text-dim">Each box is one cache instance spanning the cores that share it, as listed in shared_cpu_list.</p>
    </Panel>
  );
}

// ---------------------------------------------------------------- memory

function Bar({ label, parts, total, note }: { label: string; parts: { value: number; className: string; title: string }[]; total: number; note: ReactNode }) {
  return (
    <div className="mb-2">
      <div className="flex justify-between text-[11px]">
        <span className="text-muted">{label}</span>
        <span className="font-mono text-dim">{note}</span>
      </div>
      <div className="mt-0.5 flex h-2.5 w-full border border-line bg-canvas" role="img" aria-label={`${label}: ${parts.map((p) => p.title).join(", ")}`}>
        {parts.map((p) => (
          <span key={p.title} title={p.title} className={p.className} style={{ width: `${Math.max(0, Math.min(100, (p.value / total) * 100))}%` }} />
        ))}
      </div>
    </div>
  );
}

/** Installed vs kernel-managed vs usable, and live use of the usable part. */
export function MemoryMap({ memory, telemetry }: { memory: MemoryInfo; telemetry: ExplorerTelemetry | null }) {
  const installed = value(memory.installed_bytes);
  const kernel = value(memory.kernel_physical_bytes);
  const usable = value(memory.usable_bytes);
  const scale = installed ?? kernel ?? usable ?? 1;
  const state = liveState(telemetry);
  const m = state === "off" ? null : telemetry?.snapshot?.memory ?? null;
  const used = val(m?.used_bytes);
  const total = val(m?.total_bytes) ?? usable;
  const swapUsed = val(m?.swap_used_bytes);
  const swapTotal = val(m?.swap_total_bytes) ?? value(memory.swap_total_bytes);
  return (
    <Panel title="Memory capacity" aside={<span className="label">{state === "off" ? "Scan values · monitoring off" : state === "live" ? "Live use" : "Stale"}</span>}>
      <Bar
        label="Installed (firmware)"
        total={scale}
        parts={installed !== null ? [{ value: installed, className: "bg-purple/50", title: `${formatBytes(installed)} installed` }] : []}
        note={installed !== null ? formatBytes(installed) : show(memory.installed_bytes).text}
      />
      <Bar label="Kernel-managed" total={scale} parts={kernel !== null ? [{ value: kernel, className: "bg-purple/35", title: `${formatBytes(kernel)} managed by the kernel` }] : []} note={kernel !== null ? formatBytes(kernel) : show(memory.kernel_physical_bytes).text} />
      <Bar
        label="Usable by the system"
        total={scale}
        parts={
          usable !== null
            ? used !== null && total !== null
              ? [
                  { value: (used / total) * usable, className: "bg-purple", title: `${formatBytes(used)} in use` },
                  { value: usable - (used / total) * usable, className: "bg-purple/25", title: `${formatBytes(total - used)} not in use` },
                ]
              : [{ value: usable, className: "bg-purple/25", title: `${formatBytes(usable)} usable` }]
            : []
        }
        note={usable !== null ? (used !== null ? `${formatBytes(used)} in use of ${formatBytes(usable)}` : formatBytes(usable)) : show(memory.usable_bytes).text}
      />
      {swapTotal ? (
        <Bar
          label="Swap"
          total={swapTotal}
          parts={swapUsed !== null ? [{ value: swapUsed, className: "bg-orange/70", title: `${formatBytes(swapUsed)} swap in use` }] : []}
          note={swapUsed !== null ? `${formatBytes(swapUsed)} of ${formatBytes(swapTotal)}` : formatBytes(swapTotal)}
        />
      ) : null}
      <p className="text-[10px] text-dim">
        Installed capacity is what the firmware reports. Firmware and integrated graphics reserve part of it, so the system can use less. Live use is of the usable part only.
      </p>
    </Panel>
  );
}

// ------------------------------------------------------------------ GPU

function Block({ title, children, accent = "border-line-strong" }: { title: string; children: ReactNode; accent?: string }) {
  return (
    <div className={`min-w-[120px] flex-1 border ${accent} bg-canvas p-2`}>
      <div className="label mb-1 text-dim">{title}</div>
      {children}
    </div>
  );
}

/** Functional blocks of one GPU: PCI function, device, memory, driver. */
export function GpuDiagram({ gpu, maps, selectedId, onSelect, telemetry }: { gpu: GpuInfo; maps: IndexMaps; selectedId: string | null; onSelect: Select; telemetry: ExplorerTelemetry | null }) {
  const node = maps.byId.get(`gpu:${gpu.pci_address}`);
  const pci = node?.links.find((l) => l.relation === "pci_function");
  const memory = value(gpu.memory);
  const state = liveState(telemetry);
  const live = state === "off" ? null : telemetry?.snapshot?.gpu?.gpus.find((g) => g.pci_address === gpu.pci_address) ?? null;
  const arrow = <span className="self-center px-1 font-mono text-dim" aria-hidden="true">──</span>;
  return (
    <Panel title={`Graphics · ${gpu.pci_address}`} aside={<span className="label">Functional view · no internal units drawn</span>}>
      <div className="flex items-stretch">
        <Block title="PCI function">
          {pci ? (
            <Selectable id={pci.id} selectedId={selectedId} onSelect={onSelect} label={`PCI function ${gpu.pci_address}`} className="border border-transparent font-mono text-xs text-cyan hover:underline">
              {gpu.pci_address}
            </Selectable>
          ) : (
            <span className="font-mono text-xs text-muted">{gpu.pci_address}</span>
          )}
        </Block>
        {arrow}
        <Block title={show(gpu.kind).text === "Not available" ? "GPU" : `GPU · ${show(gpu.kind).text}`} accent="border-green/60">
          {node ? (
            <Selectable id={node.id} selectedId={selectedId} onSelect={onSelect} label={node.name} className="block w-full border border-transparent text-xs text-fg">
              {node.name}
            </Selectable>
          ) : null}
          <div className="font-mono text-[10px] text-dim">driver {value(gpu.driver) ?? "not bound"}</div>
        </Block>
        {arrow}
        <Block title="Memory">
          {memory ? (
            memory.kind === "dedicated_vram" ? (
              <p className="font-mono text-xs text-fg">{`${formatBytes(memory.bytes)} dedicated VRAM`}</p>
            ) : (
              <>
                <p className="font-mono text-xs text-fg">{`${formatBytes(memory.bytes)} reserved from system RAM`}</p>
                <p className="text-[10px] text-dim">Shared memory, not separate VRAM</p>
              </>
            )
          ) : (
            <p className="text-xs text-dim">Configuration unknown: not reported by the driver</p>
          )}
        </Block>
      </div>
      {live ? (
        <div className="mt-2 grid grid-cols-4 gap-2 font-mono text-[11px]">
          {[
            ["Busy", reading(live.busy_percent, pct)],
            ["Temp", reading(live.temperature_c, celsius)],
            ["Clock", reading(live.core_clock_mhz, (v) => `${Math.round(v)} MHz`)],
            ["Power", reading(live.power_w, watts)],
          ].map(([label, r]) => (
            <div key={label as string} className="border border-line px-2 py-1">
              <span className="label block text-dim">{label as string}</span>
              <span className={(r as { present: boolean }).present ? "text-fg" : "text-dim"}>{(r as { text: string }).text}</span>
            </div>
          ))}
        </div>
      ) : (
        <p className="mt-2 text-[10px] text-dim">{state === "off" ? "Live GPU readings appear here while monitoring is on." : "This GPU is not in the latest sample."}</p>
      )}
    </Panel>
  );
}

// --------------------------------------------------------------- storage

/** A disk's partitions as proportional segments, with what uses each. */
export function DiskLayout({ disk, maps, selectedId, onSelect, telemetry }: { disk: StorageDevice; maps: IndexMaps; selectedId: string | null; onSelect: Select; telemetry: ExplorerTelemetry | null }) {
  const capacity = value(disk.capacity_bytes);
  const state = liveState(telemetry);
  const io = state === "off" ? null : telemetry?.snapshot?.storage?.devices.find((d) => d.name === disk.name) ?? null;
  const used = (name: string) => {
    const node = maps.byId.get(`storage:${name}`);
    return node?.children.map((c) => maps.byId.get(c)?.name ?? c) ?? [];
  };
  return (
    <Panel
      title={`${disk.name} · ${value(disk.model) ?? "model not reported"}`}
      aside={<span className="label">{capacity ? formatCapacity(capacity) : "capacity unknown"}{io ? ` · read ${reading(io.read_bps, rate).text} · write ${reading(io.write_bps, rate).text}` : ""}</span>}
    >
      {disk.partitions.length && capacity ? (
        <div className="flex h-12 w-full gap-px" role="group" aria-label={`Partitions of ${disk.name}`}>
          {disk.partitions.map((p) => {
            const mounts = value(p.usage.mounts) ?? [];
            const label = mounts.length ? mounts.map((m) => m.mount_point).join(", ") : used(p.name).length ? `→ ${used(p.name).join(", ")}` : "not mounted";
            return (
              <Selectable
                key={p.name}
                id={`storage:${p.name}`}
                selectedId={selectedId}
                onSelect={onSelect}
                label={`${p.name}, ${p.size_bytes ? formatBytes(p.size_bytes) : "size unknown"}, ${label}`}
                style={{ flexGrow: Math.max(1, (p.size_bytes ?? 0) / capacity * 100), flexBasis: 0, minWidth: 44 }}
                className="overflow-hidden border border-blue/40 bg-blue/[0.06] px-1 py-0.5 hover:bg-blue/15"
              >
                <span className="block truncate font-mono text-[10px] text-fg">{p.name}</span>
                <span className="block truncate font-mono text-[9px] text-dim">{p.size_bytes ? formatBytes(p.size_bytes) : "?"}</span>
                <span className="block truncate text-[9px] text-muted">{label}</span>
              </Selectable>
            );
          })}
        </div>
      ) : (
        <p className="text-[11px] text-dim">{disk.partitions.length ? "Partition sizes are shown in the tree; the disk did not report its capacity." : "No partition table entries reported for this device."}</p>
      )}
      <p className="mt-1 text-[10px] text-dim">Widths follow reported sizes. Order is partition number, not position on the medium.</p>
    </Panel>
  );
}

// ---------------------------------------------------------------- battery

/** Design vs full-charge vs current energy, firmware values clearly marked. */
export function BatteryCapacity({ inv, telemetry }: { inv: SystemInventory; telemetry: ExplorerTelemetry | null }) {
  const state = liveState(telemetry);
  return (
    <>
      {inv.batteries.map((b) => {
        const live = state === "off" ? null : telemetry?.snapshot?.battery?.batteries.find((x) => x.name === b.name) ?? null;
        // Batteries report either energy (µWh) or charge (µAh); the bars use
        // whichever the battery reports, never a conversion between them.
        const byEnergy = value(b.energy_full_design_uwh) !== null || value(b.energy_full_uwh) !== null;
        const unit = byEnergy ? "Wh" : "Ah";
        const design = byEnergy ? value(b.energy_full_design_uwh) : value(b.charge_full_design_uah);
        const full = byEnergy ? value(b.energy_full_uwh) : value(b.charge_full_uah);
        const liveNow = live ? val(byEnergy ? live.energy_now_wh : live.charge_now_ah) : null;
        const now = liveNow !== null ? liveNow * 1e6 : null;
        const nowScan = byEnergy ? value(b.energy_now_uwh) : value(b.charge_now_uah);
        const scale = design ?? full ?? 1;
        const fmt = (v: number) => `${(v / 1e6).toFixed(byEnergy ? 1 : 2)} ${unit}`;
        return (
          <Panel key={b.name} title={`Battery capacity · ${b.name}`} aside={<span className="label">{live ? (state === "live" ? "Live charge" : "Stale") : "Scan values"}{byEnergy ? "" : " · charge (Ah)"}</span>}>
            <Bar label="Design capacity (firmware)" total={scale} parts={design ? [{ value: design, className: "bg-orange/30", title: `${fmt(design)} design` }] : []} note={design ? fmt(design) : show(byEnergy ? b.energy_full_design_uwh : b.charge_full_design_uah).text} />
            <Bar label="Full-charge capacity (firmware)" total={scale} parts={full ? [{ value: full, className: "bg-orange/55", title: `${fmt(full)} full charge` }] : []} note={full ? `${fmt(full)}${design ? ` · ${((full / design) * 100).toFixed(1)}% of design (derived)` : ""}` : show(byEnergy ? b.energy_full_uwh : b.charge_full_uah).text} />
            <Bar
              label={now !== null ? "Charge now (live)" : "Charge at scan"}
              total={scale}
              parts={now !== null ? [{ value: now, className: "bg-orange", title: `${fmt(now)} now` }] : nowScan ? [{ value: nowScan, className: "bg-orange/80", title: `${fmt(nowScan)} at scan` }] : []}
              note={now !== null ? `${fmt(now)}${live && val(live.power_w) !== null ? ` · ${reading(live.power_w, watts).text}` : ""}` : nowScan ? fmt(nowScan) : "Not reported"}
            />
            <p className="text-[10px] text-dim">
              Capacities are what the battery's controller reports. Full-charge capacity is its own estimate, not a measurement of the cells' chemical condition.
            </p>
          </Panel>
        );
      })}
    </>
  );
}

// --------------------------------------------------------------- overview

export interface OverviewTile {
  category: string;
  label: string;
  count: number;
  primary: string;
  secondary: string;
  live: string | null;
}

export function overviewTiles(inv: SystemInventory, telemetry: ExplorerTelemetry | null): OverviewTile[] {
  const state = liveState(telemetry);
  const s = state === "off" ? null : telemetry?.snapshot ?? null;
  const cpu = inv.processors[0];
  const disks = inv.storage.filter((d) => d.kind !== "virtual");
  const nets = inv.network.filter((n) => n.kind === "ethernet" || n.kind === "wireless" || n.kind === "wwan");
  const usb = inv.usb_devices.filter((d) => !d.is_root_hub);
  const bat = inv.batteries[0];
  const rx = s?.network?.interfaces.filter((i) => !i.loopback).reduce((n, i) => n + (val(i.rx_bps) ?? 0), 0);
  return [
    { category: "processor", label: "Processor", count: inv.processors.length, primary: cpu ? show(cpu.model_name).text : "Not detected", secondary: cpu ? `${value(cpu.physical_cores) ?? "?"} cores · ${value(cpu.logical_processors) ?? "?"} logical · ${cpu.caches.length} cache levels` : "", live: s?.cpu ? `${reading(s.cpu.total_percent, pct).text} busy` : null },
    { category: "memory", label: "Memory", count: inv.memory ? 1 : 0, primary: inv.memory ? (value(inv.memory.installed_bytes) !== null ? `${formatBytes(value(inv.memory.installed_bytes)!)} installed` : value(inv.memory.usable_bytes) !== null ? `${formatBytes(value(inv.memory.usable_bytes)!)} usable` : "Size not available") : "Not available", secondary: inv.memory ? `${(value(inv.memory.modules) ?? []).length || "No"} firmware memory records · ${show(inv.memory.memory_type).text}` : "", live: s?.memory ? `${reading(s.memory.used_percent, pct).text} in use` : null },
    { category: "graphics", label: "Graphics", count: inv.graphics.length, primary: inv.graphics[0] ? show(inv.graphics[0].model_name).text : "No PCI graphics device", secondary: inv.graphics.map((g) => show(g.kind).text).join(" · "), live: s?.gpu?.gpus[0] ? `${reading(s.gpu.gpus[0].busy_percent, pct).text} busy` : null },
    { category: "storage", label: "Storage", count: disks.length, primary: disks[0] ? show(disks[0].model).text : "No drive detected", secondary: `${disks.reduce((n, d) => n + d.partitions.length, 0)} partitions · ${inv.storage.length - disks.length} virtual devices`, live: s?.storage ? `${s.storage.filesystems.length} filesystems measured` : null },
    { category: "battery", label: "Battery", count: inv.batteries.length, primary: bat ? `${bat.name} · ${show(bat.technology).text}` : "No battery", secondary: bat ? `Health ${show(bat.health_percent, (h) => `${h.toFixed(0)}% of design (derived)`).text}` : "Normal for desktop computers", live: s?.battery?.batteries[0] ? `${reading(s.battery.batteries[0].percent, (v) => `${Math.round(v)}%`).text} · ${reading(s.battery.batteries[0].state, (x) => x.replace(/_/g, " ")).text}` : null },
    { category: "network", label: "Network", count: nets.length, primary: nets.map((n) => n.name).join(", ") || "No physical adapter", secondary: `${inv.network.length - nets.length} software interfaces`, live: rx !== undefined ? `${rate(rx)} received` : null },
    { category: "usb", label: "USB", count: usb.length, primary: `${usb.length} device${usb.length === 1 ? "" : "s"}`, secondary: `${inv.usb_devices.filter((d) => d.is_root_hub).length} root hubs`, live: null },
    { category: "pci", label: "PCI", count: inv.pci_devices.length, primary: `${inv.pci_devices.length} functions`, secondary: `${inv.pci_devices.filter((d) => d.role === "bridge").length} bridges`, live: null },
    { category: "motherboard", label: "Motherboard", count: inv.motherboard ? 1 : 0, primary: inv.motherboard ? show(inv.motherboard.board_name).text : "Not identified", secondary: inv.motherboard ? show(inv.motherboard.board_manufacturer).text : "", live: null },
  ];
}

export function ExplorerOverview({ inv, telemetry, onOpen }: { inv: SystemInventory; telemetry: ExplorerTelemetry | null; onOpen: (category: string) => void }) {
  const state = liveState(telemetry);
  return (
    <Panel title="Hardware overview" aside={<span className="label">{state === "live" ? "Live values from monitoring" : state === "stale" ? "Live values are stale" : "Monitoring off · scan values only"}</span>}>
      <div className="grid grid-cols-3 gap-2">
        {overviewTiles(inv, telemetry).map((t) => (
          <button key={t.category} type="button" onClick={() => onOpen(t.category)} className="border border-line bg-canvas p-2 text-left hover:border-line-strong focus-visible:ring-1 focus-visible:ring-cyan">
            <div className="flex justify-between">
              <span className="label text-dim">{t.label}</span>
              <span className="font-mono text-[11px] text-dim">{t.count}</span>
            </div>
            <div className="mt-1 truncate text-xs text-fg">{t.primary}</div>
            <div className="truncate text-[10px] text-dim">{t.secondary}</div>
            {t.live ? <div className={`mt-1 font-mono text-[11px] ${state === "live" ? "text-blue" : "text-muted"}`}>{t.live}</div> : null}
          </button>
        ))}
      </div>
    </Panel>
  );
}
