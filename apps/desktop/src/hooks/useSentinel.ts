import { useCallback, useEffect, useRef, useState } from "react";

import type { BridgePhase } from "../app/viewModel";
import { BRIDGE_UNAVAILABLE, backend, toBridgeError } from "../services/bridge";
import type { BridgeError, ConfigPatch, ConfigView, Overview } from "../types/sentinel";
import { useSerialised } from "./useSerialised";
import { afterPollFailure, afterPollSuccess } from "../app/pollState";

/**
 * How often the daemon's state is re-read while the window is visible.
 * Deliberately slow: service state changes only when the user acts, and
 * every action refreshes immediately. Live telemetry has its own pace (see
 * useTelemetry). Polling stops when the window is hidden.
 */
export const REFRESH_INTERVAL_MS = 5_000;

export interface SentinelState {
  phase: BridgePhase;
  overview: Overview | null;
  config: ConfigView | null;
  /** A lifecycle action in flight, so controls can show and block on it. */
  busy: "starting" | "stopping" | "saving" | null;
  error: BridgeError | null;
  /** The error came from a background refresh; the next good one clears it. */
  pollError: boolean;
  stale: boolean;
}

export interface SentinelActions {
  refresh: () => Promise<void>;
  setServiceRunning: (running: boolean) => Promise<void>;
  updateConfiguration: (patch: ConfigPatch) => Promise<void>;
  dismissError: () => void;
}

const INITIAL: SentinelState = { phase: "loading", overview: null, config: null, busy: null, error: null, pollError: false, stale: false };

export function useSentinel(): SentinelState & SentinelActions {
  const [state, setState] = useState<SentinelState>(INITIAL);
  const mounted = useRef(true);

  const apply = useCallback((update: (previous: SentinelState) => SentinelState) => {
    if (mounted.current) setState(update);
  }, []);

  const fail = useCallback(
    (error: unknown) => {
      const bridgeError = toBridgeError(error);
      apply((previous) =>
        bridgeError.kind === BRIDGE_UNAVAILABLE
          ? { ...INITIAL, phase: "unavailable" }
          : { ...previous, phase: "ready", busy: null, error: bridgeError },
      );
    },
    [apply],
  );

  // Background refreshes never overlap: with a hung daemon each one waits
  // for the IPC timeout, and they must not pile up or land out of order.
  const refresh = useSerialised(
    useCallback(async () => {
      // Each answer is used on its own: the overview reports a broken
      // connection as its own state and must reach the header even when the
      // configuration request times out on the same hung daemon.
      const [overview, config] = await Promise.allSettled([backend.overview(), backend.configuration()]);
      const failure = [overview, config].find((r) => r.status === "rejected");
      const bridgeError = failure ? toBridgeError(failure.reason) : null;
      if (bridgeError?.kind === BRIDGE_UNAVAILABLE) {
        fail(failure!.reason);
        return;
      }
      apply((previous) => {
        const status = bridgeError ? afterPollFailure(previous, bridgeError) : afterPollSuccess(previous);
        return {
          ...previous,
          error: status.error,
          pollError: status.pollError,
          stale: status.stale,
          phase: "ready",
          overview: overview.status === "fulfilled" ? overview.value : previous.overview,
          // Kept, but marked stale, when it could not be re-read.
          config: config.status === "fulfilled" ? config.value : previous.config,
        };
      });
    }, [apply, fail]),
  );

  const setServiceRunning = useCallback(
    async (running: boolean) => {
      apply((previous) => ({ ...previous, busy: running ? "starting" : "stopping", error: null, pollError: false }));
      try {
        // The returned overview is what the daemon reports after the action,
        // so the switch shows the achieved state, not the requested one.
        const overview = await (running ? backend.startDaemon() : backend.stopDaemon());
        const config = await backend.configuration();
        apply((previous) => ({ ...previous, phase: "ready", overview, config, busy: null }));
      } catch (error) {
        fail(error);
        void refresh();
      }
    },
    [apply, fail, refresh],
  );

  const updateConfiguration = useCallback(
    async (patch: ConfigPatch) => {
      apply((previous) => ({ ...previous, busy: "saving", error: null, pollError: false }));
      try {
        const config = await backend.updateConfiguration(patch);
        const overview = await backend.overview();
        apply((previous) => ({ ...previous, config, overview, busy: null }));
      } catch (error) {
        fail(error);
      }
    },
    [apply, fail],
  );

  const dismissError = useCallback(() => apply((previous) => ({ ...previous, error: null, pollError: false })), [apply]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    let timer: number | undefined;

    const stop = () => {
      if (timer !== undefined) window.clearInterval(timer);
      timer = undefined;
    };
    const start = () => {
      stop();
      if (document.visibilityState !== "visible") return;
      void refresh();
      timer = window.setInterval(() => void refresh(), REFRESH_INTERVAL_MS);
    };

    start();
    document.addEventListener("visibilitychange", start);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", start);
    };
  }, [refresh]);

  return { ...state, refresh, setServiceRunning, updateConfiguration, dismissError };
}
