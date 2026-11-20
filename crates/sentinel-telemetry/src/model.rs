//! Serializable telemetry types shared by the daemon, IPC, client, CLI and
//! (mirrored in TypeScript) the desktop frontend.
//!
//! Units are part of field names (`_percent`, `_bytes`, `_bps` = bytes per
//! second, `_c` = degrees Celsius, `_w`, `_v`, `_a`, `_khz`, `_mhz`, `_rpm`,
//! `_s`). A reading never carries a value it did not measure: every value is
//! wrapped in [`Reading`], whose `state` says why a value is absent.

use serde::{Deserialize, Serialize};

// ------------------------------------------------------------ readings

/// Why a reading has or lacks a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingState {
    /// Measured.
    Ok,
    /// A rate or percentage needs two samples; this is the first.
    Pending,
    /// No source was found; reason not known.
    Unknown,
    /// This machine, kernel or driver does not provide it.
    Unsupported,
    /// The source exists but gave nothing (device absent, link down...).
    Unavailable,
    /// Readable only by administrators. Never bypassed.
    PermissionDenied,
    /// The source failed or returned malformed data.
    Error,
}

/// How a value was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// Read directly from the kernel or device.
    Direct,
    /// Computed exactly from direct readings (deltas, sums, ratios).
    Calculated,
    /// An estimate with uncertainty (battery runtime, health).
    Estimated,
}

/// One measured value with its state and quality.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reading<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    pub state: ReadingState,
    pub quality: Quality,
}

impl<T> Reading<T> {
    pub fn direct(value: T) -> Self {
        Self {
            value: Some(value),
            state: ReadingState::Ok,
            quality: Quality::Direct,
        }
    }
    pub fn calculated(value: T) -> Self {
        Self {
            value: Some(value),
            state: ReadingState::Ok,
            quality: Quality::Calculated,
        }
    }
    pub fn estimated(value: T) -> Self {
        Self {
            value: Some(value),
            state: ReadingState::Ok,
            quality: Quality::Estimated,
        }
    }
    /// No value. `Ok` is downgraded to `Unknown` so a valueless reading can
    /// never claim to be a measurement.
    pub fn missing(state: ReadingState) -> Self {
        let state = if state == ReadingState::Ok {
            ReadingState::Unknown
        } else {
            state
        };
        Self {
            value: None,
            state,
            quality: Quality::Direct,
        }
    }
    pub fn pending() -> Self {
        Self::missing(ReadingState::Pending)
    }
    pub fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }
    pub fn is_ok(&self) -> bool {
        self.state == ReadingState::Ok && self.value.is_some()
    }
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Reading<U> {
        Reading {
            value: self.value.map(f),
            state: self.state,
            quality: self.quality,
        }
    }
}

impl<T: Copy> Reading<T> {
    pub fn value(&self) -> Option<T> {
        self.value
    }
}

/// Round to one decimal place for display stability and compact JSON.
pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// --------------------------------------------------------- collectors

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectorKind {
    Cpu,
    Memory,
    Processes,
    Battery,
    Thermal,
    Gpu,
    Storage,
    Network,
    Fans,
    SystemLoad,
}

impl CollectorKind {
    pub const ALL: [CollectorKind; 10] = [
        CollectorKind::Cpu,
        CollectorKind::Memory,
        CollectorKind::Processes,
        CollectorKind::Battery,
        CollectorKind::Thermal,
        CollectorKind::Gpu,
        CollectorKind::Storage,
        CollectorKind::Network,
        CollectorKind::Fans,
        CollectorKind::SystemLoad,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CollectorKind::Cpu => "CPU",
            CollectorKind::Memory => "Memory",
            CollectorKind::Processes => "Processes",
            CollectorKind::Battery => "Battery",
            CollectorKind::Thermal => "Temperatures",
            CollectorKind::Gpu => "GPU",
            CollectorKind::Storage => "Storage",
            CollectorKind::Network => "Network",
            CollectorKind::Fans => "Fans",
            CollectorKind::SystemLoad => "System load",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectorHealth {
    /// Has not run yet in this monitoring session.
    Waiting,
    Ok,
    /// Ran, but some readings were unavailable.
    Partial,
    /// Its source does not exist on this machine; checked rarely.
    Unsupported,
    /// Failing; retried with backoff.
    Failing,
}

/// Status of one collector, shown so the user can see what is really live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorStatus {
    pub kind: CollectorKind,
    pub health: CollectorHealth,
    /// Current sampling interval, after profile and backoff.
    pub interval_ms: u64,
    /// Unix milliseconds of the last successful sample.
    pub last_sample_at: Option<u64>,
    /// Duration of the last run in microseconds.
    pub last_duration_us: Option<u64>,
    pub consecutive_failures: u32,
    pub note: Option<String>,
}

// ----------------------------------------------------- sampling profile

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingProfile {
    Eco,
    Balanced,
    Performance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplingInfo {
    /// The profile the user selected.
    pub selected: SamplingProfile,
    /// The profile in use: Eco when power-aware sampling is on and the
    /// computer runs on battery, otherwise the selected one.
    pub effective: SamplingProfile,
    pub power_aware: bool,
    /// Why `effective` differs from `selected`, if it does.
    pub reason: Option<String>,
    pub intervals_ms: Vec<(CollectorKind, u64)>,
}

// ------------------------------------------------------------ sections

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuTimes {
    pub user_percent: Reading<f64>,
    pub nice_percent: Reading<f64>,
    pub system_percent: Reading<f64>,
    pub idle_percent: Reading<f64>,
    pub iowait_percent: Reading<f64>,
    pub irq_percent: Reading<f64>,
    pub softirq_percent: Reading<f64>,
    pub steal_percent: Reading<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoreSample {
    /// Logical CPU number.
    pub cpu: u32,
    pub utilisation_percent: Reading<f64>,
    pub frequency_khz: Reading<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuSample {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub logical_cpus: u32,
    pub total_percent: Reading<f64>,
    pub times: CpuTimes,
    pub cores: Vec<CoreSample>,
    /// Mean of the per-core current frequencies.
    pub average_frequency_khz: Reading<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pressure {
    /// Percentage of time some tasks stalled, averaged over 10 s / 60 s.
    pub some_avg10: Reading<f64>,
    pub some_avg60: Reading<f64>,
    /// Percentage of time all non-idle tasks stalled.
    pub full_avg10: Reading<f64>,
    pub full_avg60: Reading<f64>,
}

impl Pressure {
    pub fn missing(state: ReadingState) -> Self {
        Self {
            some_avg10: Reading::missing(state),
            some_avg60: Reading::missing(state),
            full_avg10: Reading::missing(state),
            full_avg60: Reading::missing(state),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySample {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub total_bytes: Reading<u64>,
    /// `MemAvailable`: what can be given to applications without swapping.
    pub available_bytes: Reading<u64>,
    /// `MemTotal - MemAvailable`.
    pub used_bytes: Reading<u64>,
    pub used_percent: Reading<f64>,
    pub free_bytes: Reading<u64>,
    pub buffers_bytes: Reading<u64>,
    /// Page cache including reclaimable slab.
    pub cached_bytes: Reading<u64>,
    pub active_bytes: Reading<u64>,
    pub inactive_bytes: Reading<u64>,
    pub swap_total_bytes: Reading<u64>,
    pub swap_used_bytes: Reading<u64>,
    pub swap_free_bytes: Reading<u64>,
    pub swap_in_pages_per_s: Reading<f64>,
    pub swap_out_pages_per_s: Reading<f64>,
    pub pressure: Pressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Running,
    Sleeping,
    DiskSleep,
    Stopped,
    Zombie,
    Idle,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessSample {
    pub pid: u32,
    /// Kernel short name (`comm`), at most 15 characters.
    pub name: String,
    /// Program name only (first argument without directories). Arguments
    /// are never collected: they can contain secrets.
    pub command: Option<String>,
    pub state: ProcessState,
    pub uid: Option<u32>,
    pub user: Option<String>,
    /// Share of one CPU (100 = one full core), like `top`.
    pub cpu_percent: Reading<f64>,
    pub rss_bytes: u64,
    pub virtual_bytes: u64,
    pub memory_percent: Reading<f64>,
    pub threads: u32,
    /// Unix seconds.
    pub started_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessList {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub total: u32,
    pub running: u32,
    /// Processes whose details this user may not read.
    pub inaccessible: u32,
    pub processes: Vec<ProcessSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeState {
    Charging,
    Discharging,
    NotCharging,
    Full,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RuntimeEstimate {
    /// Not discharging.
    NotDischarging,
    /// Discharging, but not enough valid samples yet.
    Calculating { samples: u32, needed: u32 },
    /// The power readings are missing or too unstable for an estimate.
    Insufficient { reason: String },
    Estimated {
        seconds: u64,
        confidence: EstimateConfidence,
        /// Average discharge power the estimate is based on, in watts.
        /// For batteries that report charge (Ah) rather than energy, the
        /// average current times the present voltage; `None` when the
        /// voltage is not reported.
        #[serde(default)]
        average_power_w: Option<f64>,
        samples: u32,
        window_s: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatterySample {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub name: String,
    pub state: Reading<ChargeState>,
    pub percent: Reading<f64>,
    pub energy_now_wh: Reading<f64>,
    pub energy_full_wh: Reading<f64>,
    pub energy_design_wh: Reading<f64>,
    pub charge_now_ah: Reading<f64>,
    pub charge_full_ah: Reading<f64>,
    pub charge_design_ah: Reading<f64>,
    pub voltage_v: Reading<f64>,
    pub current_a: Reading<f64>,
    /// Power draw: `power_now` when exposed, else voltage × current.
    pub power_w: Reading<f64>,
    pub cycle_count: Reading<u32>,
    /// Full / design capacity. An estimate from reported capacities.
    pub health_percent: Reading<f64>,
    /// Only from the battery's own sensor, never a nearby one.
    pub temperature_c: Reading<f64>,
    pub runtime: RuntimeEstimate,
    pub technology: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerSourceSample {
    pub name: String,
    pub kind: String,
    pub online: Reading<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatteryReport {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub batteries: Vec<BatterySample>,
    pub sources: Vec<PowerSourceSample>,
    /// True when any mains/USB-PD source supplies power.
    pub on_ac: Reading<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Cpu,
    Gpu,
    Storage,
    Battery,
    Memory,
    Wireless,
    Chassis,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationConfidence {
    /// No reliable association.
    None,
    /// From a firmware label (e.g. "CPU" on a laptop's embedded controller).
    Low,
    /// From the driver's device.
    Medium,
    /// A driver that only measures one kind of component (e.g. k10temp).
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Association {
    pub component: Component,
    pub confidence: AssociationConfidence,
    pub basis: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemperatureReading {
    /// e.g. `hwmon4/temp1` or `thermal_zone0`.
    pub id: String,
    pub chip: String,
    pub label: Option<String>,
    pub value_c: Reading<f64>,
    /// Limits reported by the driver, when it reports them.
    pub max_c: Option<f64>,
    pub critical_c: Option<f64>,
    pub association: Association,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanReading {
    pub id: String,
    pub chip: String,
    pub label: Option<String>,
    pub rpm: Reading<u32>,
    pub min_rpm: Option<u32>,
    pub max_rpm: Option<u32>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThermalReport {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub temperatures: Vec<TemperatureReading>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanReport {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub fans: Vec<FanReading>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuMemoryModel {
    /// Memory on the card.
    Dedicated,
    /// Integrated: "VRAM" is system RAM reserved for the GPU.
    Shared,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuSample {
    pub pci_address: String,
    pub driver: Option<String>,
    pub memory_model: GpuMemoryModel,
    pub busy_percent: Reading<f64>,
    /// Card memory (dedicated) or reserved system memory (shared) in use.
    pub vram_used_bytes: Reading<u64>,
    pub vram_total_bytes: Reading<u64>,
    /// Ordinary system memory the GPU has mapped (GTT).
    pub gtt_used_bytes: Reading<u64>,
    pub gtt_total_bytes: Reading<u64>,
    pub temperature_c: Reading<f64>,
    pub power_w: Reading<f64>,
    pub core_clock_mhz: Reading<u64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuReport {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub gpus: Vec<GpuSample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskIo {
    pub name: String,
    pub read_bps: Reading<f64>,
    pub write_bps: Reading<f64>,
    pub reads_per_s: Reading<f64>,
    pub writes_per_s: Reading<f64>,
    /// Share of time the device had I/O in flight (not a health measure).
    pub busy_percent: Reading<f64>,
    pub read_total_bytes: u64,
    pub written_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesystemUsage {
    pub mount_point: String,
    pub device: String,
    pub fs_type: String,
    pub total_bytes: Reading<u64>,
    pub used_bytes: Reading<u64>,
    pub available_bytes: Reading<u64>,
    pub used_percent: Reading<f64>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageReport {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub devices: Vec<DiskIo>,
    pub filesystems: Vec<FilesystemUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterfaceTraffic {
    pub name: String,
    pub operational_state: Option<String>,
    pub loopback: bool,
    pub rx_bps: Reading<f64>,
    pub tx_bps: Reading<f64>,
    pub rx_packets_per_s: Reading<f64>,
    pub tx_packets_per_s: Reading<f64>,
    pub rx_total_bytes: u64,
    pub tx_total_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkReport {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub interfaces: Vec<InterfaceTraffic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemLoad {
    pub measured_at: u64,
    pub interval_ms: u64,
    pub load1: Reading<f64>,
    pub load5: Reading<f64>,
    pub load15: Reading<f64>,
    pub runnable_tasks: Reading<u32>,
    pub total_tasks: Reading<u32>,
    pub uptime_s: Reading<u64>,
    pub cpu_pressure: Pressure,
    pub io_pressure: Pressure,
}

// -------------------------------------------------------------- health

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthLevel {
    Unknown,
    Normal,
    Elevated,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthArea {
    CpuPressure,
    MemoryPressure,
    SwapPressure,
    Thermal,
    Battery,
    StorageCapacity,
    StorageActivity,
    NetworkActivity,
    SensorAvailability,
}

/// One understandable status with the evidence behind it. No combined
/// score is produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthIndicator {
    pub area: HealthArea,
    pub level: HealthLevel,
    pub summary: String,
    pub evidence: Vec<String>,
}

// ------------------------------------------------------------ snapshot

/// The latest result of every collector, each with its own timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub cpu: Option<CpuSample>,
    pub memory: Option<MemorySample>,
    pub battery: Option<BatteryReport>,
    pub thermal: Option<ThermalReport>,
    pub fans: Option<FanReport>,
    pub gpu: Option<GpuReport>,
    pub storage: Option<StorageReport>,
    pub network: Option<NetworkReport>,
    pub load: Option<SystemLoad>,
    pub health: Vec<HealthIndicator>,
    /// Top processes by CPU and memory (full list: GetProcessMetrics).
    pub top_processes: Vec<ProcessSample>,
}

impl TelemetrySnapshot {
    pub fn empty() -> Self {
        Self {
            cpu: None,
            memory: None,
            battery: None,
            thermal: None,
            fans: None,
            gpu: None,
            storage: None,
            network: None,
            load: None,
            health: Vec::new(),
            top_processes: Vec::new(),
        }
    }
}

// --------------------------------------------------------- monitoring

/// Monitoring lifecycle, as the user sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringPhase {
    Off,
    Starting,
    Active,
    Paused,
    Stopping,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitoringReport {
    pub phase: MonitoringPhase,
    /// Unix milliseconds the phase was entered.
    pub since: u64,
    pub sampling: SamplingInfo,
    pub collectors: Vec<CollectorStatus>,
    pub last_sample_at: Option<u64>,
    pub error: Option<String>,
    /// Whether the user's saved preference is to monitor (survives restart).
    pub enabled_preference: bool,
    pub history_available: bool,
    pub notifications: NotificationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationStatus {
    pub enabled: bool,
    /// `None` until a notification has been attempted.
    pub last_delivery_ok: Option<bool>,
    pub last_error: Option<String>,
    pub delivered: u32,
    pub failed: u32,
}

// --------------------------------------------------------------- alerts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertCategory {
    Cpu,
    Memory,
    Battery,
    Storage,
    Thermal,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertStatus {
    Active,
    Resolved,
}

/// A condition that held long enough to matter. Recommendations are text;
/// there is no action attached to an alert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub id: u64,
    pub rule: String,
    pub category: AlertCategory,
    pub severity: AlertSeverity,
    pub title: String,
    pub explanation: String,
    pub recommendation: Option<String>,
    /// History series the evidence comes from, e.g. `cpu.total`.
    pub metric: String,
    /// Device, sensor, mount point or battery the alert concerns.
    pub subject: Option<String>,
    pub threshold: String,
    /// Plain-language evidence: the values that triggered it.
    pub evidence: String,
    pub detected_at: u64,
    pub last_updated_at: u64,
    pub resolved_at: Option<u64>,
    pub status: AlertStatus,
    pub acknowledged: bool,
    pub notified: bool,
    /// Whether the threshold is the user's own or a driver-reported limit.
    pub threshold_source: ThresholdSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdSource {
    /// Sentinel's documented default; adjustable.
    Default,
    /// Set by the user.
    User,
    /// Reported by the hardware driver (e.g. `temp1_crit`).
    Driver,
}

// -------------------------------------------------------------- history

/// Identifier of a history series, e.g. `cpu.total`, `cpu.core.3`,
/// `net.wlp2s0.rx_bps`. Validated: short, and only `[A-Za-z0-9._:/@-]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SeriesKey(String);

impl SeriesKey {
    pub const MAX_LEN: usize = 96;

    pub fn new(key: impl Into<String>) -> Result<Self, String> {
        let key = key.into();
        if key.is_empty()
            || key.len() > Self::MAX_LEN
            || !key.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '@' | '-')
            })
        {
            return Err(format!("invalid series key {key:?}"));
        }
        Ok(Self(key))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SeriesKey {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SeriesKey> for String {
    fn from(key: SeriesKey) -> String {
        key.0
    }
}

impl std::fmt::Display for SeriesKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeWindow {
    /// From the in-memory buffer at full resolution.
    Minutes5,
    Minutes15,
    /// From the database at 30 s resolution.
    Hour1,
    Hours24,
    /// From the hourly aggregates.
    Days7,
    Days30,
}

impl TimeWindow {
    pub fn seconds(self) -> u64 {
        match self {
            TimeWindow::Minutes5 => 300,
            TimeWindow::Minutes15 => 900,
            TimeWindow::Hour1 => 3_600,
            TimeWindow::Hours24 => 86_400,
            TimeWindow::Days7 => 7 * 86_400,
            TimeWindow::Days30 => 30 * 86_400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub series: Vec<SeriesKey>,
    pub window: TimeWindow,
}

impl HistoryQuery {
    pub const MAX_SERIES: usize = 32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryResolution {
    /// Every sample, from memory.
    Raw,
    /// 30-second averages.
    Detailed,
    /// Hourly averages.
    Hourly,
}

/// One point: unix ms and the value, or `None` for a gap (collection
/// failed or monitoring was off). Gaps are never interpolated.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub t: u64,
    pub v: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub key: SeriesKey,
    pub unit: String,
    /// Where the values come from, e.g. `/proc/stat`. Empty when unknown
    /// (no data for this series yet).
    #[serde(default)]
    pub source: String,
    /// How the values were obtained. `None` when unknown.
    #[serde(default)]
    pub quality: Option<Quality>,
    pub resolution: HistoryResolution,
    pub points: Vec<SeriesPoint>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_readings_never_hold_values() {
        let r: Reading<f64> = Reading::missing(ReadingState::Ok);
        assert_eq!(r.state, ReadingState::Unknown);
        assert!(r.value.is_none());
        let json =
            serde_json::to_value(Reading::<f64>::missing(ReadingState::PermissionDenied)).unwrap();
        assert!(json.get("value").is_none(), "absent, not zero: {json}");
        assert_eq!(json["state"], "permission_denied");
        // A real zero is a value.
        let zero = serde_json::to_value(Reading::direct(0.0)).unwrap();
        assert_eq!(zero["value"], 0.0);
    }

    #[test]
    fn series_keys_are_validated() {
        assert!(SeriesKey::new("cpu.core.3").is_ok());
        assert!(SeriesKey::new("temp.hwmon4/temp1").is_ok());
        assert!(SeriesKey::new("net.wlp2s0.rx_bps").is_ok());
        for bad in ["", "a b", "x;rm", &"a".repeat(97), "../x\n"] {
            assert!(SeriesKey::new(bad).is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<SeriesKey>("\"bad key\"").is_err());
    }

    #[test]
    fn rounding() {
        assert_eq!(round1(12.345), 12.3);
        assert_eq!(round2(12.3456), 12.35);
    }
}
