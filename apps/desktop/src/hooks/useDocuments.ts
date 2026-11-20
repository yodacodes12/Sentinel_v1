import { useCallback, useEffect, useRef, useState } from "react";

import { BRIDGE_UNAVAILABLE, backend, toBridgeError } from "../services/bridge";
import type { BridgeError } from "../types/sentinel";
import type { DocumentsView, OpenTarget } from "../types/documents";
import { useSerialised } from "./useSerialised";

/** Progress is re-read this often, and only while a job runs. */
export const DOCUMENT_POLL_MS = 1_000;

/**
 * Document discovery state. Loaded when the page is shown (`active`) and
 * the service runs; polled only while a search or download is running.
 * Nothing runs in the background otherwise, and searches never start by
 * themselves.
 */
export function useDocuments(serviceRunning: boolean | null, active: boolean) {
  const [view, setView] = useState<DocumentsView | null>(null);
  const [error, setError] = useState<BridgeError | null>(null);
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const load = useCallback(async () => {
    if (serviceRunning !== true) {
      if (mounted.current) setView(null);
      return;
    }
    try {
      const v = await backend.documents();
      if (mounted.current) setView(v);
    } catch (e) {
      const err = toBridgeError(e);
      if (err.kind !== BRIDGE_UNAVAILABLE && mounted.current) setError(err);
    }
  }, [serviceRunning]);
  const refresh = useSerialised(load);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  const running = view?.progress.running === true;
  useEffect(() => {
    if (!active || !running) return;
    const timer = window.setInterval(() => void refresh(), DOCUMENT_POLL_MS);
    return () => window.clearInterval(timer);
  }, [active, running, refresh]);

  const act = useCallback(async (run: () => Promise<DocumentsView>) => {
    setBusy(true);
    setError(null);
    try {
      const v = await run();
      if (mounted.current) setView(v);
    } catch (e) {
      if (mounted.current) setError(toBridgeError(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }, []);

  return {
    view,
    error,
    busy,
    dismissError: () => setError(null),
    refresh,
    search: (force: boolean) => act(() => backend.startDocumentDiscovery(force)),
    cancel: () => act(() => backend.cancelDocumentDiscovery()),
    download: (id: number) => act(() => backend.downloadDocument(id)),
    removeFile: (id: number) => act(() => backend.removeDocumentFile(id)),
    clearCache: () => act(() => backend.clearDocumentCache()),
    open: async (id: number, target: OpenTarget) => {
      try {
        await backend.openDocument(id, target);
      } catch (e) {
        setError(toBridgeError(e));
      }
    },
  };
}
