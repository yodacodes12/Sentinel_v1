import type { DocumentSettings } from "./documents";

// Mirrors the Rust types serialised by the desktop bridge
// (sentinel-client::Overview, sentinel-ipc::DaemonStatus, sentinel-config).

export type PowerMode = "eco" | "balanced" | "performance";
export type Theme = "dark" | "black";
export type DaemonState = "starting" | "running" | "shutting_down" | "stopped";
export type MonitoringState = "stopped" | "running" | "paused";
export type MonitoringCommand = "start" | "stop" | "pause" | "resume";
export type IpcState = "connected" | "not_running" | "error";

export interface MonitoringStatus {
  state: MonitoringState;
  engine_available: boolean;
  note: string;
}

export interface DaemonStatus {
  app_version: string;
  protocol_version: number;
  pid: number;
  state: DaemonState;
  started_at: number;
  uptime_secs: number;
  monitoring: MonitoringStatus;
  operating_mode: PowerMode;
  config_path: string;
  storage_available: boolean;
  recovered_from_unclean_shutdown: boolean;
}

export interface Overview {
  client_version: string;
  running: boolean;
  ipc: IpcState;
  ipc_detail: string | null;
  socket_path: string;
  daemon: DaemonStatus | null;
  operating_mode: PowerMode;
  config_path: string;
  log_path: string;
}

export interface AlertThresholdsConfig {
  cpu_high_percent: number;
  cpu_high_duration_s: number;
  load_per_cpu_high: number;
  memory_available_low_percent: number;
  memory_available_critical_percent: number;
  swap_activity_pages_per_s: number;
  battery_low_percent: number;
  battery_critical_percent: number;
  battery_health_warning_percent: number;
  storage_free_low_percent: number;
  storage_free_critical_percent: number;
  disk_busy_high_percent: number;
  temperature_warning_c?: number | null;
  battery_temperature_warning_c?: number | null;
  battery_discharge_high_w?: number | null;
  network_high_mbps?: number | null;
  sustain_s: number;
  cooldown_s: number;
}

export interface RetentionConfig {
  detailed_hours: number;
  hourly_days: number;
  alert_days: number;
}

export interface SentinelConfig {
  version: number;
  monitoring_enabled: boolean;
  power_mode: PowerMode;
  notifications_enabled: boolean;
  theme: Theme;
  power_aware_sampling: boolean;
  alerts: AlertThresholdsConfig;
  retention: RetentionConfig;
  documents: DocumentSettings;
}

export type ConfigPatch = Partial<Omit<SentinelConfig, "version">>;

export interface ConfigView {
  config: SentinelConfig;
  path: string;
  source: "daemon" | "file";
  warning: string | null;
}

/** Error shape returned by every bridge command. */
export interface BridgeError {
  kind: string;
  message: string;
}
