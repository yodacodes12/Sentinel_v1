import { useCallback, useEffect, useRef, useState } from "react";

import { BRIDGE_UNAVAILABLE, backend, toBridgeError } from "../services/bridge";
import { useSerialised } from "./useSerialised";
import { afterPollFailure, afterPollSuccess } from "../app/pollState";
import type { MonitoringCommand, BridgeError } from "../types/sentinel";
import type { Alert, MonitoringReport, SamplingProfile, Series, TelemetrySnapshot, TimeWindow } from "../types/telemetry";

/** Status is re-read this often when monitoring is not active. */
export const IDLE_REFRESH_MS = 5_000;
/** Never poll faster than this, whatever the sampling profile. */
export const MIN_POLL_MS = 1_000;

export interface TelemetryState {
  report: MonitoringReport | null;
  snapshot: TelemetrySnapshot | null;
  alerts: Alert[];
  busy: MonitoringCommand | "sampling" | null;
  error: BridgeError | null;
  /** The error came from background polling; the next good poll clears it. */
  pollError: boolean;
  /**
   * The last poll failed: what is shown is the last answer received, not
   * live data. Pages must not call it live.
   */
  stale: boolean;
  updatedAt: number | null;
}


function visible(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

/**
 * Live telemetry for the whole app. While monitoring is active and the
 * window is visible, metrics are polled at the CPU sampling interval;
 * otherwise only the status is checked every few seconds; nothing at all
 * while hidden.
 */
export function useTelemetry(serviceRunning: boolean | null) {
  const [state, setState] = useState<TelemetryState>({ report: null, snapshot: null, alerts: [], busy: null, error: null, pollError: false, stale: false, updatedAt: null });
  const mounted = useRef(true);
  const apply = useCallback((f: (s: TelemetryState) => TelemetryState) => {
    if (mounted.current) setState(f);
  }, []);

  const poll = useCallback(async () => {
    if (serviceRunning === false) {
      apply((s) => ({ ...s, ...afterPollSuccess(s), report: null, snapshot: null, alerts: [] }));
      return;
    }
    if (serviceRunning === null) {
      // Not known yet, or the daemon is not answering: whatever is shown
      // is no longer live.
      apply((s) => (s.report && !s.stale ? { ...s, stale: true } : s));
      return;
    }
    try {
      const answer = await backend.metrics("all");
      const alerts = answer.monitoring.phase === "off" ? [] : await backend.activeAlerts();
      apply((s) => ({ ...s, ...afterPollSuccess(s), report: answer.monitoring, snapshot: answer.snapshot, alerts, updatedAt: Date.now() }));
    } catch (e) {
      const err = toBridgeError(e);
      if (err.kind === BRIDGE_UNAVAILABLE) return;
      apply((s) => {
        const { dropData, ...status } = afterPollFailure(s, err);
        return dropData ? { ...s, ...status, report: null, snapshot: null, alerts: [] } : { ...s, ...status };
      });
    }
  }, [serviceRunning, apply]);
  const refresh = useSerialised(poll);

  const command = useCallback(
    async (c: MonitoringCommand) => {
      apply((s) => ({ ...s, busy: c, error: null, pollError: false }));
      try {
        await backend.monitoring(c);
      } catch (e) {
        apply((s) => ({ ...s, error: toBridgeError(e), pollError: false }));
      }
      apply((s) => ({ ...s, busy: null }));
      await refresh();
    },
    [apply, refresh],
  );

  const setSampling = useCallback(
    async (profile: SamplingProfile, powerAware: boolean | null) => {
      apply((s) => ({ ...s, busy: "sampling", error: null, pollError: false }));
      try {
        await backend.setSampling(profile, powerAware);
      } catch (e) {
        apply((s) => ({ ...s, error: toBridgeError(e), pollError: false }));
      }
      apply((s) => ({ ...s, busy: null }));
      await refresh();
    },
    [apply, refresh],
  );

  const acknowledge = useCallback(
    async (id: number) => {
      try {
        await backend.acknowledgeAlert(id);
      } catch (e) {
        apply((s) => ({ ...s, error: toBridgeError(e), pollError: false }));
      }
      await refresh();
    },
    [apply, refresh],
  );

  const active = state.report?.phase === "active";
  const cpuInterval = state.report?.sampling.intervals_ms.find(([k]) => k === "cpu")?.[1] ?? 2_000;
  const period = active ? Math.max(MIN_POLL_MS, cpuInterval) : IDLE_REFRESH_MS;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  // Poll at once when the service starts or stops, then at `period`.
  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => {
      if (visible()) void refresh();
    }, period);
    const onVisible = () => visible() && void refresh();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [refresh, period, serviceRunning]);

  const dismissError = useCallback(() => apply((s) => ({ ...s, error: null, pollError: false })), [apply]);
  return { ...state, refresh, command, setSampling, acknowledge, dismissError };
}

/**
 * History for a chart. Polled at `refreshMs` only while `enabled` (monitoring
 * active, page visible); the server bounds the number of points.
 */
export function useSeries(keys: string[], window: TimeWindow, enabled: boolean, refreshMs: number): Series[] {
  const [series, setSeries] = useState<Series[]>([]);
  const joined = keys.join("|");
  useEffect(() => {
    let cancelled = false;
    let inFlight = false;
    const list = joined ? joined.split("|") : [];
    const load = async () => {
      // A tick while the previous request is still out is skipped.
      if (list.length === 0 || !visible() || inFlight) return;
      inFlight = true;
      try {
        const result = await backend.history(list.slice(0, 32), window);
        if (!cancelled) setSeries(result);
      } catch {
        // History is optional; charts show "No data" instead.
      } finally {
        inFlight = false;
      }
    };
    void load();
    if (!enabled) return () => {
      cancelled = true;
    };
    const timer = globalThis.setInterval(() => void load(), Math.max(MIN_POLL_MS, refreshMs));
    return () => {
      cancelled = true;
      globalThis.clearInterval(timer);
    };
  }, [joined, window, enabled, refreshMs]);
  return series;
}
