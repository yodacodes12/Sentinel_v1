import { OUTCOME_TEXT, formatScanTime, isScanning, originLabel } from "../app/inventoryView";
import { MODULE_LABEL } from "../app/catalog";
import type { DiscoveryJob, InventoryOrigin, JobProgress, ModuleOutcome, ViewSource } from "../types/inventory";
import { StatusDot, Tag } from "./primitives";

const OUTCOME_TONE: Record<ModuleOutcome, "ok" | "warn" | "idle" | "error"> = {
  completed: "ok",
  partial: "warn",
  unavailable: "idle",
  failed: "error",
  cancelled: "idle",
};

/** Origin of the data shown: scanned this session, or cached. */
export function OriginBadge({ origin, source, scannedAt }: { origin: InventoryOrigin | null; source: ViewSource; scannedAt: number | null }) {
  const label = originLabel(origin, source, scannedAt);
  return (
    <span title={label.detail}>
      <Tag kind={label.cached ? "development" : "stored"}>{label.label}</Tag>
    </span>
  );
}

/** Scan controls and per-module progress. Never blocks the rest of the UI. */
export function DiscoveryPanel({
  job,
  progress,
  serviceRunning,
  hasResult,
  onScan,
  onCancel,
  compact = false,
}: {
  compact?: boolean;
  job: DiscoveryJob;
  progress: JobProgress | null;
  serviceRunning: boolean;
  hasResult: boolean;
  onScan: (force: boolean) => void;
  onCancel: () => void;
}) {
  const scanning = isScanning(progress);
  const noun = job === "hardware" ? "hardware" : "utilities";
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <div className="flex flex-col">
          <StatusDot
            tone={scanning ? "warn" : progress?.state === "completed" ? "ok" : progress?.error ? "error" : "idle"}
            label={
              scanning
                ? progress?.current_module
                  ? `Scanning: ${MODULE_LABEL[progress.current_module]}`
                  : "Scan queued"
                : progress?.state === "completed"
                  ? "Last scan completed"
                  : progress?.state === "cancelled"
                    ? "Last scan cancelled"
                    : progress?.state === "timed_out"
                      ? "Last scan timed out"
                      : hasResult
                        ? "Showing cached result"
                        : "Not scanned yet"
            }
          />
          <span className="font-mono text-[10px] text-dim">
            {progress?.last_successful_scan ? `Last successful scan ${formatScanTime(progress.last_successful_scan)}` : "No successful scan yet"}
          </span>
        </div>
        <div className="flex gap-1.5">
          {scanning ? (
            <button type="button" onClick={onCancel} className="border border-line-strong px-2 py-1 font-mono text-[11px] uppercase tracking-wider text-muted hover:bg-raised hover:text-fg">
              Cancel
            </button>
          ) : (
            <button
              type="button"
              disabled={!serviceRunning}
              title={serviceRunning ? undefined : "Start the Sentinel service to scan"}
              onClick={() => onScan(true)}
              className="border border-cyan/60 px-2 py-1 font-mono text-[11px] uppercase tracking-wider text-cyan hover:bg-cyan/10 disabled:border-line-strong disabled:text-dim"
            >
              {hasResult ? `Rescan ${noun}` : `Scan ${noun}`}
            </button>
          )}
        </div>
      </div>
      {progress?.error && !scanning ? <p className="text-[11px] text-orange">{progress.error}</p> : null}
      {progress && progress.cache_saved === false ? (
        <p className="text-[11px] text-orange">The result could not be saved to the local cache.</p>
      ) : null}
      {compact && progress && progress.modules.length > 0 ? (
        <p className="font-mono text-[10px] text-dim">
          {(["completed", "partial", "unavailable", "failed", "cancelled"] as ModuleOutcome[])
            .map((o) => [o, progress.modules.filter((m) => m.outcome === o).length] as const)
            .filter(([, n]) => n > 0)
            .map(([o, n]) => `${n} ${OUTCOME_TEXT[o].toLowerCase()}`)
            .join(" · ")}
        </p>
      ) : null}
      {!compact && progress && progress.modules.length > 0 ? (
        <ul>
          {progress.modules.map((report) => (
            <li key={report.module} className="flex items-center justify-between border-b border-line py-1 text-[11px]" title={report.notes.join("\n")}>
              <span className="text-muted">{MODULE_LABEL[report.module]}</span>
              <span className="flex items-center gap-2">
                <span className="font-mono text-dim">{report.items}</span>
                <StatusDot tone={OUTCOME_TONE[report.outcome]} label={OUTCOME_TEXT[report.outcome]} />
              </span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
