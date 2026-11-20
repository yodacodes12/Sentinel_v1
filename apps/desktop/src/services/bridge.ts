// The only place the frontend talks to the backend.
//
// Every call is a Tauri command implemented in src-tauri, which in turn uses
// the same `sentinel-client` crate as the CLI. The frontend performs no
// network requests and reads nothing from the system itself.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type {
  BridgeError,
  ConfigPatch,
  ConfigView,
  MonitoringCommand,
  MonitoringStatus,
  Overview,
} from "../types/sentinel";
import type {
  DiscoveryJob,
  DiscoveryStatus,
  HardwareCapabilities,
  InventoryView,
  SystemUtility,
  UtilitiesView,
} from "../types/inventory";
import type {
  Alert,
  MetricsAnswer,
  MetricsSection,
  MonitoringReport,
  ProcessAnswer,
  SamplingInfo,
  SamplingProfile,
  Series,
  TimeWindow,
} from "../types/telemetry";
import type { DocumentsView, OpenTarget } from "../types/documents";

/** True inside the Tauri shell; false in a plain browser (frontend-only dev). */
export function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const BRIDGE_UNAVAILABLE = "bridge_unavailable";

export function toBridgeError(error: unknown): BridgeError {
  if (
    typeof error === "object" &&
    error !== null &&
    "kind" in error &&
    "message" in error &&
    typeof error.kind === "string" &&
    typeof error.message === "string"
  ) {
    return { kind: error.kind, message: error.message };
  }
  return { kind: "unknown", message: typeof error === "string" ? error : "Unexpected error." };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop()) {
    throw {
      kind: BRIDGE_UNAVAILABLE,
      message: "This page is running outside the Sentinel desktop app, so it cannot reach the daemon.",
    } satisfies BridgeError;
  }
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toBridgeError(error);
  }
}

export const backend = {
  overview: () => call<Overview>("get_overview"),
  startDaemon: () => call<Overview>("start_daemon"),
  stopDaemon: () => call<Overview>("stop_daemon"),
  configuration: () => call<ConfigView>("get_configuration"),
  updateConfiguration: (patch: ConfigPatch) => call<ConfigView>("update_configuration", { patch }),
  monitoring: (command: MonitoringCommand) => call<MonitoringStatus>("monitoring_command", { command }),
  inventory: () => call<InventoryView>("get_inventory"),
  utilities: () => call<UtilitiesView>("get_utilities"),
  discoveryStatus: () => call<DiscoveryStatus>("get_discovery_status"),
  /** Ask the daemon to scan; returns at once. `force` rescans even if a result exists. */
  runDiscovery: (job: DiscoveryJob, force: boolean) => call<DiscoveryStatus>("run_discovery", { job, force }),
  cancelDiscovery: (job: DiscoveryJob) => call<DiscoveryStatus>("cancel_discovery", { job }),
  capabilities: () => call<HardwareCapabilities>("get_capabilities"),
  /** Open an installed utility. Only ever called from a user's click. */
  launchUtility: (id: string) => call<SystemUtility>("launch_utility", { id }),
  monitoringReport: () => call<MonitoringReport>("get_monitoring_status"),
  metrics: (section: MetricsSection) => call<MetricsAnswer>("get_metrics", { section }),
  processes: () => call<ProcessAnswer>("get_processes"),
  activeAlerts: () => call<Alert[]>("get_active_alerts"),
  alertHistory: (limit: number) => call<Alert[]>("get_alert_history", { limit }),
  /** Marks an alert as seen; changes nothing on the system. */
  acknowledgeAlert: (alertId: number) => call<Alert>("acknowledge_alert", { alertId }),
  sampling: () => call<SamplingInfo>("get_sampling"),
  setSampling: (profile: SamplingProfile, powerAware: boolean | null) =>
    call<SamplingInfo>("set_sampling", { profile, powerAware }),
  history: (series: string[], window: TimeWindow) =>
    call<Series[]>("get_telemetry_history", { query: { series, window } }),
  // Phase 6: documents are named by the daemon's numeric ID only.
  documents: () => call<DocumentsView>("get_documents"),
  startDocumentDiscovery: (force: boolean) => call<DocumentsView>("start_document_discovery", { force }),
  cancelDocumentDiscovery: () => call<DocumentsView>("cancel_document_discovery"),
  downloadDocument: (documentId: number) => call<DocumentsView>("download_document", { documentId }),
  removeDocumentFile: (documentId: number) => call<DocumentsView>("remove_document_file", { documentId }),
  clearDocumentCache: () => call<DocumentsView>("clear_document_cache"),
  openDocument: (documentId: number, target: OpenTarget) => call<string>("open_document", { documentId, target }),
};

export const windowControls = {
  minimize: () => (isDesktop() ? getCurrentWindow().minimize() : Promise.resolve()),
  toggleMaximize: () => (isDesktop() ? getCurrentWindow().toggleMaximize() : Promise.resolve()),
  // Closing the window never stops the daemon: they are separate processes.
  close: () => (isDesktop() ? getCurrentWindow().close() : Promise.resolve()),
};
