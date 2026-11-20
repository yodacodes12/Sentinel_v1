import { useState } from "react";

import { formatScanTime, hardwareCount, show, summaryCards, value } from "../app/inventoryView";
import { type BridgePhase, formatClock, formatUptime, serviceIndicator } from "../app/viewModel";
import { CpuTopology } from "../components/CpuTopology";
import { DiscoveryPanel, OriginBadge } from "../components/DiscoveryPanel";
import { Icon, type IconName } from "../components/Icon";
import { ACCENT_TEXT, EmptyState, Panel, Segmented, StatusDot, Switch, Tag } from "../components/primitives";
import { Chart } from "../components/Chart";
import { useSeries } from "../hooks/useTelemetry";
import { HEALTH_AREA_LABEL, HEALTH_TONE, celsius, clock, pct, phaseLabel, rate, reading, val } from "../app/telemetryView";
import type { Alert, MonitoringReport, SamplingProfile, TelemetrySnapshot } from "../types/telemetry";
import type { MonitoringCommand } from "../types/sentinel";
import type { InventoryView, JobProgress } from "../types/inventory";
import type { ConfigView, Overview, PowerMode } from "../types/sentinel";

const POWER_MODES: readonly { value: PowerMode; label: string }[] = [
  { value: "eco", label: "Eco" },
  { value: "balanced", label: "Balanced" },
  { value: "performance", label: "Performance" },
];

const CARD_ICON: Record<string, IconName> = {
  cpu: "cpu",
  memory: "memory",
  storage: "storage",
  gpu: "gpu",
  battery: "battery",
  network: "network",
};

export function OverviewPage({
  phase,
  overview,
  config,
  saving,
  onPowerMode,
  inventory,
  progress,
  onScan,
  onCancel,
  onOpenHardware,
  telemetry,
  onMonitoring,
  onProfile,
  onOpenAlerts,
}: {
  phase: BridgePhase;
  overview: Overview | null;
  config: ConfigView | null;
  saving: boolean;
  onPowerMode: (mode: PowerMode) => void;
  inventory: InventoryView | null;
  progress: JobProgress | null;
  onScan: (force: boolean) => void;
  onCancel: () => void;
  onOpenHardware: () => void;
  telemetry?: { report: MonitoringReport | null; snapshot: TelemetrySnapshot | null; alerts: Alert[]; busy: string | null; stale?: boolean };
  onMonitoring?: (command: MonitoringCommand) => void;
  onProfile?: (profile: SamplingProfile) => void;
  onOpenAlerts?: () => void;
}) {
  const report = telemetry?.report ?? null;
  const live = telemetry?.snapshot ?? null;
  const mon = phaseLabel(report?.phase);
  // Live only while the daemon keeps answering.
  const isLive = mon.live && !telemetry?.stale;
  const [selectedCore, setSelectedCore] = useState<number | null>(null);
  const daemon = phase === "ready" ? (overview?.daemon ?? null) : null;
  const service = serviceIndicator(phase, overview);
  const inv = inventory?.inventory ?? null;
  const system = inv?.system ?? null;
  const board = inv?.motherboard ?? null;
  const cpu = inv?.processors[0] ?? null;
  const core = cpu?.cores.find((c) => c.index === selectedCore) ?? null;
  const cards = summaryCards(inv);
  const deviceName = system ? [value(system.manufacturer), value(system.product_name)].filter(Boolean).join(" ") : "";

  return (
    <div className="grid min-w-[980px] grid-cols-12 gap-3">
      {inv?.root_override ? (
        <div className="col-span-12 border border-orange/60 bg-orange/10 px-3 py-2 text-xs text-orange">
          This inventory was read from a test directory ({inv.root_override}), not from this computer.
        </div>
      ) : null}

      {/* 1. System overview */}
      <Panel
        title="System overview"
        className="col-span-12"
        bodyClassName="grid grid-cols-6 divide-x divide-line"
        aside={inventory && inv ? <OriginBadge origin={inventory.origin} source={inventory.source} scannedAt={inv.discovered_at} /> : undefined}
      >
        <OverviewCell label="Device">
          <span className={`selectable truncate font-mono text-sm ${deviceName ? "text-fg" : "text-dim"}`} title={deviceName || undefined}>{deviceName || (inv ? "Not identified by firmware" : "Not scanned")}</span>
          <span className="truncate text-[11px] text-dim">{system ? show(system.os.pretty_name).text : "Run a hardware scan"}</span>
        </OverviewCell>
        <OverviewCell label="Motherboard">
          <span className={`selectable truncate font-mono text-sm ${board && value(board.board_name) ? "text-fg" : "text-dim"}`}>
            {board ? show(board.board_name).text : "—"}
          </span>
          <span className="truncate text-[11px] text-dim">{board ? `${show(board.board_manufacturer).text} · rev ${show(board.board_revision).text}` : "Not scanned"}</span>
        </OverviewCell>
        <OverviewCell label="Hardware detected">
          <span className="font-mono text-sm text-fg">{inv ? `${hardwareCount(inv)} devices` : "—"}</span>
          <span className="text-[11px] text-dim">{inv ? `Last scan ${formatScanTime(inv.discovered_at)}` : "No scan yet"}</span>
        </OverviewCell>
        <OverviewCell label="Sentinel service">
          <StatusDot tone={service.tone} label={service.label} />
          <span className="selectable font-mono text-[11px] text-muted">{daemon ? `pid ${daemon.pid}` : service.detail}</span>
        </OverviewCell>
        <OverviewCell label="Running since">
          <span className="font-mono text-sm text-fg">{daemon ? formatClock(daemon.started_at) : "—"}</span>
          <span className="font-mono text-[11px] text-muted">{daemon ? `up ${formatUptime(daemon.uptime_secs)} at last refresh` : "Service is not running"}</span>
        </OverviewCell>
        <OverviewCell label="IPC link">
          <StatusDot
            tone={overview?.ipc === "connected" ? "ok" : overview?.ipc === "error" ? "error" : "idle"}
            label={overview?.ipc === "connected" ? "Connected" : overview?.ipc === "error" ? "Error" : "Not connected"}
          />
          <span className="font-mono text-[11px] text-muted">{daemon ? `protocol v${daemon.protocol_version} · ${overview?.client_version}` : "Unix socket, this user only"}</span>
        </OverviewCell>
      </Panel>

      {/* 2. Hardware summary cards */}
      <div className="col-span-12 grid grid-cols-6 gap-3">
        {cards.map((card) => (
          <button
            type="button"
            key={card.id}
            onClick={onOpenHardware}
            className="border border-line bg-panel p-3 text-left hover:border-line-strong"
            aria-label={`${card.label}: ${card.primary.text}`}
          >
            <div className="flex items-center justify-between">
              <span className="label text-muted">{card.label}</span>
              <Icon name={CARD_ICON[card.id] ?? "cpu"} className={`${ACCENT_TEXT[card.accent]} ${card.primary.present ? "" : "opacity-50"}`} />
            </div>
            <div className={`selectable mt-2.5 truncate font-mono text-[13px] leading-tight ${card.primary.present ? "text-fg" : "text-dim"}`} title={card.primary.text}>
              {card.primary.text}
            </div>
            <div className="mt-1.5 truncate text-[11px] text-dim">{card.secondary}</div>
          </button>
        ))}
      </div>

      {/* 3. Hardware visualisation: CPU topology drawn from discovered data */}
      <Panel
        title={cpu ? `Processor topology · ${show(cpu.model_name).text}` : "Processor topology"}
        className="col-span-8 h-[440px]"
        bodyClassName="p-2"
        aside={cpu ? <span className="label">Generated from /sys/devices/system/cpu</span> : undefined}
      >
        {cpu ? (
          <CpuTopology processor={cpu} selectedCore={selectedCore} onSelect={setSelectedCore} />
        ) : (
          <EmptyState title={inv ? "No processor information" : "Not scanned yet"}>
            {inv
              ? "The processor could not be read on this system."
              : "Scan the hardware to draw this computer's real core topology. Nothing is shown until it has been read."}
          </EmptyState>
        )}
      </Panel>

      <div className="col-span-4 flex h-[440px] flex-col gap-3">
        {/* 4. Component information */}
        <Panel title="Component information" className="min-h-[190px] flex-1" bodyClassName="overflow-hidden p-3">
          {cpu && core ? (
            <div className="flex h-full flex-col">
              <h3 className="font-mono text-sm tracking-wider text-cyan">{`CORE ${core.index}`}</h3>
              <p className="mb-2 mt-1 text-[11px] text-dim">Static identity from sysfs. Live per-core activity arrives with the telemetry engine.</p>
              <Row label="Kernel core ID" value={String(core.core_id)} />
              <Row label="Logical CPUs" value={core.logical_cpus.map((c) => `CPU ${c}`).join(", ")} />
              <Row label="Core type" value={core.kind === "uniform" ? "Same as all others" : core.kind} />
              <Row label="Die" value={core.die_id === null ? "Not reported" : String(core.die_id)} />
              <Row label="Max frequency" value={core.max_frequency_khz ? `${(core.max_frequency_khz / 1e6).toFixed(2)} GHz (kernel limit)` : "Not reported"} />
            </div>
          ) : board ? (
            <div className="flex h-full flex-col">
              <p className="mb-2 text-[11px] text-dim">
                Motherboard identity detected — schematic analysis not yet available. Select a core for its details.
              </p>
              <Row label="Board" value={show(board.board_name).text} />
              <Row label="Revision" value={show(board.board_revision).text} />
              <Row label="BIOS" value={`${show(board.bios_vendor).text} ${show(board.bios_version).text}`} />
              <Row label="Identification" value="Firmware-reported, not verified" />
            </div>
          ) : (
            <EmptyState title="Nothing selected">Scan the hardware, then select a core in the topology.</EmptyState>
          )}
        </Panel>

        {/* 6. Power mode, monitoring and discovery */}
        <Panel title="Discovery, power mode and monitoring">
          <DiscoveryPanel
            compact
            job="hardware"
            progress={progress}
            serviceRunning={daemon !== null}
            hasResult={inv !== null}
            onScan={onScan}
            onCancel={onCancel}
          />
          <div className="mt-3 flex items-center justify-between border-t border-line pt-2">
            <div className="flex flex-col">
              <span className="text-xs text-fg">Monitoring</span>
              <StatusDot tone={mon.tone} label={report?.phase === "paused" ? "Paused" : mon.label} />
            </div>
            <div className="flex items-center gap-2">
              {report?.phase === "active" ? (
                <button type="button" disabled={!onMonitoring || telemetry?.busy !== null} onClick={() => onMonitoring?.("pause")} className="border border-line-strong px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted hover:text-fg">
                  Pause
                </button>
              ) : report?.phase === "paused" ? (
                <button type="button" disabled={!onMonitoring || telemetry?.busy !== null} onClick={() => onMonitoring?.("resume")} className="border border-line-strong px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted hover:text-fg">
                  Resume
                </button>
              ) : null}
              <Switch
                label={report && report.phase !== "off" ? "Turn monitoring off" : "Turn monitoring on"}
                checked={!!report && report.phase !== "off" && report.phase !== "error"}
                disabled={!onMonitoring || daemon === null || (telemetry?.busy ?? null) !== null}
                onChange={(on) => onMonitoring?.(on ? "start" : "stop")}
              />
            </div>
          </div>
          <div className="mb-1.5 mt-2 flex items-center justify-between">
            <span className="text-xs text-muted">Sampling profile</span>
            {report?.sampling.reason ? <Tag kind="development">Eco on battery</Tag> : <Tag kind="stored">Applies to monitoring</Tag>}
          </div>
          <Segmented
            label="Sampling profile"
            options={POWER_MODES}
            value={config?.config.power_mode ?? overview?.operating_mode ?? "balanced"}
            disabled={config === null || saving}
            onChange={(mode) => (onProfile ? onProfile(mode) : onPowerMode(mode))}
          />
          {report?.sampling.reason ? <p className="mt-1 text-[10px] text-orange">{report.sampling.reason}</p> : null}
        </Panel>
      </div>

      {/* 5. System telemetry: live only while monitoring is active */}
      <Panel
        title="System telemetry"
        className="col-span-9"
        aside={
          <span role="status" aria-live="off" className="label">
            {isLive && report?.last_sample_at
              ? `Live · updated ${clock(report.last_sample_at)}`
              : telemetry?.stale && report?.last_sample_at
                ? `Not live · Sentinel is not answering · last measured ${clock(report.last_sample_at)}`
                : report?.phase === "paused"
                  ? "Paused · not live"
                  : "Monitoring off · no live data"}
          </span>
        }
      >
        <LiveTiles snapshot={live} live={isLive} refreshMs={report?.sampling.intervals_ms.find(([k]) => k === "cpu")?.[1] ?? 2000} />
      </Panel>
      <Panel title={`Alerts · ${telemetry?.alerts.length ?? 0} active`} className="col-span-3" aside={onOpenAlerts ? <button type="button" onClick={onOpenAlerts} className="label hover:text-fg">All alerts</button> : undefined}>
        {(telemetry?.alerts ?? []).length === 0 ? (
          <EmptyState title={mon.live ? "No active alerts" : "Monitoring off"}>{mon.live ? "Alerts appear when a condition lasts, not for brief spikes." : "Alerts are evaluated only while monitoring runs."}</EmptyState>
        ) : (
          telemetry!.alerts.slice(0, 4).map((a) => (
            <div key={a.id} className="border-b border-line py-1.5">
              <StatusDot tone={a.severity === "critical" ? "error" : a.severity === "warning" ? "warn" : "idle"} label={a.title} />
              <div className="text-[10px] text-dim">{a.evidence}</div>
            </div>
          ))
        )}
        {live && live.health.length ? (
          <div className="mt-2 grid grid-cols-2 gap-x-2 border-t border-line pt-2">
            {live.health.map((h) => (
              <div key={h.area} className="flex items-center justify-between py-0.5 text-[10px]" title={`${h.summary}${h.evidence.length ? ` — ${h.evidence.join("; ")}` : ""}`}>
                <span className="text-muted">{HEALTH_AREA_LABEL[h.area]}</span>
                <StatusDot tone={HEALTH_TONE[h.level]} label={h.level} />
              </div>
            ))}
          </div>
        ) : null}
      </Panel>
    </div>
  );
}

function LiveTiles({ snapshot, live, refreshMs }: { snapshot: TelemetrySnapshot | null; live: boolean; refreshMs: number }) {
  const series = useSeries(["cpu.total", "mem.used_percent"], "minutes5", live, refreshMs);
  const s = snapshot;
  const hottest = s?.thermal?.temperatures.filter((t) => val(t.value_c) !== null).sort((a, b) => val(b.value_c)! - val(a.value_c)!)[0];
  const bat = s?.battery?.batteries[0];
  const disk = s?.storage?.devices[0];
  const net = s?.network?.interfaces.filter((i) => !i.loopback) ?? [];
  const sum = (key: "rx_bps" | "tx_bps") => {
    const values = net.map((i) => val(i[key]));
    return values.length && values.every((v) => v !== null) ? values.reduce((a, b) => a! + b!, 0)! : null;
  };
  const tiles: { label: string; text: string; present: boolean; note: string }[] = [
    { label: "CPU", ...reading(s?.cpu?.total_percent, pct), note: s?.load ? `load ${reading(s.load.load1, (v) => v.toFixed(2)).text}` : "" },
    { label: "Memory", ...reading(s?.memory?.used_percent, pct), note: s?.memory ? `${reading(s.memory.available_bytes, (v) => `${(v / 1073741824).toFixed(1)} GiB`).text} available` : "" },
    { label: "Temperature", ...reading(hottest?.value_c, celsius), note: hottest ? `${hottest.chip} ${hottest.label ?? ""} (hottest)` : "no readable sensor" },
    { label: "Battery", ...reading(bat?.percent, (v) => `${Math.round(v)}%`), note: bat ? reading(bat.state, (v) => v.replace("_", " ")).text : "no battery" },
    { label: "Storage I/O", ...reading(disk?.write_bps, rate), note: disk ? `${disk.name} write · read ${reading(disk.read_bps, rate).text}` : "" },
    { label: "Network", text: sum("rx_bps") !== null ? rate(sum("rx_bps")!) : "No data", present: sum("rx_bps") !== null, note: sum("tx_bps") !== null ? `down · up ${rate(sum("tx_bps")!)}` : "" },
  ];
  return (
    <div className="grid grid-cols-12 gap-3">
      <div className="col-span-7 grid grid-cols-3 gap-2">
        {tiles.map((t) => (
          <div key={t.label} className="border border-line bg-canvas p-2">
            <span className="label">{t.label}</span>
            <div className={`font-mono text-lg ${t.present && live ? "text-fg" : "text-dim"}`}>{live || t.present ? t.text : "—"}</div>
            <div className="truncate text-[10px] text-dim">{live || t.present ? t.note : "Monitoring off"}</div>
          </div>
        ))}
      </div>
      <div className="col-span-5">
        <Chart series={series} window="minutes5" height={118} labels={{ "cpu.total": "CPU %", "mem.used_percent": "memory %" }} />
      </div>
    </div>
  );
}

function OverviewCell({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1 px-3 py-2.5">
      <span className="label">{label}</span>
      {children}
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3 border-b border-line py-1.5 last:border-b-0">
      <span className="shrink-0 text-xs text-muted">{label}</span>
      <span className="selectable truncate font-mono text-xs text-fg" title={value}>
        {value}
      </span>
    </div>
  );
}
