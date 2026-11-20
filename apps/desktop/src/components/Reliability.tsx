import { MISSING_RELIABILITY, RELIABILITY, reliabilityOf } from "../app/explorer";
import type { Availability, Confidence } from "../types/inventory";

const TONE_CLASS = {
  solid: "border-line-strong text-muted",
  derived: "border-blue/60 text-blue",
  inferred: "border-orange/60 text-orange",
  missing: "border-dashed border-line-strong text-dim",
} as const;

/**
 * Where a value came from, in Sentinel's one reliability vocabulary. The
 * tooltip explains the category and names the source.
 */
export function ReliabilityBadge({ availability, confidence, source }: { availability: Availability; confidence: Confidence | null; source?: string | null }) {
  const r = reliabilityOf(availability, confidence);
  return (
    <span
      className={`shrink-0 border px-1 py-px font-mono text-[9px] uppercase tracking-wider ${TONE_CLASS[r.tone]}`}
      title={`${r.meaning}${source ? `\nSource: ${source}` : ""}`}
    >
      {r.label}
    </span>
  );
}

/** Explains every reliability label shown in the explorer. */
export function ReliabilityLegend() {
  const entries = [...Object.values(RELIABILITY), ...Object.values(MISSING_RELIABILITY)];
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11px]">
      {entries.map((r) => (
        <div key={r.label} className="contents">
          <dt>
            <span className={`border px-1 py-px font-mono text-[9px] uppercase tracking-wider ${TONE_CLASS[r.tone]}`}>{r.label}</span>
          </dt>
          <dd className="text-muted">{r.meaning}</dd>
        </div>
      ))}
      <div className="contents">
        <dt>
          <span className="border border-blue/60 px-1 py-px font-mono text-[9px] uppercase tracking-wider text-blue">Live</span>
        </dt>
        <dd className="text-muted">A reading from the monitoring engine. Confidence is not accuracy: a reliable source can still report a rounded value.</dd>
      </div>
    </dl>
  );
}
