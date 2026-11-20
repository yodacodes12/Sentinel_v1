//! The telemetry engine: owns the collectors, runs one on request, keeps
//! the latest section of each and reports per-collector status. It is
//! driven by the daemon's monitor thread according to the [`Scheduler`].
//!
//! [`Scheduler`]: crate::scheduler::Scheduler

use std::collections::BTreeMap;
use std::time::Instant;

use sentinel_hardware::SysRoot;

use crate::collectors::battery::BatteryCollector;
use crate::collectors::cpu::CpuCollector;
use crate::collectors::memory::MemoryCollector;
use crate::collectors::network::NetworkCollector;
use crate::collectors::process::ProcessCollector;
use crate::collectors::storage::StorageCollector;
use crate::collectors::{RunReport, SampleTime, gpu, system_load, thermal};
use crate::model::*;
use crate::series::{self, Point};

/// Processes kept in the snapshot's top list.
pub const TOP_PROCESSES: usize = 8;
/// Processes whose CPU and memory are kept as live series.
pub const TRACKED_PROCESSES: usize = 10;

pub struct Engine {
    root: SysRoot,
    cpu: CpuCollector,
    memory: MemoryCollector,
    processes: ProcessCollector,
    battery: BatteryCollector,
    storage: StorageCollector,
    network: NetworkCollector,
    latest: TelemetrySnapshot,
    process_list: Option<ProcessList>,
    status: BTreeMap<CollectorKind, CollectorStatus>,
}

/// Result of running one collector.
pub struct RunResult {
    pub kind: CollectorKind,
    pub health: CollectorHealth,
    pub points: Vec<Point>,
}

impl Engine {
    pub fn new(root: SysRoot, processes: ProcessCollector, storage: StorageCollector) -> Self {
        Self {
            root,
            cpu: CpuCollector::default(),
            memory: MemoryCollector::default(),
            processes,
            battery: BatteryCollector::default(),
            storage,
            network: NetworkCollector::default(),
            latest: TelemetrySnapshot::empty(),
            process_list: None,
            status: CollectorKind::ALL
                .iter()
                .map(|k| {
                    (
                        *k,
                        CollectorStatus {
                            kind: *k,
                            health: CollectorHealth::Waiting,
                            interval_ms: 0,
                            last_sample_at: None,
                            last_duration_us: None,
                            consecutive_failures: 0,
                            note: None,
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn for_host() -> Self {
        Self::new(
            SysRoot::host(),
            ProcessCollector::for_host(),
            StorageCollector::for_host(),
        )
    }

    /// Run one collector. Never panics on bad input: every problem becomes
    /// a collector status and reading states.
    pub fn run(&mut self, kind: CollectorKind, t: SampleTime) -> RunResult {
        let started = Instant::now();
        let root = &self.root;
        let (points, report): (Vec<Point>, RunReport) = match kind {
            CollectorKind::Cpu => {
                let (s, r) = self.cpu.sample(root, t);
                let p = s.as_ref().map(series::from_cpu).unwrap_or_default();
                if s.is_some() {
                    self.latest.cpu = s;
                }
                (p, r)
            }
            CollectorKind::Memory => {
                let (s, r) = self.memory.sample(root, t);
                let p = s.as_ref().map(series::from_memory).unwrap_or_default();
                if s.is_some() {
                    self.latest.memory = s;
                }
                (p, r)
            }
            CollectorKind::SystemLoad => {
                let (s, r) = system_load::sample(root, t);
                let p = s.as_ref().map(series::from_load).unwrap_or_default();
                if s.is_some() {
                    self.latest.load = s;
                }
                (p, r)
            }
            CollectorKind::Processes => {
                let (s, r) = self.processes.sample(root, t);
                let mut p = Vec::new();
                if let Some(list) = s {
                    p = series::from_processes(&list, TRACKED_PROCESSES);
                    self.latest.top_processes = top_processes(&list, TOP_PROCESSES);
                    self.process_list = Some(list);
                }
                (p, r)
            }
            CollectorKind::Battery => {
                let (s, r) = self.battery.sample(root, t);
                let p = s.as_ref().map(series::from_battery).unwrap_or_default();
                if s.is_some() {
                    self.latest.battery = s;
                }
                (p, r)
            }
            CollectorKind::Thermal => {
                let (s, r) = thermal::sample_temperatures(root, t);
                let p = s.as_ref().map(series::from_thermal).unwrap_or_default();
                if s.is_some() {
                    self.latest.thermal = s;
                }
                (p, r)
            }
            CollectorKind::Fans => {
                let (s, r) = thermal::sample_fans(root, t);
                let p = s.as_ref().map(series::from_fans).unwrap_or_default();
                if s.is_some() {
                    self.latest.fans = s;
                }
                (p, r)
            }
            CollectorKind::Gpu => {
                let (s, r) = gpu::sample(root, t);
                let p = s.as_ref().map(series::from_gpu).unwrap_or_default();
                if s.is_some() {
                    self.latest.gpu = s;
                }
                (p, r)
            }
            CollectorKind::Storage => {
                let (s, r) = self.storage.sample(root, t);
                let p = s.as_ref().map(series::from_storage).unwrap_or_default();
                if s.is_some() {
                    self.latest.storage = s;
                }
                (p, r)
            }
            CollectorKind::Network => {
                let (s, r) = self.network.sample(root, t);
                let p = s.as_ref().map(series::from_network).unwrap_or_default();
                if s.is_some() {
                    self.latest.network = s;
                }
                (p, r)
            }
        };
        let status = self.status.get_mut(&kind).expect("every kind has a status");
        status.health = report.health;
        status.note = report.note;
        status.interval_ms = t.interval_ms;
        status.last_duration_us = Some(started.elapsed().as_micros() as u64);
        if report.health == CollectorHealth::Failing {
            status.consecutive_failures += 1;
        } else {
            status.consecutive_failures = 0;
            status.last_sample_at = Some(t.wall_ms);
        }
        RunResult {
            kind,
            health: report.health,
            points,
        }
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.latest.clone()
    }

    pub fn latest(&self) -> &TelemetrySnapshot {
        &self.latest
    }

    pub fn process_list(&self) -> Option<&ProcessList> {
        self.process_list.as_ref()
    }

    pub fn statuses(&self) -> Vec<CollectorStatus> {
        self.status.values().cloned().collect()
    }

    pub fn set_interval(&mut self, kind: CollectorKind, interval_ms: u64) {
        if let Some(s) = self.status.get_mut(&kind) {
            s.interval_ms = interval_ms;
        }
    }
}

/// Busiest processes by CPU, then by memory, without duplicates.
pub fn top_processes(list: &ProcessList, n: usize) -> Vec<ProcessSample> {
    let mut by_cpu: Vec<&ProcessSample> = list.processes.iter().collect();
    by_cpu.sort_by(|a, b| {
        b.cpu_percent
            .value()
            .unwrap_or(0.0)
            .total_cmp(&a.cpu_percent.value().unwrap_or(0.0))
    });
    let mut by_mem: Vec<&ProcessSample> = list.processes.iter().collect();
    by_mem.sort_by_key(|p| std::cmp::Reverse(p.rss_bytes));
    let mut out: Vec<ProcessSample> = Vec::new();
    for p in by_cpu.into_iter().take(n / 2).chain(by_mem) {
        if out.len() >= n {
            break;
        }
        if !out.iter().any(|o| o.pid == p.pid) {
            out.push(p.clone());
        }
    }
    out
}
