// Mirrors crates/sentinel-telemetry/src/model.rs as serialised by the bridge.

export type ReadingState = "ok" | "pending" | "unknown" | "unsupported" | "unavailable" | "permission_denied" | "error";
export type Quality = "direct" | "calculated" | "estimated";

/** A measurement. `value` is present only when `state` is "ok". */
export interface Reading<T> {
  value?: T;
  state: ReadingState;
  quality: Quality;
}

export type CollectorKind = "cpu" | "memory" | "processes" | "battery" | "thermal" | "gpu" | "storage" | "network" | "fans" | "system_load";
export type CollectorHealth = "waiting" | "ok" | "partial" | "unsupported" | "failing";
export type SamplingProfile = "eco" | "balanced" | "performance";
export type MonitoringPhase = "off" | "starting" | "active" | "paused" | "stopping" | "error";

export interface CollectorStatus {
  kind: CollectorKind;
  health: CollectorHealth;
  interval_ms: number;
  last_sample_at: number | null;
  last_duration_us: number | null;
  consecutive_failures: number;
  note: string | null;
}

export interface SamplingInfo {
  selected: SamplingProfile;
  effective: SamplingProfile;
  power_aware: boolean;
  reason: string | null;
  intervals_ms: [CollectorKind, number][];
}

export interface NotificationStatus {
  enabled: boolean;
  last_delivery_ok: boolean | null;
  last_error: string | null;
  delivered: number;
  failed: number;
}

export interface MonitoringReport {
  phase: MonitoringPhase;
  since: number;
  sampling: SamplingInfo;
  collectors: CollectorStatus[];
  last_sample_at: number | null;
  error: string | null;
  enabled_preference: boolean;
  history_available: boolean;
  notifications: NotificationStatus;
}

export interface CpuSample {
  measured_at: number;
  interval_ms: number;
  logical_cpus: number;
  total_percent: Reading<number>;
  times: Record<
    "user_percent" | "nice_percent" | "system_percent" | "idle_percent" | "iowait_percent" | "irq_percent" | "softirq_percent" | "steal_percent",
    Reading<number>
  >;
  cores: { cpu: number; utilisation_percent: Reading<number>; frequency_khz: Reading<number> }[];
  average_frequency_khz: Reading<number>;
}

export interface Pressure {
  some_avg10: Reading<number>;
  some_avg60: Reading<number>;
  full_avg10: Reading<number>;
  full_avg60: Reading<number>;
}

export interface MemorySample {
  measured_at: number;
  interval_ms: number;
  total_bytes: Reading<number>;
  available_bytes: Reading<number>;
  used_bytes: Reading<number>;
  used_percent: Reading<number>;
  free_bytes: Reading<number>;
  buffers_bytes: Reading<number>;
  cached_bytes: Reading<number>;
  active_bytes: Reading<number>;
  inactive_bytes: Reading<number>;
  swap_total_bytes: Reading<number>;
  swap_used_bytes: Reading<number>;
  swap_free_bytes: Reading<number>;
  swap_in_pages_per_s: Reading<number>;
  swap_out_pages_per_s: Reading<number>;
  pressure: Pressure;
}

export type ProcessState = "running" | "sleeping" | "disk_sleep" | "stopped" | "zombie" | "idle" | "other";

export interface ProcessSample {
  pid: number;
  name: string;
  command: string | null;
  state: ProcessState;
  uid: number | null;
  user: string | null;
  cpu_percent: Reading<number>;
  rss_bytes: number;
  virtual_bytes: number;
  memory_percent: Reading<number>;
  threads: number;
  started_at: number | null;
}

export interface ProcessList {
  measured_at: number;
  interval_ms: number;
  total: number;
  running: number;
  inaccessible: number;
  processes: ProcessSample[];
}

export type ChargeState = "charging" | "discharging" | "not_charging" | "full" | "unknown";
export type EstimateConfidence = "low" | "medium" | "high";

export type RuntimeEstimate =
  | { state: "not_discharging" }
  | { state: "calculating"; samples: number; needed: number }
  | { state: "insufficient"; reason: string }
  | { state: "estimated"; seconds: number; confidence: EstimateConfidence; average_power_w: number | null; samples: number; window_s: number };

export interface BatterySample {
  measured_at: number;
  interval_ms: number;
  name: string;
  state: Reading<ChargeState>;
  percent: Reading<number>;
  energy_now_wh: Reading<number>;
  energy_full_wh: Reading<number>;
  energy_design_wh: Reading<number>;
  charge_now_ah: Reading<number>;
  charge_full_ah: Reading<number>;
  charge_design_ah: Reading<number>;
  voltage_v: Reading<number>;
  current_a: Reading<number>;
  power_w: Reading<number>;
  cycle_count: Reading<number>;
  health_percent: Reading<number>;
  temperature_c: Reading<number>;
  runtime: RuntimeEstimate;
  technology: string | null;
  manufacturer: string | null;
  model: string | null;
}

export interface BatteryReport {
  measured_at: number;
  interval_ms: number;
  batteries: BatterySample[];
  sources: { name: string; kind: string; online: Reading<boolean> }[];
  on_ac: Reading<boolean>;
}

export type Component = "cpu" | "gpu" | "storage" | "battery" | "memory" | "wireless" | "chassis" | "unknown";
export type AssociationConfidence = "none" | "low" | "medium" | "high";

export interface TemperatureReading {
  id: string;
  chip: string;
  label: string | null;
  value_c: Reading<number>;
  max_c: number | null;
  critical_c: number | null;
  association: { component: Component; confidence: AssociationConfidence; basis: string };
  source: string;
}

export interface FanReading {
  id: string;
  chip: string;
  label: string | null;
  rpm: Reading<number>;
  min_rpm: number | null;
  max_rpm: number | null;
  source: string;
}

export interface GpuSample {
  pci_address: string;
  driver: string | null;
  memory_model: "dedicated" | "shared" | "unknown";
  busy_percent: Reading<number>;
  vram_used_bytes: Reading<number>;
  vram_total_bytes: Reading<number>;
  gtt_used_bytes: Reading<number>;
  gtt_total_bytes: Reading<number>;
  temperature_c: Reading<number>;
  power_w: Reading<number>;
  core_clock_mhz: Reading<number>;
  note: string | null;
}

export interface DiskIo {
  name: string;
  read_bps: Reading<number>;
  write_bps: Reading<number>;
  reads_per_s: Reading<number>;
  writes_per_s: Reading<number>;
  busy_percent: Reading<number>;
  read_total_bytes: number;
  written_total_bytes: number;
}

export interface FilesystemUsage {
  mount_point: string;
  device: string;
  fs_type: string;
  total_bytes: Reading<number>;
  used_bytes: Reading<number>;
  available_bytes: Reading<number>;
  used_percent: Reading<number>;
  read_only: boolean;
}

export interface InterfaceTraffic {
  name: string;
  operational_state: string | null;
  loopback: boolean;
  rx_bps: Reading<number>;
  tx_bps: Reading<number>;
  rx_packets_per_s: Reading<number>;
  tx_packets_per_s: Reading<number>;
  rx_total_bytes: number;
  tx_total_bytes: number;
  rx_errors: number;
  tx_errors: number;
  rx_dropped: number;
  tx_dropped: number;
}

export interface SystemLoad {
  measured_at: number;
  interval_ms: number;
  load1: Reading<number>;
  load5: Reading<number>;
  load15: Reading<number>;
  runnable_tasks: Reading<number>;
  total_tasks: Reading<number>;
  uptime_s: Reading<number>;
  cpu_pressure: Pressure;
  io_pressure: Pressure;
}

export type HealthLevel = "unknown" | "normal" | "elevated" | "high" | "critical";
export type HealthArea =
  | "cpu_pressure"
  | "memory_pressure"
  | "swap_pressure"
  | "thermal"
  | "battery"
  | "storage_capacity"
  | "storage_activity"
  | "network_activity"
  | "sensor_availability";

export interface HealthIndicator {
  area: HealthArea;
  level: HealthLevel;
  summary: string;
  evidence: string[];
}

export interface TelemetrySnapshot {
  cpu: CpuSample | null;
  memory: MemorySample | null;
  battery: BatteryReport | null;
  thermal: { measured_at: number; interval_ms: number; temperatures: TemperatureReading[] } | null;
  fans: { measured_at: number; interval_ms: number; fans: FanReading[] } | null;
  gpu: { measured_at: number; interval_ms: number; gpus: GpuSample[] } | null;
  storage: { measured_at: number; interval_ms: number; devices: DiskIo[]; filesystems: FilesystemUsage[] } | null;
  network: { measured_at: number; interval_ms: number; interfaces: InterfaceTraffic[] } | null;
  load: SystemLoad | null;
  health: HealthIndicator[];
  top_processes: ProcessSample[];
}

export interface MetricsAnswer {
  monitoring: MonitoringReport;
  snapshot: TelemetrySnapshot | null;
}

export interface ProcessAnswer {
  monitoring: MonitoringReport;
  processes: ProcessList | null;
}

export type AlertSeverity = "info" | "warning" | "critical";
export type AlertCategory = "cpu" | "memory" | "battery" | "storage" | "thermal" | "network";

export interface Alert {
  id: number;
  rule: string;
  category: AlertCategory;
  severity: AlertSeverity;
  title: string;
  explanation: string;
  recommendation: string | null;
  metric: string;
  subject: string | null;
  threshold: string;
  evidence: string;
  detected_at: number;
  last_updated_at: number;
  resolved_at: number | null;
  status: "active" | "resolved";
  acknowledged: boolean;
  notified: boolean;
  threshold_source: "default" | "user" | "driver";
}

export type TimeWindow = "minutes5" | "minutes15" | "hour1" | "hours24" | "days7" | "days30";

export interface SeriesPoint {
  t: number;
  v: number | null;
  min?: number;
  max?: number;
}

export interface Series {
  key: string;
  unit: string;
  /** Where the values come from, e.g. "/proc/stat"; empty when unknown. */
  source: string;
  quality: Quality | null;
  resolution: "raw" | "detailed" | "hourly";
  points: SeriesPoint[];
}

export type MetricsSection = "all" | "cpu" | "memory" | "battery" | "thermal" | "gpu" | "storage" | "network";
