import { useEffect, useMemo, useState } from "react";

import { formatBytes } from "../../app/inventoryView";
import {
  celsius,
  clock,
  mhz,
  pct,
  pct0,
  reading,
  runtimeText,
  val,
  watts,
} from "../../app/telemetryView";
import { Chart, WindowPicker } from "../../components/Chart";
import { CpuTopology } from "../../components/CpuTopology";
import { EmptyState, Panel } from "../../components/primitives";
import { useSeries } from "../../hooks/useTelemetry";
import { backend } from "../../services/bridge";
import type { ProcessorInfo } from "../../types/inventory";
import type { ProcessList, ProcessSample, Reading, TelemetrySnapshot, TimeWindow } from "../../types/telemetry";

export interface TabProps {
  snapshot: TelemetrySnapshot;
  live: boolean;
  refreshMs: number;
}

/** A labelled value; missing readings explain themselves. */
export function Metric<T>({ label, r, render, big }: { label: string; r: Reading<T> | null | undefined; render?: (v: T) => string; big?: boolean }) {
  const shown = reading(r, render);
  return (
    <div className="flex min-w-0 flex-col">
      <span className="label">{label}</span>
      <span className={`selectable truncate font-mono ${big ? "text-xl" : "text-sm"} ${shown.present ? "text-fg" : "text-dim"}`} title={shown.text}>
        {shown.text}
      </span>
      {shown.estimated ? <span className="text-[10px] text-orange">estimate</span> : null}
    </div>
  );
}

export function ChartPanel({ title, keys, labels, live, refreshMs, height }: { title: string; keys: string[]; labels?: Record<string, string>; live: boolean; refreshMs: number; height?: number }) {
  const [window, setWindow] = useState<TimeWindow>("minutes5");
  const series = useSeries(keys, window, live, window === "minutes5" || window === "minutes15" ? refreshMs : 30_000);
  return (
    <Panel title={title} aside={<WindowPicker value={window} onChange={setWindow} />}>
      <Chart series={series} window={window} labels={labels} height={height} />
    </Panel>
  );
}

// ------------------------------------------------------------------ CPU

export function CpuTab({ snapshot, live, refreshMs, processor }: TabProps & { processor: ProcessorInfo | null }) {
  const [core, setCore] = useState<number | null>(null);
  const cpu = snapshot.cpu;
  const load = snapshot.load;
  const usage = useMemo(() => new Map(cpu?.cores.map((c) => [c.cpu, val(c.utilisation_percent)]) ?? []), [cpu]);
  const selected = processor?.cores.find((c) => c.index === core) ?? null;
  const cpuTemps = snapshot.thermal?.temperatures.filter((t) => t.association.component === "cpu") ?? [];
  return (
    <div className="grid grid-cols-12 gap-3">
      <Panel title={`Processor · ${processor?.model_name.value ?? "model not scanned"}`} className="col-span-12" bodyClassName="grid grid-cols-6 gap-4 p-3">
        <Metric label="Total utilisation" r={cpu?.total_percent} render={pct} big />
        <Metric label="User / system" r={cpu?.times.user_percent} render={(u) => `${u.toFixed(1)}% / ${val(cpu?.times.system_percent)?.toFixed(1) ?? "?"}%`} />
        <Metric label="I/O wait" r={cpu?.times.iowait_percent} render={pct} />
        <Metric label="Average frequency" r={cpu?.average_frequency_khz} render={mhz} />
        <Metric label="Load average (1/5/15 min)" r={load?.load1} render={(l) => `${l.toFixed(2)} / ${val(load?.load5)?.toFixed(2) ?? "?"} / ${val(load?.load15)?.toFixed(2) ?? "?"}`} />
        <Metric label="CPU stall time (10 s)" r={load?.cpu_pressure.some_avg10} render={pct} />
      </Panel>
      <Panel title="Core topology · live utilisation" className="col-span-8 h-[330px]" bodyClassName="p-2" aside={<span className="label">Logical diagram, not a die image</span>}>
        {processor ? (
          <CpuTopology processor={processor} selectedCore={core} onSelect={setCore} usage={usage} />
        ) : (
          <div className="grid grid-cols-8 gap-2 p-2">
            {cpu?.cores.map((c) => (
              <div key={c.cpu} className="border border-line p-2 font-mono text-[11px]">
                CPU {c.cpu}: {reading(c.utilisation_percent, pct).text}
              </div>
            ))}
          </div>
        )}
      </Panel>
      <Panel title={selected ? `Core ${selected.index}` : "Selected core"} className="col-span-4 h-[330px]">
        {selected ? (
          <div className="flex flex-col gap-2">
            {selected.logical_cpus.map((n) => {
              const c = cpu?.cores.find((x) => x.cpu === n);
              return (
                <div key={n} className="grid grid-cols-3 gap-2 border-b border-line pb-1">
                  <span className="font-mono text-xs text-muted">CPU {n}</span>
                  <Metric label="Utilisation" r={c?.utilisation_percent} render={pct} />
                  <Metric label="Frequency" r={c?.frequency_khz} render={mhz} />
                </div>
              );
            })}
            <p className="text-[10px] text-dim">Kernel core ID {selected.core_id}{selected.kind !== "uniform" ? ` · ${selected.kind} core` : ""}.</p>
          </div>
        ) : (
          <EmptyState title="Select a core">Its logical CPUs, live utilisation and frequency appear here.</EmptyState>
        )}
      </Panel>
      <div className="col-span-8">
        <ChartPanel title="Utilisation history" keys={["cpu.total", "cpu.iowait"]} labels={{ "cpu.total": "total", "cpu.iowait": "I/O wait" }} live={live} refreshMs={refreshMs} />
      </div>
      <Panel title="Processor temperatures" className="col-span-4">
        {cpuTemps.length ? (
          cpuTemps.map((t) => (
            <div key={t.id} className="flex justify-between border-b border-line py-1 text-xs">
              <span className="text-muted">{`${t.chip} ${t.label ?? ""}`}</span>
              <span className="font-mono">{reading(t.value_c, celsius).text}</span>
              <span className="text-[10px] text-dim">{t.association.confidence} confidence</span>
            </div>
          ))
        ) : (
          <EmptyState title="No sensor is reliably the processor's" />
        )}
      </Panel>
      <div className="col-span-12">
        <ChartPanel title="Per-core history" keys={(cpu?.cores ?? []).slice(0, 16).map((c) => `cpu.core.${c.cpu}`)} live={live} refreshMs={refreshMs} height={150} />
      </div>
    </div>
  );
}

// --------------------------------------------------------------- memory

export function MemoryTab({ snapshot, live, refreshMs }: TabProps) {
  const m = snapshot.memory;
  const b = (r: Reading<number> | undefined) => reading(r, formatBytes);
  return (
    <div className="grid grid-cols-12 gap-3">
      <Panel title="Physical memory (RAM)" className="col-span-7" bodyClassName="grid grid-cols-4 gap-4 p-3">
        <Metric label="Used" r={m?.used_percent} render={pct} big />
        <Metric label="Used (bytes)" r={m?.used_bytes} render={formatBytes} />
        <Metric label="Available" r={m?.available_bytes} render={formatBytes} />
        <Metric label="Total usable" r={m?.total_bytes} render={formatBytes} />
        <Metric label="Cache" r={m?.cached_bytes} render={formatBytes} />
        <Metric label="Buffers" r={m?.buffers_bytes} render={formatBytes} />
        <Metric label="Free (unused)" r={m?.free_bytes} render={formatBytes} />
        <Metric label="Stall time (10 s)" r={m?.pressure.some_avg10} render={pct} />
        <p className="col-span-4 text-[10px] text-dim">
          Used = total − available. Cache is reclaimable and is not counted as used. Available ({b(m?.available_bytes).text}) is what applications can still get without swapping.
        </p>
      </Panel>
      <Panel title="Swap (disk-backed virtual memory)" className="col-span-5" bodyClassName="grid grid-cols-2 gap-4 p-3">
        <Metric label="Used" r={m?.swap_used_bytes} render={formatBytes} />
        <Metric label="Total" r={m?.swap_total_bytes} render={formatBytes} />
        <Metric label="Swapped in" r={m?.swap_in_pages_per_s} render={(v) => `${v.toFixed(0)} pages/s`} />
        <Metric label="Swapped out" r={m?.swap_out_pages_per_s} render={(v) => `${v.toFixed(0)} pages/s`} />
      </Panel>
      <div className="col-span-7">
        <ChartPanel title="Memory history" keys={["mem.used_percent"]} labels={{ "mem.used_percent": "used %" }} live={live} refreshMs={refreshMs} />
      </div>
      <Panel title="Top memory users" className="col-span-5">
        <ProcessTable processes={[...snapshot.top_processes].sort((a, z) => z.rss_bytes - a.rss_bytes).slice(0, 6)} compact />
      </Panel>
    </div>
  );
}

// ------------------------------------------------------------ processes

const STATE_FILTERS = ["all", "running", "sleeping"] as const;

export function ProcessTable({ processes, compact, onSelect, selected }: { processes: ProcessSample[]; compact?: boolean; onSelect?: (p: ProcessSample) => void; selected?: number | null }) {
  return (
    <table className="w-full text-left text-[11px]">
      <thead>
        <tr className="border-b border-line">
          {["PID", "Name", "CPU", "Memory", ...(compact ? [] : ["Mem %", "State", "User", "Threads"])].map((h) => (
            <th key={h} className="label px-2 py-1 font-normal">{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {processes.map((p) => (
          <tr key={p.pid} onClick={() => onSelect?.(p)} className={`border-b border-line ${onSelect ? "cursor-pointer hover:bg-raised" : ""} ${selected === p.pid ? "bg-cyan/10" : ""}`}>
            <td className="px-2 py-1 font-mono text-dim">{p.pid}</td>
            <td className="selectable max-w-[180px] truncate px-2 py-1 text-fg" title={p.command ?? p.name}>{p.name}</td>
            <td className="px-2 py-1 font-mono">{reading(p.cpu_percent, pct).text}</td>
            <td className="px-2 py-1 font-mono">{formatBytes(p.rss_bytes)}</td>
            {compact ? null : (
              <>
                <td className="px-2 py-1 font-mono">{reading(p.memory_percent, pct).text}</td>
                <td className="px-2 py-1 text-muted">{p.state.replace("_", " ")}</td>
                <td className="px-2 py-1 text-muted">{p.user ?? (p.uid !== null ? `uid ${p.uid}` : "not readable")}</td>
                <td className="px-2 py-1 font-mono text-muted">{p.threads}</td>
              </>
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function sortProcesses(list: ProcessSample[], by: "cpu" | "memory", search: string, state: (typeof STATE_FILTERS)[number]): ProcessSample[] {
  const needle = search.trim().toLowerCase();
  return list
    .filter((p) => (state === "all" ? true : state === "running" ? p.state === "running" : p.state === "sleeping" || p.state === "idle"))
    .filter((p) => !needle || p.name.toLowerCase().includes(needle) || (p.command ?? "").toLowerCase().includes(needle) || String(p.pid) === needle)
    .sort((a, b) => (by === "cpu" ? (val(b.cpu_percent) ?? -1) - (val(a.cpu_percent) ?? -1) : b.rss_bytes - a.rss_bytes));
}

export function ProcessesTab({ live, refreshMs }: TabProps) {
  const [list, setList] = useState<ProcessList | null>(null);
  const [by, setBy] = useState<"cpu" | "memory">("cpu");
  const [search, setSearch] = useState("");
  const [state, setState] = useState<(typeof STATE_FILTERS)[number]>("all");
  const [selected, setSelected] = useState<ProcessSample | null>(null);
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const answer = await backend.processes();
        if (!cancelled) setList(answer.processes);
      } catch {
        if (!cancelled) setList(null);
      }
    };
    void load();
    if (!live) return () => void (cancelled = true);
    const timer = globalThis.setInterval(() => void load(), Math.max(2_000, refreshMs));
    return () => {
      cancelled = true;
      globalThis.clearInterval(timer);
    };
  }, [live, refreshMs]);
  const shown = list ? sortProcesses(list.processes, by, search, state).slice(0, 200) : [];
  const current = selected ? list?.processes.find((p) => p.pid === selected.pid) ?? null : null;
  return (
    <div className="grid grid-cols-12 gap-3">
      <Panel
        title={list ? `Processes · ${list.total} (${list.inaccessible} not readable by you)` : "Processes"}
        className="col-span-8"
        bodyClassName="max-h-[640px] overflow-auto p-0"
        aside={<span className="label">Read-only · no process can be stopped or changed here</span>}
      >
        <div className="flex gap-2 border-b border-line p-2">
          <input type="search" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search name or PID" aria-label="Search processes" className="w-56 border border-line-strong bg-canvas px-2 py-1 font-mono text-xs text-fg placeholder:text-dim" />
          {(["cpu", "memory"] as const).map((k) => (
            <button key={k} type="button" aria-pressed={by === k} onClick={() => setBy(k)} className={`border px-2 font-mono text-[11px] uppercase ${by === k ? "border-cyan text-cyan" : "border-line-strong text-muted"}`}>
              Sort by {k}
            </button>
          ))}
          {STATE_FILTERS.map((s) => (
            <button key={s} type="button" aria-pressed={state === s} onClick={() => setState(s)} className={`border px-2 font-mono text-[11px] uppercase ${state === s ? "border-cyan text-cyan" : "border-line-strong text-muted"}`}>
              {s}
            </button>
          ))}
        </div>
        {list ? <ProcessTable processes={shown} onSelect={setSelected} selected={selected?.pid ?? null} /> : <EmptyState title="No process data">Turn monitoring on to see processes.</EmptyState>}
      </Panel>
      <div className="col-span-4 flex flex-col gap-3">
        <Panel title={current ? `${current.name} · PID ${current.pid}` : "Process details"}>
          {current ? (
            <div className="grid grid-cols-2 gap-3">
              <Metric label="CPU (share of one core)" r={current.cpu_percent} render={pct} />
              <Metric label="Memory (resident)" r={{ value: current.rss_bytes, state: "ok", quality: "direct" }} render={formatBytes} />
              <Metric label="Virtual memory" r={{ value: current.virtual_bytes, state: "ok", quality: "direct" }} render={formatBytes} />
              <Metric label="Threads" r={{ value: current.threads, state: "ok", quality: "direct" }} />
              <div className="col-span-2 text-[11px] text-muted">
                Program: <span className="font-mono">{current.command ?? "not readable"}</span> · user {current.user ?? "not readable"} · started {current.started_at ? new Date(current.started_at * 1000).toLocaleString() : "unknown"}
              </div>
              <p className="col-span-2 text-[10px] text-dim">Arguments and environment are never read. Sentinel cannot stop, pause or reprioritise processes.</p>
            </div>
          ) : (
            <EmptyState title={selected ? "The process has ended" : "Select a process"} />
          )}
        </Panel>
        {current ? (
          <ChartPanel title="CPU and memory of this process" keys={[`proc.${current.pid}.cpu`]} labels={{ [`proc.${current.pid}.cpu`]: "CPU %" }} live={live} refreshMs={refreshMs} />
        ) : null}
        {current ? <p className="text-[10px] text-dim">History is kept in memory only for the busiest processes, and only while monitoring runs.</p> : null}
      </div>
    </div>
  );
}

// -------------------------------------------------------------- battery

export function BatteryTab({ snapshot, live, refreshMs }: TabProps) {
  const report = snapshot.battery;
  if (!report || report.batteries.length === 0) {
    return (
      <Panel title="Battery">
        <EmptyState title="No battery">This computer reports no battery. Power sources: {report?.sources.map((s) => s.name).join(", ") || "none"}.</EmptyState>
      </Panel>
    );
  }
  return (
    <div className="grid grid-cols-12 gap-3">
      {report.batteries.map((b) => {
        const runtime = runtimeText(b.runtime);
        const percent = val(b.percent);
        return (
          <div key={b.name} className="col-span-12 grid grid-cols-12 gap-3">
            <Panel title={`Battery ${b.name}${b.model ? ` · ${b.model}` : ""}`} className="col-span-4" bodyClassName="flex items-center gap-4 p-3">
              <svg viewBox="0 0 60 120" className="h-32 w-16" role="img" aria-label={`Battery at ${reading(b.percent, pct0).text}`}>
                <rect x="20" y="2" width="20" height="8" fill="#2c3541" />
                <rect x="4" y="10" width="52" height="106" fill="none" stroke="#2c3541" strokeWidth="2" />
                {percent !== null ? (
                  <rect x="8" y={14 + (98 * (100 - percent)) / 100} width="44" height={(98 * percent) / 100} fill={percent <= 15 ? "#ff5d5d" : percent <= 30 ? "#ff9f43" : "#3ddc97"} />
                ) : null}
              </svg>
              <div className="flex flex-col gap-2">
                <Metric label="Charge" r={b.percent} render={pct0} big />
                <Metric label="State" r={b.state} render={(s) => s.replace("_", " ")} />
                <div>
                  <span className="label">Remaining time</span>
                  <div className="font-mono text-sm text-fg">{runtime.text}</div>
                  <div className="text-[10px] text-dim">{runtime.detail}</div>
                </div>
              </div>
            </Panel>
            <Panel title="Details" className="col-span-8" bodyClassName="grid grid-cols-4 gap-4 p-3">
              <Metric label="Power draw" r={b.power_w} render={watts} />
              <Metric label="Health" r={b.health_percent} render={pct0} />
              <Metric label="Temperature" r={b.temperature_c} render={celsius} />
              <Metric label="Cycle count" r={b.cycle_count} />
              <Metric label="Design capacity" r={b.energy_design_wh.state === "ok" ? b.energy_design_wh : b.charge_design_ah} render={(v) => (b.energy_design_wh.state === "ok" ? `${v.toFixed(1)} Wh` : `${v.toFixed(2)} Ah`)} />
              <Metric label="Full-charge capacity" r={b.energy_full_wh.state === "ok" ? b.energy_full_wh : b.charge_full_ah} render={(v) => (b.energy_full_wh.state === "ok" ? `${v.toFixed(1)} Wh` : `${v.toFixed(2)} Ah`)} />
              <Metric label="Current energy" r={b.energy_now_wh.state === "ok" ? b.energy_now_wh : b.charge_now_ah} render={(v) => (b.energy_now_wh.state === "ok" ? `${v.toFixed(1)} Wh` : `${v.toFixed(2)} Ah`)} />
              <Metric label="On mains power" r={report.on_ac} render={(v) => (v ? "Yes" : "No")} />
              <p className="col-span-4 text-[10px] text-dim">
                Health is full-charge capacity divided by design capacity as the battery reports them: an estimate, not a laboratory test. Temperature is shown only from the battery's own sensor.
              </p>
            </Panel>
            <div className="col-span-6">
              <ChartPanel title="Charge history" keys={[`battery.${b.name}.percent`]} labels={{ [`battery.${b.name}.percent`]: "charge %" }} live={live} refreshMs={refreshMs} />
            </div>
            <div className="col-span-6">
              <ChartPanel title="Power draw history" keys={[`battery.${b.name}.power`]} labels={{ [`battery.${b.name}.power`]: "W" }} live={live} refreshMs={refreshMs} />
            </div>
          </div>
        );
      })}
      <p className="col-span-12 text-[10px] text-dim">Battery values are measured {report.measured_at ? `at ${clock(report.measured_at)}` : ""}, every {report.interval_ms / 1000} s.</p>
    </div>
  );
}

