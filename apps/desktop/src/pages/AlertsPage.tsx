import { useEffect, useState } from "react";

import { clock } from "../app/telemetryView";
import { EmptyState, Panel, StatusDot } from "../components/primitives";
import { backend } from "../services/bridge";
import type { Alert } from "../types/telemetry";

const SEVERITY_TONE = { info: "idle", warning: "warn", critical: "error" } as const;

function when(ms: number): string {
  return `${new Date(ms).toLocaleDateString([], { month: "short", day: "2-digit" })} ${clock(ms)}`;
}

/** An alert card. Recommendations are text: there is no action button. */
export function AlertCard({ alert, onAcknowledge }: { alert: Alert; onAcknowledge?: (id: number) => void }) {
  return (
    <article className={`border p-3 ${alert.status === "active" ? (alert.severity === "critical" ? "border-red/60" : alert.severity === "warning" ? "border-orange/60" : "border-line-strong") : "border-line"}`}>
      <header className="flex items-start justify-between gap-3">
        <div>
          <StatusDot tone={alert.status === "active" ? SEVERITY_TONE[alert.severity] : "idle"} label={`${alert.severity.toUpperCase()} · ${alert.category}`} />
          <h3 className="mt-1 text-sm text-fg">{alert.title}</h3>
        </div>
        <div className="text-right font-mono text-[10px] text-dim">
          <div>detected {when(alert.detected_at)}</div>
          <div>{alert.status === "active" ? "still active" : `resolved ${alert.resolved_at ? when(alert.resolved_at) : ""}`}</div>
          {alert.acknowledged ? <div>acknowledged</div> : null}
        </div>
      </header>
      <p className="mt-2 text-xs text-muted">{alert.explanation}</p>
      <dl className="mt-2 grid grid-cols-[110px_1fr] gap-x-2 gap-y-0.5 text-[11px]">
        <dt className="text-dim">Evidence</dt>
        <dd className="text-fg">{alert.evidence}</dd>
        <dt className="text-dim">Threshold</dt>
        <dd className="text-fg">{`${alert.threshold} (${alert.threshold_source === "user" ? "your setting" : alert.threshold_source === "driver" ? "reported by the driver" : "default"})`}</dd>
        <dt className="text-dim">Metric</dt>
        <dd className="font-mono text-muted">{`${alert.metric}${alert.subject ? ` · ${alert.subject}` : ""}`}</dd>
        {alert.recommendation ? (
          <>
            <dt className="text-dim">You may consider</dt>
            <dd className="text-fg">{alert.recommendation}</dd>
          </>
        ) : null}
        <dt className="text-dim">Notification</dt>
        <dd className="text-muted">{alert.notified ? "sent to the desktop" : "not sent (cooldown or notifications off)"}</dd>
      </dl>
      {onAcknowledge && !alert.acknowledged ? (
        <button type="button" onClick={() => onAcknowledge(alert.id)} className="mt-2 border border-line-strong px-2 py-0.5 font-mono text-[10px] uppercase text-muted hover:text-fg" title="Marks it as seen. Changes nothing on your computer and does not silence other alerts.">
          Acknowledge
        </button>
      ) : null}
    </article>
  );
}

export function AlertsPage({ active, onAcknowledge, serviceRunning }: { active: Alert[]; onAcknowledge: (id: number) => void; serviceRunning: boolean }) {
  const [history, setHistory] = useState<Alert[] | null>(null);
  const [filter, setFilter] = useState<"all" | "warning" | "critical">("all");
  useEffect(() => {
    let cancelled = false;
    backend
      .alertHistory(200)
      .then((h) => !cancelled && setHistory(h))
      .catch(() => !cancelled && setHistory([]));
    return () => {
      cancelled = true;
    };
  }, [active.length, serviceRunning]);
  const past = (history ?? []).filter((a) => a.status === "resolved").filter((a) => filter === "all" || a.severity === filter);
  return (
    <div className="grid min-w-[980px] grid-cols-12 gap-3">
      <Panel title={`Active alerts · ${active.length}`} className="col-span-6" bodyClassName="flex max-h-[760px] flex-col gap-2 overflow-auto p-3">
        {active.length === 0 ? <EmptyState title="No active alerts" /> : active.map((a) => <AlertCard key={a.id} alert={a} onAcknowledge={onAcknowledge} />)}
      </Panel>
      <Panel
        title="History"
        className="col-span-6"
        bodyClassName="flex max-h-[760px] flex-col gap-2 overflow-auto p-3"
        aside={
          <div className="flex gap-1">
            {(["all", "warning", "critical"] as const).map((f) => (
              <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)} className={`px-1.5 font-mono text-[10px] uppercase ${filter === f ? "text-cyan" : "text-muted"}`}>
                {f}
              </button>
            ))}
          </div>
        }
      >
        {history === null ? <EmptyState title="Loading" /> : past.length === 0 ? <EmptyState title="No resolved alerts recorded" /> : past.map((a) => <AlertCard key={a.id} alert={a} onAcknowledge={onAcknowledge} />)}
      </Panel>
      <p className="col-span-12 text-[10px] text-dim">
        Alerts inform; they never act. Acknowledging only marks an alert as seen. Alerts are kept locally for the configured retention period.
      </p>
    </div>
  );
}
