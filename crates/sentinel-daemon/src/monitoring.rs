//! Monitoring: the telemetry engine running on its own thread.
//!
//! The monitor thread exists only while monitoring is on. It sleeps until
//! the scheduler's next due time (or until a command arrives), runs the due
//! collectors, evaluates alerts, delivers notifications, and hands rows to
//! the history database in batches. With monitoring off there is no thread,
//! no timer and no wake-up.
//!
//! Commands (start, stop, pause, resume) are acknowledged only once the
//! thread reports that the requested state holds.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sentinel_alerts::{
    AlertEngine, AlertEvent, DbusNotifier, NotificationService, Notifier, health,
};
use sentinel_config::{AlertThresholds, PowerMode, Retention, SentinelConfig};
use sentinel_core::{MonitoringCommand, MonitoringState};
use sentinel_hardware::SysRoot;
use sentinel_history::History;
use sentinel_ipc::{ErrorCode, IpcError, MonitoringStatus};
use sentinel_telemetry::collectors::SampleTime;
use sentinel_telemetry::collectors::process::ProcessCollector;
use sentinel_telemetry::collectors::storage::StorageCollector;
use sentinel_telemetry::model::*;
use sentinel_telemetry::series::{Bucketer, HistoryRow, LiveBuffers, SeriesMeta};
use sentinel_telemetry::{Engine, Scheduler, scheduler};

/// Points kept per series in memory (15 minutes at 1 s).
pub const LIVE_POINTS: usize = 900;
pub const LIVE_SERIES: usize = 512;
/// Width of a history bucket.
pub const BUCKET_MS: u64 = 30_000;
/// History rows are written at most this often...
pub const FLUSH_INTERVAL_S: f64 = 300.0;
/// ...or when this many are waiting.
pub const FLUSH_ROWS: usize = 4_000;
pub const MAINTENANCE_INTERVAL_S: f64 = 3_600.0;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
/// The wall clock moving this much more (or less) than the monotonic clock
/// between two passes means the system slept, or its time was set.
pub const CLOCK_JUMP_MS: i64 = 5_000;

/// How far the wall clock moved beyond the monotonic clock between two
/// passes, when that exceeds [`CLOCK_JUMP_MS`]. The monotonic clock stops
/// while the system is suspended; the wall clock does not.
fn clock_jump(prev_wall_ms: u64, prev_mono_s: f64, wall_ms: u64, mono_s: f64) -> Option<i64> {
    let wall = wall_ms as i64 - prev_wall_ms as i64;
    let mono = ((mono_s - prev_mono_s) * 1000.0).round() as i64;
    let drift = wall - mono;
    (drift.abs() > CLOCK_JUMP_MS).then_some(drift)
}

pub fn profile_of(mode: PowerMode) -> SamplingProfile {
    match mode {
        PowerMode::Eco => SamplingProfile::Eco,
        PowerMode::Balanced => SamplingProfile::Balanced,
        PowerMode::Performance => SamplingProfile::Performance,
    }
}

pub fn mode_of(profile: SamplingProfile) -> PowerMode {
    match profile {
        SamplingProfile::Eco => PowerMode::Eco,
        SamplingProfile::Balanced => PowerMode::Balanced,
        SamplingProfile::Performance => PowerMode::Performance,
    }
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Settings the monitor thread follows; updated live.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorSettings {
    pub profile: SamplingProfile,
    pub power_aware: bool,
    pub thresholds: AlertThresholds,
    pub notifications: bool,
    pub retention: Retention,
}

impl MonitorSettings {
    pub fn from_config(c: &SentinelConfig) -> Self {
        Self {
            profile: profile_of(c.power_mode),
            power_aware: c.power_aware_sampling,
            thresholds: c.alerts.clone(),
            notifications: c.notifications_enabled,
            retention: c.retention,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Stop,
    Pause,
    Resume,
}

/// Where the monitor reads and writes.
#[derive(Clone)]
pub struct MonitorConfig {
    /// `/`, or a fixture tree in tests.
    pub root: PathBuf,
    /// `None` when history cannot be stored.
    pub history_path: Option<PathBuf>,
}

/// State shared between the IPC handler and the monitor thread.
pub struct Shared {
    phase: MonitoringPhase,
    since: u64,
    error: Option<String>,
    command: Option<Command>,
    settings: MonitorSettings,
    settings_changed: bool,
    process_interest: bool,
    snapshot: TelemetrySnapshot,
    processes: Option<ProcessList>,
    collectors: Vec<CollectorStatus>,
    sampling: SamplingInfo,
    last_sample_at: Option<u64>,
    live: LiveBuffers,
    active_alerts: Vec<Alert>,
    recent_alerts: Vec<Alert>,
    acknowledgements: Vec<u64>,
    notifications: NotificationStatus,
    history_available: bool,
    history_rows_written: u64,
    history_flushes: u64,
}

fn sampling_info(
    selected: SamplingProfile,
    effective: SamplingProfile,
    power_aware: bool,
    reason: Option<String>,
) -> SamplingInfo {
    SamplingInfo {
        selected,
        effective,
        power_aware,
        reason,
        intervals_ms: CollectorKind::ALL
            .iter()
            .map(|k| (*k, scheduler::profile_interval_ms(effective, *k)))
            .collect(),
    }
}

pub struct Monitor {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    thread: Option<JoinHandle<()>>,
    config: MonitorConfig,
    notifier: Arc<Mutex<Option<Box<dyn Notifier>>>>,
}

fn lock(shared: &Arc<(Mutex<Shared>, Condvar)>) -> MutexGuard<'_, Shared> {
    shared.0.lock().unwrap_or_else(|p| p.into_inner())
}

impl Monitor {
    pub fn new(config: MonitorConfig, settings: MonitorSettings) -> Self {
        let sampling = sampling_info(
            settings.profile,
            settings.profile,
            settings.power_aware,
            None,
        );
        let notifications = NotificationStatus {
            enabled: settings.notifications,
            last_delivery_ok: None,
            last_error: None,
            delivered: 0,
            failed: 0,
        };
        let history_available = config.history_path.is_some();
        Self {
            shared: Arc::new((
                Mutex::new(Shared {
                    phase: MonitoringPhase::Off,
                    since: wall_ms(),
                    error: None,
                    command: None,
                    settings,
                    settings_changed: false,
                    process_interest: false,
                    snapshot: TelemetrySnapshot::empty(),
                    processes: None,
                    collectors: Vec::new(),
                    sampling,
                    last_sample_at: None,
                    live: LiveBuffers::new(LIVE_POINTS, LIVE_SERIES),
                    active_alerts: Vec::new(),
                    recent_alerts: Vec::new(),
                    acknowledgements: Vec::new(),
                    notifications,
                    history_available,
                    history_rows_written: 0,
                    history_flushes: 0,
                }),
                Condvar::new(),
            )),
            thread: None,
            config,
            notifier: Arc::new(Mutex::new(None)),
        }
    }

    /// Use a different notifier (tests). The default is the session bus.
    pub fn set_notifier(&mut self, notifier: Box<dyn Notifier>) {
        *self.notifier.lock().unwrap_or_else(|p| p.into_inner()) = Some(notifier);
    }

    pub fn phase(&self) -> MonitoringPhase {
        lock(&self.shared).phase
    }

    pub fn state(&self) -> MonitoringState {
        match self.phase() {
            MonitoringPhase::Active | MonitoringPhase::Starting => MonitoringState::Running,
            MonitoringPhase::Paused => MonitoringState::Paused,
            _ => MonitoringState::Stopped,
        }
    }

    pub fn status(&self) -> MonitoringStatus {
        let s = lock(&self.shared);
        let note = match s.phase {
            MonitoringPhase::Off => "Monitoring is off. Nothing is being measured.".to_string(),
            MonitoringPhase::Starting => "Monitoring is starting.".to_string(),
            MonitoringPhase::Active => format!(
                "Monitoring is active ({:?} sampling).",
                s.sampling.effective
            )
            .to_lowercase(),
            MonitoringPhase::Paused => {
                "Monitoring is paused. Nothing is being measured.".to_string()
            }
            MonitoringPhase::Stopping => "Monitoring is stopping.".to_string(),
            MonitoringPhase::Error => {
                format!("Monitoring failed: {}", s.error.clone().unwrap_or_default())
            }
        };
        MonitoringStatus {
            state: self.state_from(s.phase),
            engine_available: true,
            note,
        }
    }

    fn state_from(&self, phase: MonitoringPhase) -> MonitoringState {
        match phase {
            MonitoringPhase::Active | MonitoringPhase::Starting => MonitoringState::Running,
            MonitoringPhase::Paused => MonitoringState::Paused,
            _ => MonitoringState::Stopped,
        }
    }

    pub fn report(&self, enabled_preference: bool) -> MonitoringReport {
        let s = lock(&self.shared);
        MonitoringReport {
            phase: s.phase,
            since: s.since,
            sampling: s.sampling.clone(),
            collectors: s.collectors.clone(),
            last_sample_at: s.last_sample_at,
            error: s.error.clone(),
            enabled_preference,
            history_available: s.history_available,
            notifications: s.notifications.clone(),
        }
    }

    /// Live snapshot, only while monitoring is active or paused.
    pub fn snapshot(&self) -> Option<TelemetrySnapshot> {
        let s = lock(&self.shared);
        matches!(s.phase, MonitoringPhase::Active | MonitoringPhase::Paused)
            .then(|| s.snapshot.clone())
    }

    /// Process list; asking for it makes the process collector run at full
    /// rate for the next minute.
    pub fn processes(&self) -> Option<ProcessList> {
        let mut s = lock(&self.shared);
        s.process_interest = true;
        self.shared.1.notify_all();
        matches!(s.phase, MonitoringPhase::Active | MonitoringPhase::Paused)
            .then(|| s.processes.clone())
            .flatten()
    }

    pub fn active_alerts(&self) -> Vec<Alert> {
        lock(&self.shared).active_alerts.clone()
    }

    pub fn recent_alerts(&self) -> Vec<Alert> {
        let s = lock(&self.shared);
        let mut all = s.active_alerts.clone();
        all.extend(s.recent_alerts.iter().cloned());
        all
    }

    pub fn acknowledge(&self, id: u64) -> Option<Alert> {
        let mut guard = lock(&self.shared);
        let s = &mut *guard;
        let found = s
            .active_alerts
            .iter_mut()
            .chain(s.recent_alerts.iter_mut())
            .find(|a| a.id == id)
            .map(|a| {
                a.acknowledged = true;
                a.clone()
            });
        if found.is_some() {
            s.acknowledgements.push(id);
            self.shared.1.notify_all();
        }
        found
    }

    pub fn sampling(&self) -> SamplingInfo {
        lock(&self.shared).sampling.clone()
    }

    /// The sampling `settings` put in effect, decided exactly as the monitor
    /// thread will decide it (from the latest power-source reading). An
    /// answer to a profile change must not report the profile that was in
    /// effect before it, even if the thread has not woken up yet.
    pub fn sampling_for(&self, settings: &MonitorSettings) -> SamplingInfo {
        let s = lock(&self.shared);
        if s.phase == MonitoringPhase::Off {
            return s.sampling.clone();
        }
        let on_ac = s.snapshot.battery.as_ref().and_then(|b| b.on_ac.value());
        let (effective, reason) = select_profile(settings, on_ac);
        sampling_info(settings.profile, effective, settings.power_aware, reason)
    }

    pub fn live_series(
        &self,
        key: &SeriesKey,
        since_ms: u64,
    ) -> Option<(SeriesMeta, Vec<SeriesPoint>)> {
        lock(&self.shared).live.query(key, since_ms)
    }

    pub fn history_path(&self) -> Option<&PathBuf> {
        self.config.history_path.as_ref()
    }

    pub fn history_stats(&self) -> (u64, u64) {
        let s = lock(&self.shared);
        (s.history_rows_written, s.history_flushes)
    }

    pub fn apply_settings(&self, settings: MonitorSettings) {
        let mut s = lock(&self.shared);
        if s.settings != settings {
            s.notifications.enabled = settings.notifications;
            if s.phase == MonitoringPhase::Off {
                s.sampling = sampling_info(
                    settings.profile,
                    settings.profile,
                    settings.power_aware,
                    None,
                );
            }
            s.settings = settings;
            s.settings_changed = true;
            self.shared.1.notify_all();
        }
    }

    /// Wait until `done` holds (or the timeout passes).
    fn wait_for(&self, done: impl Fn(&Shared) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        let mut s = lock(&self.shared);
        while !done(&s) {
            let now = Instant::now();
            if now >= deadline {
                return Err("the monitor did not respond in time".into());
            }
            s = self
                .shared
                .1
                .wait_timeout(s, deadline - now)
                .map(|(g, _)| g)
                .unwrap_or_else(|p| p.into_inner().0);
        }
        match s.phase {
            MonitoringPhase::Error => Err(s
                .error
                .clone()
                .unwrap_or_else(|| "monitoring failed".into())),
            _ => Ok(()),
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        if let Some(t) = self.thread.take() {
            if !t.is_finished() {
                self.thread = Some(t);
                // One monitor thread at most, ever: a second would sample
                // and alert in parallel with the first.
                return Err(if lock(&self.shared).command == Some(Command::Stop) {
                    "monitoring is still stopping; try again shortly".into()
                } else {
                    "monitoring is already running".into()
                });
            }
            let _ = t.join();
        }
        {
            let mut s = lock(&self.shared);
            s.phase = MonitoringPhase::Starting;
            s.since = wall_ms();
            s.error = None;
            s.command = None;
        }
        let shared = self.shared.clone();
        let config = self.config.clone();
        let notifier = self
            .notifier
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let spawned = std::thread::Builder::new()
            .name("sentinel-monitor".into())
            .spawn(move || {
                let notifier = notifier.unwrap_or_else(|| {
                    Box::new(DbusNotifier::from_env(
                        sentinel_core::platform::current_uid(),
                    ))
                });
                run_monitor(&shared, &config, notifier);
            });
        match spawned {
            Ok(handle) => self.thread = Some(handle),
            Err(e) => {
                let mut s = lock(&self.shared);
                s.phase = MonitoringPhase::Error;
                s.error = Some(format!("could not start the monitor thread: {e}"));
                return Err(s.error.clone().unwrap_or_default());
            }
        }
        self.wait_for(|s| s.phase != MonitoringPhase::Starting)?;
        tracing::info!("monitoring started");
        Ok(())
    }

    fn command(&self, cmd: Command, until: impl Fn(&Shared) -> bool) -> Result<(), String> {
        {
            let mut s = lock(&self.shared);
            s.command = Some(cmd);
            self.shared.1.notify_all();
        }
        self.wait_for(until)
    }

    pub fn pause(&self) -> Result<(), String> {
        self.command(Command::Pause, |s| s.phase == MonitoringPhase::Paused)?;
        tracing::info!("monitoring paused");
        Ok(())
    }

    pub fn resume(&self) -> Result<(), String> {
        self.command(Command::Resume, |s| s.phase == MonitoringPhase::Active)?;
        tracing::info!("monitoring resumed");
        Ok(())
    }

    /// Stop and wait (bounded) for the thread to exit.
    pub fn stop(&mut self) -> Result<(), String> {
        let Some(thread) = self.thread.take() else {
            lock(&self.shared).phase = MonitoringPhase::Off;
            return Ok(());
        };
        let result = self.command(Command::Stop, |s| s.phase == MonitoringPhase::Off);
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if !thread.is_finished() {
            // Still inside a collector (a hung sensor read, say). Keep the
            // handle: dropping it would detach a thread that still holds a
            // Stop command, and a later start would clear that command and
            // run a second monitor beside it.
            self.thread = Some(thread);
            tracing::warn!(
                "the monitor thread has not exited yet; it will stop after its current read"
            );
            return Err("monitoring is still stopping; try again shortly".into());
        }
        let _ = thread.join();
        tracing::info!("monitoring stopped");
        result
    }

    /// Execute a lifecycle command, acknowledging only reached states.
    pub fn execute(&mut self, command: MonitoringCommand) -> Result<MonitoringStatus, IpcError> {
        let current = self.state();
        let target = current
            .after(command)
            .map_err(|e| IpcError::new(ErrorCode::InvalidStateTransition, e.to_string()))?;
        if target != current || self.phase() == MonitoringPhase::Error {
            let result = match command {
                MonitoringCommand::Start => self.start(),
                MonitoringCommand::Stop => self.stop(),
                MonitoringCommand::Pause => self.pause(),
                MonitoringCommand::Resume => self.resume(),
            };
            result.map_err(|reason| IpcError::new(ErrorCode::MonitoringUnavailable, reason))?;
        }
        Ok(self.status())
    }
}

// ------------------------------------------------------------ the thread

struct Thread {
    engine: Engine,
    scheduler: Scheduler,
    alerts: AlertEngine,
    notifications: NotificationService,
    history: Option<History>,
    bucketer: Bucketer,
    pending_rows: Vec<HistoryRow>,
    last_flush: f64,
    last_maintenance: f64,
    effective: SamplingProfile,
    started: Instant,
    root: PathBuf,
    /// Wall and monotonic time of the last sampling pass.
    last_pass: Option<(u64, f64)>,
}

impl Thread {
    fn mono(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn new_engine(root: &PathBuf) -> Engine {
        let sys = SysRoot::at(root);
        if sys.is_host() {
            Engine::for_host()
        } else {
            Engine::new(
                sys,
                ProcessCollector::for_host(),
                StorageCollector::for_host(),
            )
        }
    }

    /// Start sampling afresh after a gap the counters must not span (resume
    /// from sleep, a clock change): open buckets are closed, conditions
    /// that were only pending are forgotten, so no sustained-duration check
    /// counts the gap, and counters and schedule start over.
    fn restart_after_gap(&mut self, shared: &Arc<(Mutex<Shared>, Condvar)>) {
        lock(shared).live.mark_gap(wall_ms());
        let rows = self.bucketer.flush();
        self.pending_rows.extend(rows);
        self.alerts.evaluator.reset_pending();
        self.engine = Thread::new_engine(&self.root);
        let now = self.mono();
        self.scheduler = Scheduler::new(self.effective, now);
        self.last_pass = None;
    }

    fn flush(&mut self, force: bool, shared: &Arc<(Mutex<Shared>, Condvar)>) {
        let now = self.mono();
        if !force
            && self.pending_rows.len() < FLUSH_ROWS
            && now - self.last_flush < FLUSH_INTERVAL_S
        {
            return;
        }
        self.last_flush = now;
        if self.pending_rows.is_empty() {
            return;
        }
        let rows = std::mem::take(&mut self.pending_rows);
        if let Some(h) = self.history.as_mut() {
            match h.write_rows(&rows) {
                Ok(n) => {
                    let mut s = lock(shared);
                    s.history_rows_written += n as u64;
                    s.history_flushes += 1;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "writing telemetry history failed; continuing")
                }
            }
        }
    }
}

fn run_monitor(
    shared: &Arc<(Mutex<Shared>, Condvar)>,
    config: &MonitorConfig,
    notifier: Box<dyn Notifier>,
) {
    let settings = lock(shared).settings.clone();
    let history = config.history_path.as_ref().and_then(|p| match History::open(p) {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::warn!(error = %e, "telemetry history is unavailable; monitoring continues without it");
            None
        }
    });
    let first_id = history
        .as_ref()
        .and_then(|h| h.max_alert_id().ok())
        .unwrap_or(0)
        + 1;
    let mut alerts = AlertEngine::new(settings.thresholds.clone(), first_id);
    // Alerts left active by a previous session are resolved: their
    // condition is re-evaluated from scratch.
    let started = Instant::now();
    let mut t = Thread {
        engine: Thread::new_engine(&config.root),
        scheduler: Scheduler::new(settings.profile, 0.0),
        notifications: NotificationService::new(notifier, settings.notifications),
        history,
        bucketer: Bucketer::new(BUCKET_MS),
        pending_rows: Vec::new(),
        last_flush: 0.0,
        last_maintenance: f64::NEG_INFINITY,
        effective: settings.profile,
        started,
        root: config.root.clone(),
        last_pass: None,
        alerts: {
            alerts.evaluator.cooldown_ms = u64::from(settings.thresholds.cooldown_s) * 1000;
            alerts
        },
    };
    if let Some(h) = t.history.as_mut() {
        let now = wall_ms();
        for mut a in h.unresolved_alerts().unwrap_or_default() {
            a.status = AlertStatus::Resolved;
            a.resolved_at = Some(now);
            a.last_updated_at = now;
            a.evidence = format!(
                "{} Closed when monitoring restarted; the condition is checked again.",
                a.evidence
            );
            let _ = h.upsert_alert(&a);
        }
        // The cooldown outlives the session: a condition that resolved (or
        // was closed above) a moment ago is not notified again on return.
        let cooldown_from = now.saturating_sub(t.alerts.evaluator.cooldown_ms);
        for a in h.alerts(1_000).unwrap_or_default() {
            if let Some(at) = a.resolved_at.filter(|at| *at >= cooldown_from) {
                t.alerts
                    .evaluator
                    .remember_resolved(&a.rule, a.subject.clone(), at);
            }
        }
    }
    {
        // The phase becomes Active when the first round of samples is
        // published below, so "active" always comes with data.
        let mut s = lock(shared);
        s.history_available = t.history.is_some();
        s.sampling = sampling_info(
            settings.profile,
            settings.profile,
            settings.power_aware,
            None,
        );
        shared.1.notify_all();
    }

    loop {
        // ---- commands and settings
        let mut s = lock(shared);
        match s.command.take() {
            Some(Command::Stop) => break,
            Some(Command::Pause) => {
                s.phase = MonitoringPhase::Paused;
                s.since = wall_ms();
                s.live.mark_gap(wall_ms());
                shared.1.notify_all();
                drop(s);
                // Close open buckets so paused time is not averaged in.
                let rows = t.bucketer.flush();
                t.pending_rows.extend(rows);
                t.flush(true, shared);
                t.alerts.evaluator.reset_pending();
                // Sleep until a command arrives: no timer while paused.
                let mut s = lock(shared);
                while s.command.is_none() {
                    s = shared.1.wait(s).unwrap_or_else(|p| p.into_inner());
                }
                if s.command == Some(Command::Resume) {
                    s.command = None;
                    s.phase = MonitoringPhase::Active;
                    s.since = wall_ms();
                    shared.1.notify_all();
                    // Fresh counters: rates must not span the paused time.
                    t.engine = Thread::new_engine(&t.root);
                    let now = t.mono();
                    t.scheduler = Scheduler::new(t.effective, now);
                    t.last_pass = None;
                }
                continue;
            }
            Some(Command::Resume) | None => {}
        }
        if s.settings_changed {
            s.settings_changed = false;
            let settings = s.settings.clone();
            t.alerts.set_thresholds(settings.thresholds.clone());
            t.notifications.set_enabled(settings.notifications);
            s.notifications.enabled = settings.notifications;
            drop(s);
            let (effective, reason) = effective_profile(&settings, &t.engine);
            t.effective = effective;
            let now = t.mono();
            t.scheduler.set_profile(effective, now);
            let mut s = lock(shared);
            s.sampling = sampling_info(settings.profile, effective, settings.power_aware, reason);
            s.settings_changed = false;
            continue;
        }
        if std::mem::take(&mut s.process_interest) {
            let now = t.mono();
            t.scheduler.note_process_interest(now);
        }
        // ---- acknowledgements: applied as soon as they arrive, whether or
        // not a collector is due, and recorded so they survive a restart.
        let acks = std::mem::take(&mut s.acknowledgements);
        if !acks.is_empty() {
            drop(s);
            for id in acks {
                if let Some(alert) = t.alerts.evaluator.acknowledge(id)
                    && let Some(h) = t.history.as_mut()
                    && let Err(e) = h.upsert_alert(&alert)
                {
                    tracing::warn!(error = %e, "recording an acknowledgement failed");
                }
            }
            continue;
        }
        // ---- wait for the next due collector
        let now = t.mono();
        let due = t.scheduler.due(now);
        if due.is_empty() {
            let wait = (t.scheduler.next_wake() - now).clamp(0.001, 3_600.0);
            let _ = shared.1.wait_timeout(s, Duration::from_secs_f64(wait));
            continue;
        }
        drop(s);

        // ---- resumed from sleep, or the clock was set
        let (pass_wall, pass_mono) = (wall_ms(), t.mono());
        if let Some(jump) = t
            .last_pass
            .and_then(|(w, m)| clock_jump(w, m, pass_wall, pass_mono))
        {
            tracing::info!(
                jump_s = jump / 1000,
                "the system resumed from sleep or its clock changed; sampling restarts"
            );
            t.restart_after_gap(shared);
            continue;
        }
        t.last_pass = Some((pass_wall, pass_mono));

        // ---- sample
        let mut points = Vec::new();
        let mut battery_ran = false;
        for kind in due {
            let now = t.mono();
            let interval_ms = t.scheduler.interval_ms(kind, now);
            let time = SampleTime {
                wall_ms: wall_ms(),
                mono_s: now,
                interval_ms,
            };
            let result = t.engine.run(kind, time);
            t.scheduler.record(kind, result.health, t.mono());
            t.engine
                .set_interval(kind, t.scheduler.interval_ms(kind, t.mono()));
            battery_ran |= kind == CollectorKind::Battery;
            for p in result.points {
                points.push((time.wall_ms, p));
            }
        }
        for (wall, p) in &points {
            if let Some(row) = t.bucketer.push(*wall, p.clone()) {
                t.pending_rows.push(row);
            }
        }

        // ---- power-aware sampling
        let settings = lock(shared).settings.clone();
        let mut sampling_update = None;
        if battery_ran {
            let (effective, reason) = effective_profile(&settings, &t.engine);
            if effective != t.effective {
                tracing::info!(?effective, "sampling profile adjusted for the power source");
                t.effective = effective;
                let now = t.mono();
                t.scheduler.set_profile(effective, now);
            }
            sampling_update = Some(sampling_info(
                settings.profile,
                effective,
                settings.power_aware,
                reason,
            ));
        }

        // ---- alerts and notifications
        let now_wall = wall_ms();
        let mut snapshot = t.engine.snapshot();
        let events = t.alerts.evaluate(&snapshot, now_wall);
        for event in &events {
            let (alert, notify) = match event {
                AlertEvent::Raised { alert, notify } | AlertEvent::Escalated { alert, notify } => {
                    (alert, *notify)
                }
                AlertEvent::Resolved { alert } => (alert, false),
            };
            tracing::info!(rule = %alert.rule, severity = ?alert.severity, status = ?alert.status, "alert");
            if notify {
                let delivered = t.notifications.deliver(alert);
                if !delivered && t.notifications.status.enabled {
                    tracing::warn!(rule = %alert.rule, "notification could not be delivered; recorded locally");
                }
            }
            if let Some(h) = t.history.as_mut()
                && let Err(e) = h.upsert_alert(alert)
            {
                tracing::warn!(error = %e, "recording an alert failed");
            }
        }
        let collectors = t.engine.statuses();
        let active = t.alerts.evaluator.active();
        snapshot.health =
            health::indicators(&snapshot, t.alerts.thresholds(), &active, &collectors);

        // ---- publish
        {
            let mut s = lock(shared);
            for (wall, p) in points {
                s.live.push(wall, p);
            }
            if let Some(list) = t.engine.process_list() {
                let keep: Vec<u32> = list.processes.iter().map(|p| p.pid).collect();
                s.live.retain_processes(&keep);
                s.processes = Some(list.clone());
            }
            s.snapshot = snapshot;
            s.collectors = collectors;
            s.active_alerts = active;
            s.recent_alerts = t.alerts.evaluator.recently_resolved();
            s.notifications.delivered = t.notifications.status.delivered;
            s.notifications.failed = t.notifications.status.failed;
            s.notifications.last_delivery_ok = t.notifications.status.last_delivery_ok;
            s.notifications.last_error = t.notifications.status.last_error.clone();
            s.last_sample_at = Some(now_wall);
            if let Some(info) = sampling_update {
                s.sampling = info;
            }
            if s.phase == MonitoringPhase::Starting {
                s.phase = MonitoringPhase::Active;
                s.since = now_wall;
                shared.1.notify_all();
            }
        }

        // ---- history
        t.flush(false, shared);
        let now = t.mono();
        if now - t.last_maintenance >= MAINTENANCE_INTERVAL_S {
            t.last_maintenance = now;
            if let Some(h) = t.history.as_mut() {
                match h.maintain(wall_ms(), &settings.retention) {
                    Ok(m) => tracing::info!(
                        hours = m.hours_rolled_up,
                        detailed_deleted = m.detailed_deleted,
                        hourly_deleted = m.hourly_deleted,
                        alerts_deleted = m.alerts_deleted,
                        "history maintenance"
                    ),
                    Err(e) => tracing::warn!(error = %e, "history maintenance failed"),
                }
            }
        }
    }

    // ---- stop
    let rows = t.bucketer.flush();
    t.pending_rows.extend(rows);
    t.flush(true, shared);
    let mut s = lock(shared);
    s.live.mark_gap(wall_ms());
    s.phase = MonitoringPhase::Off;
    s.since = wall_ms();
    s.snapshot = TelemetrySnapshot::empty();
    s.processes = None;
    s.collectors = Vec::new();
    s.last_sample_at = None;
    shared.1.notify_all();
}

/// Eco when power-aware sampling is on and the computer runs on battery.
fn effective_profile(
    settings: &MonitorSettings,
    engine: &Engine,
) -> (SamplingProfile, Option<String>) {
    select_profile(
        settings,
        engine
            .latest()
            .battery
            .as_ref()
            .and_then(|b| b.on_ac.value()),
    )
}

/// `on_ac`: `None` when the power source is not known; then the chosen
/// profile is kept, since running on battery was not established.
fn select_profile(
    settings: &MonitorSettings,
    on_ac: Option<bool>,
) -> (SamplingProfile, Option<String>) {
    if settings.power_aware && on_ac == Some(false) && settings.profile != SamplingProfile::Eco {
        (
            SamplingProfile::Eco,
            Some("Power-aware sampling: running on battery, so the Eco profile is in use.".into()),
        )
    } else {
        (settings.profile, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(profile: SamplingProfile, power_aware: bool) -> MonitorSettings {
        MonitorSettings {
            profile,
            power_aware,
            ..MonitorSettings::from_config(&SentinelConfig::default())
        }
    }

    #[test]
    fn sampling_profile_selection() {
        use SamplingProfile::*;
        // Power-aware and on battery: Eco, with the reason.
        let (p, reason) = select_profile(&settings(Performance, true), Some(false));
        assert_eq!(p, Eco);
        assert!(reason.unwrap().contains("battery"));
        assert_eq!(
            select_profile(&settings(Balanced, true), Some(false)).0,
            Eco
        );
        // Eco already: no reason needed.
        assert_eq!(
            select_profile(&settings(Eco, true), Some(false)),
            (Eco, None)
        );
        // On AC, unknown power source, or power-aware off: the user's choice.
        assert_eq!(
            select_profile(&settings(Performance, true), Some(true)),
            (Performance, None)
        );
        assert_eq!(
            select_profile(&settings(Performance, true), None),
            (Performance, None)
        );
        assert_eq!(
            select_profile(&settings(Performance, false), Some(false)),
            (Performance, None)
        );
    }

    /// A collector stuck in a read (a FIFO here, a hung sensor in real
    /// life) must not let `stop` forget the thread, or the next `start`
    /// would run a second monitor beside the first.
    #[test]
    fn a_hung_collector_never_leads_to_a_second_monitor_thread() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let proc_dir = dir.path().join("proc");
        std::fs::create_dir_all(&proc_dir).unwrap();
        std::fs::write(proc_dir.join("loadavg"), "0.10 0.20 0.30 1/100 42\n").unwrap();
        let mut m = Monitor::new(
            MonitorConfig {
                root: dir.path().to_path_buf(),
                history_path: None,
            },
            settings(SamplingProfile::Performance, false),
        );
        m.start().unwrap();

        // From now on reading the load average blocks until someone writes.
        let fifo = proc_dir.join("loadavg.fifo");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )
        .unwrap();
        std::fs::rename(&fifo, proc_dir.join("loadavg")).unwrap();
        std::thread::sleep(Duration::from_millis(2_500)); // past the next 1 s run

        let err = m.stop().unwrap_err();
        assert!(err.contains("still stopping"), "{err}");
        assert!(m.thread.is_some(), "the stuck thread must stay owned");
        let err = m.start().unwrap_err();
        assert!(err.contains("still stopping"), "{err}");

        // The read completes; the thread sees its Stop and exits.
        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .open(proc_dir.join("loadavg"))
            .unwrap();
        writer.write_all(b"0.10 0.20 0.30 1/100 42\n").unwrap();
        drop(writer);
        m.stop().unwrap();
        assert!(m.thread.is_none());
        assert_eq!(m.state(), MonitoringState::Stopped);

        // And monitoring can be started again, once.
        std::fs::remove_file(proc_dir.join("loadavg")).unwrap();
        m.start().unwrap();
        assert!(m.start().unwrap_err().contains("already running"));
        m.stop().unwrap();
    }

    #[test]
    fn sleep_and_clock_changes_are_detected() {
        // Normal passes: both clocks advance together, with jitter.
        assert_eq!(clock_jump(1_000_000, 10.0, 1_002_003, 12.0), None);
        assert_eq!(clock_jump(1_000_000, 10.0, 1_004_900, 10.0), None);
        // Suspended for an hour: the monotonic clock did not move.
        assert_eq!(
            clock_jump(1_000_000, 10.0, 1_000_000 + 3_602_000, 12.0),
            Some(3_600_000)
        );
        // The clock was set back by a minute.
        assert_eq!(
            clock_jump(1_000_000, 10.0, 1_000_000 - 58_000, 12.0),
            Some(-60_000)
        );
    }

    #[test]
    fn power_modes_map_to_profiles_and_back() {
        for mode in [PowerMode::Eco, PowerMode::Balanced, PowerMode::Performance] {
            assert_eq!(mode_of(profile_of(mode)), mode);
        }
    }

    #[test]
    fn settings_follow_the_configuration() {
        let config = SentinelConfig {
            power_mode: PowerMode::Performance,
            power_aware_sampling: true,
            notifications_enabled: false,
            ..Default::default()
        };
        let s = MonitorSettings::from_config(&config);
        assert_eq!(s.profile, SamplingProfile::Performance);
        assert!(s.power_aware && !s.notifications);
        assert_eq!(s.retention, config.retention);
    }
}
