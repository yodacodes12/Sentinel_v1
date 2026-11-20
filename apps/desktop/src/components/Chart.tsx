import { ACCENT_HEX, type Accent } from "../app/catalog";
import { formatValue, linePath, niceMax } from "../app/telemetryView";
import type { Series, TimeWindow } from "../types/telemetry";
import { EmptyState } from "./primitives";

export const WINDOWS: readonly { value: TimeWindow; label: string; ms: number }[] = [
  { value: "minutes5", label: "5 min", ms: 300_000 },
  { value: "minutes15", label: "15 min", ms: 900_000 },
  { value: "hour1", label: "1 h", ms: 3_600_000 },
  { value: "hours24", label: "24 h", ms: 86_400_000 },
  { value: "days7", label: "7 d", ms: 7 * 86_400_000 },
  { value: "days30", label: "30 d", ms: 30 * 86_400_000 },
];

const PALETTE: Accent[] = ["cyan", "purple", "orange", "green", "blue"];

/**
 * A time-series chart drawn from real measurements only. Gaps (no value,
 * or more than three sample intervals without one) are left empty.
 */
export function Chart({
  series,
  window,
  height = 120,
  labels,
  now = Date.now(),
}: {
  series: Series[];
  window: TimeWindow;
  height?: number;
  labels?: Record<string, string>;
  now?: number;
}) {
  const span = WINDOWS.find((w) => w.value === window)?.ms ?? 300_000;
  const withData = series.filter((s) => s.points.some((p) => p.v !== null));
  if (withData.length === 0) {
    return (
      <div style={{ height }} className="border border-line bg-canvas">
        <EmptyState title="No data">Nothing was measured in this period.</EmptyState>
      </div>
    );
  }
  const unit = withData[0]!.unit;
  const width = 600;
  const yMax = niceMax(withData.map((s) => s.points), unit);
  const geometry = { width, height, tMin: now - span, tMax: now, yMin: 0, yMax };
  const resolutionGap = withData[0]!.resolution === "hourly" ? 3 * 3_600_000 : withData[0]!.resolution === "detailed" ? 90_000 : 0;
  const rawInterval = (() => {
    const pts = withData[0]!.points.filter((p) => p.v !== null);
    if (pts.length < 2) return 10_000;
    return Math.max(1_000, (pts[pts.length - 1]!.t - pts[0]!.t) / (pts.length - 1));
  })();
  const maxGap = resolutionGap || rawInterval * 3;
  return (
    <figure className="m-0">
      <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="block w-full border border-line bg-canvas" style={{ height }} role="img" aria-label={`Chart of ${withData.map((s) => labels?.[s.key] ?? s.key).join(", ")}`}>
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1="0" x2={width} y1={height * f} y2={height * f} stroke="var(--line)" strokeDasharray="2 4" />
        ))}
        {withData.map((s, i) => (
          <path
            key={s.key}
            d={linePath(s.points, geometry, maxGap)}
            fill="none"
            stroke={ACCENT_HEX[PALETTE[i % PALETTE.length]!]}
            strokeWidth="1.5"
            vectorEffect="non-scaling-stroke"
          />
        ))}
      </svg>
      <figcaption className="mt-1 flex flex-wrap items-center gap-x-3 font-mono text-[10px] text-dim">
        <span>max {formatValue(yMax, unit)}</span>
        {withData.map((s, i) => (
          <span key={s.key} className="flex items-center gap-1">
            <span className="inline-block h-1.5 w-3" style={{ background: ACCENT_HEX[PALETTE[i % PALETTE.length]!] }} />
            {labels?.[s.key] ?? s.key}
          </span>
        ))}
        <span className="ml-auto">{withData[0]!.resolution === "raw" ? "every sample" : withData[0]!.resolution === "detailed" ? "30 s averages" : "hourly averages"}</span>
      </figcaption>
    </figure>
  );
}

export function WindowPicker({ value, onChange }: { value: TimeWindow; onChange: (w: TimeWindow) => void }) {
  return (
    <div role="radiogroup" aria-label="Time window" className="flex border border-line-strong">
      {WINDOWS.map((w) => (
        <button
          key={w.value}
          type="button"
          role="radio"
          aria-checked={w.value === value}
          onClick={() => onChange(w.value)}
          className={`border-r border-line-strong px-1.5 py-0.5 font-mono text-[10px] last:border-r-0 ${w.value === value ? "bg-cyan/10 text-cyan" : "text-muted hover:text-fg"}`}
        >
          {w.label}
        </button>
      ))}
    </div>
  );
}
