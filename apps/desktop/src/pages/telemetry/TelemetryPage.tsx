import { useState } from "react";

import { clock, phaseLabel } from "../../app/telemetryView";
import { EmptyState, Panel, StatusDot } from "../../components/primitives";
import type { ProcessorInfo } from "../../types/inventory";
import type { AlertThresholdsConfig } from "../../types/sentinel";
import type { MonitoringReport, TelemetrySnapshot } from "../../types/telemetry";
import { GpuTab, NetworkTab, StorageTab, ThermalTab } from "./DeviceTabs";
import { BatteryTab, CpuTab, MemoryTab, ProcessesTab } from "./TelemetryTabs";

export const TABS = [
  { id: "cpu", label: "CPU" },
  { id: "memory", label: "Memory" },
  { id: "processes", label: "Processes" },
  { id: "battery", label: "Battery" },
  { id: "thermal", label: "Thermal" },
  { id: "gpu", label: "GPU" },
  { id: "storage", label: "Storage" },
  { id: "network", label: "Network" },
] as const;
export type TabId = (typeof TABS)[number]["id"];

export function TelemetryPage({
  report,
  snapshot,
  processor,
  thresholds,
  serviceRunning,
  onStart,
  stale = false,
  unreachable = false,
  initialTab = "cpu",
}: {
  report: MonitoringReport | null;
  snapshot: TelemetrySnapshot | null;
  processor: ProcessorInfo | null;
  thresholds: AlertThresholdsConfig | null;
  serviceRunning: boolean;
  onStart: () => void;
  /** The last request to the daemon failed: the values shown are old. */
  stale?: boolean;
  /** The daemon holds its socket but does not answer. */
  unreachable?: boolean;
  initialTab?: TabId;
}) {
  const [tab, setTab] = useState<TabId>(initialTab);
  const phase = phaseLabel(report?.phase);
  const live = phase.live && !stale;
  const refreshMs = report?.sampling.intervals_ms.find(([k]) => k === "cpu")?.[1] ?? 2_000;
  const props = { snapshot: snapshot!, live, refreshMs };
  return (
    <div className="flex min-w-[980px] flex-col gap-3">
      <div className="flex items-center justify-between border border-line bg-panel px-3 py-2">
        <div role="tablist" aria-label="Telemetry" className="flex gap-1">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              onClick={() => setTab(t.id)}
              className={`px-2.5 py-1 font-mono text-[11px] uppercase tracking-wider ${tab === t.id ? "bg-cyan/10 text-cyan" : "text-muted hover:text-fg"}`}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-3 text-xs">
          <StatusDot tone={phase.tone} label={`Monitoring ${phase.label.toLowerCase()}`} />
          {report?.last_sample_at && report.phase !== "off" ? (
            <span role="status" aria-live="off" className="font-mono text-[11px] text-dim">
              {live ? "Live · last update" : stale ? "Not live · Sentinel is not answering · last measured" : "Not live · last measured"} {clock(report.last_sample_at)}
            </span>
          ) : null}
        </div>
      </div>
      {report?.phase === "active" && !report.history_available ? (
        <div className="border border-orange/60 bg-orange/10 px-3 py-2 text-xs text-orange">
          History is unavailable: Sentinel could not open its history database (see its log). Live values are shown and alerts still work, but nothing is being recorded and charts cover only the last 15 minutes.
        </div>
      ) : null}
      {report?.phase === "paused" ? (
        <div className="border border-orange/60 bg-orange/10 px-3 py-2 text-xs text-orange">Monitoring is paused. The values below are the last ones measured before pausing; nothing is being measured now.</div>
      ) : null}
      {snapshot === null ? (
        <Panel title="Telemetry" className="h-[420px]">
          <EmptyState title={unreachable ? "Sentinel is not answering" : serviceRunning ? "Monitoring is off" : "The Sentinel service is not running"}>
            {unreachable
              ? "The service is running but did not answer in time. It may be busy; the header shows when the connection is back. Nothing is shown rather than old values."
              : serviceRunning
                ? "No live measurements are shown while monitoring is off. History collected earlier is kept."
                : "Start the service from the header to use monitoring."}
          </EmptyState>
          {serviceRunning ? (
            <div className="-mt-28 flex justify-center">
              <button type="button" onClick={onStart} className="border border-cyan/60 px-3 py-1.5 font-mono text-[11px] uppercase tracking-wider text-cyan hover:bg-cyan/10">
                Turn monitoring on
              </button>
            </div>
          ) : null}
        </Panel>
      ) : tab === "cpu" ? (
        <CpuTab {...props} processor={processor} />
      ) : tab === "memory" ? (
        <MemoryTab {...props} />
      ) : tab === "processes" ? (
        <ProcessesTab {...props} />
      ) : tab === "battery" ? (
        <BatteryTab {...props} />
      ) : tab === "thermal" ? (
        <ThermalTab {...props} thresholds={thresholds} />
      ) : tab === "gpu" ? (
        <GpuTab {...props} />
      ) : tab === "storage" ? (
        <StorageTab {...props} />
      ) : (
        <NetworkTab {...props} />
      )}
    </div>
  );
}
