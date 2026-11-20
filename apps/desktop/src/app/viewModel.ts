// Pure functions that turn backend state into what the UI says.
// Kept free of React so the honesty rules below are unit-tested.

import type { Overview } from "../types/sentinel";

export type Tone = "ok" | "idle" | "warn" | "error" | "unknown";

export interface Indicator {
  label: string;
  tone: Tone;
  detail: string;
}

export type BridgePhase = "loading" | "ready" | "unavailable";

/** State of the Sentinel service (the daemon process). */
export function serviceIndicator(phase: BridgePhase, overview: Overview | null): Indicator {
  if (phase === "unavailable") {
    return { label: "Unknown", tone: "unknown", detail: "Not running inside the desktop app." };
  }
  if (phase === "loading" || overview === null) {
    return { label: "Checking", tone: "unknown", detail: "Asking the daemon for its status." };
  }
  if (overview.ipc === "error") {
    return {
      label: "Unreachable",
      tone: "error",
      detail: overview.ipc_detail ?? "The daemon could not be reached.",
    };
  }
  if (overview.daemon === null) {
    return { label: "Stopped", tone: "idle", detail: "The Sentinel daemon is not running." };
  }
  if (overview.daemon.state !== "running") {
    return { label: "Changing state", tone: "warn", detail: `Daemon is ${overview.daemon.state}.` };
  }
  return { label: "Running", tone: "ok", detail: `Daemon pid ${overview.daemon.pid}.` };
}

/**
 * State of monitoring. "Active" is only ever reported when the daemon says
 * an engine exists AND that it is running. Nothing else may produce it.
 */
export function monitoringIndicator(phase: BridgePhase, overview: Overview | null): Indicator {
  if (phase === "ready" && overview?.ipc === "error") {
    return { label: "Unknown", tone: "unknown", detail: "The daemon is not answering, so whether it is monitoring is not known." };
  }
  const daemon = phase === "ready" ? (overview?.daemon ?? null) : null;
  if (daemon === null) {
    return { label: "Inactive", tone: "idle", detail: "Nothing is being monitored." };
  }
  const { state, engine_available, note } = daemon.monitoring;
  if (!engine_available) {
    return {
      label: "Not available",
      tone: "idle",
      detail: note || "The daemon reports that monitoring cannot run. Nothing is being monitored.",
    };
  }
  if (state === "running") {
    return { label: "Active", tone: "ok", detail: note || "Monitoring is active." };
  }
  if (state === "paused") {
    return { label: "Paused", tone: "warn", detail: note || "Monitoring is paused; nothing is being measured." };
  }
  return { label: "Off", tone: "idle", detail: note || "Monitoring is off; nothing is being measured." };
}

export function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const secs = Math.floor(seconds % 60);
  if (days > 0) return `${days}d ${hours}h ${minutes}m`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${secs}s`;
  return `${secs}s`;
}

/** Local wall-clock time for a Unix timestamp, e.g. "21:55:02". */
export function formatClock(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}
