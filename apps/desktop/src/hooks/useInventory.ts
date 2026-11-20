import { useCallback, useEffect, useRef, useState } from "react";

import { isScanning } from "../app/inventoryView";
import { BRIDGE_UNAVAILABLE, backend, toBridgeError } from "../services/bridge";
import type { DiscoveryJob, DiscoveryStatus, InventoryView, UtilitiesView } from "../types/inventory";
import type { BridgeError } from "../types/sentinel";

/** Progress is polled this often, and only while a scan is queued or running. */
export const SCAN_POLL_MS = 750;

export interface InventoryState {
  inventory: InventoryView | null;
  utilities: UtilitiesView | null;
  status: DiscoveryStatus | null;
  error: BridgeError | null;
  launching: string | null;
  launched: string | null;
}

export interface InventoryActions {
  reload: () => Promise<void>;
  scan: (jobs: DiscoveryJob[], force: boolean) => Promise<void>;
  cancel: (job: DiscoveryJob) => Promise<void>;
  launch: (id: string) => Promise<void>;
  dismissError: () => void;
}

const INITIAL: InventoryState = { inventory: null, utilities: null, status: null, error: null, launching: null, launched: null };

/**
 * Inventory and utility catalogue. Loaded once, again when the service starts
 * or stops, and when a scan finishes. Nothing is rescanned automatically.
 */
export function useInventory(serviceRunning: boolean | null): InventoryState & InventoryActions {
  const [state, setState] = useState<InventoryState>(INITIAL);
  const mounted = useRef(true);
  const apply = useCallback((update: (s: InventoryState) => InventoryState) => {
    if (mounted.current) setState(update);
  }, []);
  const fail = useCallback(
    (error: unknown) => {
      const e = toBridgeError(error);
      if (e.kind !== BRIDGE_UNAVAILABLE) apply((s) => ({ ...s, error: e, launching: null }));
    },
    [apply],
  );

  const reload = useCallback(async () => {
    try {
      const [inventory, utilities] = await Promise.all([backend.inventory(), backend.utilities()]);
      let status: DiscoveryStatus | null = null;
      if (inventory.source === "daemon") {
        status = await backend.discoveryStatus().catch(() => null);
      }
      apply((s) => ({ ...s, inventory, utilities, status }));
    } catch (error) {
      fail(error);
    }
  }, [apply, fail]);

  const scan = useCallback(
    async (jobs: DiscoveryJob[], force: boolean) => {
      apply((s) => ({ ...s, error: null }));
      try {
        let status: DiscoveryStatus | null = null;
        for (const job of jobs) status = await backend.runDiscovery(job, force);
        apply((s) => ({ ...s, status }));
      } catch (error) {
        fail(error);
      }
    },
    [apply, fail],
  );

  const cancel = useCallback(
    async (job: DiscoveryJob) => {
      try {
        const status = await backend.cancelDiscovery(job);
        apply((s) => ({ ...s, status }));
      } catch (error) {
        fail(error);
      }
    },
    [apply, fail],
  );

  const launch = useCallback(
    async (id: string) => {
      apply((s) => ({ ...s, launching: id, launched: null, error: null }));
      try {
        const utility = await backend.launchUtility(id);
        apply((s) => ({ ...s, launching: null, launched: utility.name }));
      } catch (error) {
        fail(error);
      }
    },
    [apply, fail],
  );

  const dismissError = useCallback(() => apply((s) => ({ ...s, error: null, launched: null })), [apply]);

  // Load on mount and whenever the service starts or stops.
  useEffect(() => {
    mounted.current = true;
    if (serviceRunning !== null) void reload();
    return () => {
      mounted.current = false;
    };
  }, [serviceRunning, reload]);

  // Poll progress only while a scan is active; reload results when it ends.
  const active = isScanning(state.status?.hardware) || isScanning(state.status?.utilities);
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const status = await backend.discoveryStatus();
        apply((s) => ({ ...s, status }));
        if (!isScanning(status.hardware) && !isScanning(status.utilities)) void reload();
      } catch (error) {
        fail(error);
      }
    }, SCAN_POLL_MS);
    return () => window.clearInterval(timer);
  }, [active, apply, fail, reload]);

  return { ...state, reload, scan, cancel, launch, dismissError };
}
