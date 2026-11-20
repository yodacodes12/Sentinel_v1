// Pure display logic for live telemetry. Honesty rules, unit-tested:
// - a reading without a value says why; never 0;
// - estimates are labelled as estimates;
// - charts break at gaps instead of drawing a line across them;
// - nothing is called live unless monitoring is active.

import type {
  HealthLevel,
  MonitoringPhase,
  Reading,
  RuntimeEstimate,
  SeriesPoint,
} from "../types/telemetry";
import type { Tone } from "./viewModel";
import { formatBytes } from "./inventoryView";

const MISSING: Record<string, string> = {
  pending: "Measuring…",
  unknown: "Unknown",
  unsupported: "Not provided",
  unavailable: "Not available",
  permission_denied: "Administrator only",
  error: "Read error",
};

export interface ShownReading {
  text: string;
  present: boolean;
  estimated: boolean;
}

export function reading<T>(r: Reading<T> | null | undefined, render: (v: T) => string = String): ShownReading {
  if (!r) return { text: "No data", present: false, estimated: false };
  if (r.state === "ok" && r.value !== undefined && r.value !== null) {
    return { text: render(r.value), present: true, estimated: r.quality === "estimated" };
  }
  return { text: MISSING[r.state] ?? "Not available", present: false, estimated: false };
}

export function val<T>(r: Reading<T> | null | undefined): T | null {
  return r && r.state === "ok" && r.value !== undefined ? r.value : null;
}

export const pct = (v: number) => `${v.toFixed(1)}%`;
export const pct0 = (v: number) => `${Math.round(v)}%`;
export const celsius = (v: number) => `${v.toFixed(1)} °C`;
export const watts = (v: number) => `${v.toFixed(1)} W`;
export const rate = (v: number) => `${formatBytes(Math.round(v))}/s`;
export const mhz = (khz: number) => `${Math.round(khz / 1000)} MHz`;

export function duration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h > 0 ? `${h} h ${String(m).padStart(2, "0")} min` : `${m} min`;
}

export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
}

/** Battery runtime text. Never an exact countdown; always marked estimate. */
export function runtimeText(r: RuntimeEstimate): { text: string; detail: string } {
  switch (r.state) {
    case "not_discharging":
      return { text: "Not discharging", detail: "Runtime is estimated only while running on battery." };
    case "calculating":
      return { text: "Calculating remaining time...", detail: `${r.samples} of ${r.needed} readings collected.` };
    case "insufficient":
      return { text: "Insufficient battery data for a reliable estimate.", detail: r.reason };
    case "estimated": {
      // Rounded to 5 minutes, or 15 when confidence is low.
      const step = r.confidence === "low" ? 900 : 300;
      const rounded = Math.max(step, Math.round(r.seconds / step) * step);
      return {
        text: `About ${duration(rounded)} (estimate)`,
        detail: [
          `${r.confidence} confidence`,
          r.average_power_w === null ? null : `average draw ${r.average_power_w.toFixed(1)} W over ${Math.round(r.window_s / 60)} min`,
          "not a countdown",
        ]
          .filter(Boolean)
          .join(" · "),
      };
    }
  }
}

export interface PhaseLabel {
  label: string;
  tone: Tone;
  live: boolean;
}

export function phaseLabel(phase: MonitoringPhase | null | undefined): PhaseLabel {
  switch (phase) {
    case "active":
      return { label: "Active", tone: "ok", live: true };
    case "starting":
      return { label: "Starting", tone: "warn", live: false };
    case "paused":
      return { label: "Paused", tone: "warn", live: false };
    case "stopping":
      return { label: "Stopping", tone: "warn", live: false };
    case "error":
      return { label: "Error", tone: "error", live: false };
    default:
      return { label: "Off", tone: "idle", live: false };
  }
}

export const HEALTH_TONE: Record<HealthLevel, Tone> = {
  unknown: "unknown",
  normal: "ok",
  elevated: "warn",
  high: "error",
  critical: "error",
};

export const HEALTH_AREA_LABEL: Record<string, string> = {
  cpu_pressure: "CPU",
  memory_pressure: "Memory",
  swap_pressure: "Swap",
  thermal: "Thermal",
  battery: "Battery",
  storage_capacity: "Storage space",
  storage_activity: "Storage activity",
  network_activity: "Network",
  sensor_availability: "Sensors",
};

// ---------------------------------------------------------------- charts

export interface ChartGeometry {
  width: number;
  height: number;
  tMin: number;
  tMax: number;
  yMin: number;
  yMax: number;
}

/**
 * SVG path data for a series. Each run of consecutive values becomes its
 * own subpath; a gap (null value, or a jump longer than `maxGapMs`) starts
 * a new one, so missing data is never bridged by a line.
 */
export function linePath(points: SeriesPoint[], g: ChartGeometry, maxGapMs: number): string {
  const x = (t: number) => ((t - g.tMin) / Math.max(1, g.tMax - g.tMin)) * g.width;
  const y = (v: number) => g.height - ((v - g.yMin) / Math.max(1e-9, g.yMax - g.yMin)) * g.height;
  let d = "";
  let prev: SeriesPoint | null = null;
  for (const p of points) {
    if (p.v === null || p.t < g.tMin) {
      prev = null;
      continue;
    }
    const start = prev === null || p.t - prev.t > maxGapMs;
    d += `${start ? "M" : "L"}${x(p.t).toFixed(1)} ${y(Math.min(g.yMax, Math.max(g.yMin, p.v))).toFixed(1)}`;
    prev = p;
  }
  return d;
}

/** A y-axis maximum: fixed for percentages, otherwise rounded above the data. */
export function niceMax(points: SeriesPoint[][], unit: string): number {
  if (unit === "%") return 100;
  const max = Math.max(0, ...points.flat().map((p) => p.max ?? p.v ?? 0));
  if (max <= 0) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(max));
  for (const step of [1, 2, 2.5, 5, 10]) {
    if (max <= step * magnitude) return step * magnitude;
  }
  return 10 * magnitude;
}

export function formatValue(v: number, unit: string): string {
  switch (unit) {
    case "%":
      return pct0(v);
    case "B":
      return formatBytes(Math.round(v));
    case "B/s":
      return rate(v);
    case "°C":
      return `${Math.round(v)} °C`;
    case "W":
      return watts(v);
    case "kHz":
      return mhz(v);
    default:
      return `${Number.isInteger(v) ? v : v.toFixed(1)}${unit ? ` ${unit}` : ""}`;
  }
}
