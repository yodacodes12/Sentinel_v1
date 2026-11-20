//! Turning samples into time series: bounded in-memory buffers for live
//! charts, and 30-second buckets for the history database.
//!
//! A reading without a value becomes a gap (`None`), never zero. Process
//! series (`proc.*`) live in memory only and are never written to history.

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::model::{
    BatteryReport, CpuSample, FanReport, GpuReport, MemorySample, NetworkReport, ProcessList,
    Quality, Reading, SeriesKey, SeriesPoint, StorageReport, SystemLoad, ThermalReport,
};

/// What a series is: its unit, where its values come from and how they were
/// obtained. Stored with the history so a value is never separated from its
/// provenance. The device or process is part of the series key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesMeta {
    pub unit: &'static str,
    pub source: &'static str,
    pub quality: Quality,
}

/// One extracted value: series, what it is, value (`None` = gap).
pub type Point = (SeriesKey, SeriesMeta, Option<f64>);

fn key(text: String) -> Option<SeriesKey> {
    SeriesKey::new(text).ok()
}

fn meta(unit: &'static str, source: &'static str, quality: Quality) -> SeriesMeta {
    SeriesMeta {
        unit,
        source,
        quality,
    }
}

fn push<T: Copy + Into<f64>>(
    out: &mut Vec<Point>,
    k: String,
    (unit, source): (&'static str, &'static str),
    r: &Reading<T>,
) {
    if let Some(k) = key(k) {
        // Pending readings (first sample of a rate) are not gaps; skip them.
        if r.state != crate::model::ReadingState::Pending {
            out.push((k, meta(unit, source, r.quality), r.value().map(Into::into)));
        }
    }
}

fn push_u64(
    out: &mut Vec<Point>,
    k: String,
    (unit, source): (&'static str, &'static str),
    r: &Reading<u64>,
) {
    if let Some(k) = key(k)
        && r.state != crate::model::ReadingState::Pending
    {
        out.push((
            k,
            meta(unit, source, r.quality),
            r.value().map(|v| v as f64),
        ));
    }
}

pub fn from_cpu(s: &CpuSample) -> Vec<Point> {
    let mut out = Vec::new();
    push(
        &mut out,
        "cpu.total".into(),
        ("%", "/proc/stat"),
        &s.total_percent,
    );
    push(
        &mut out,
        "cpu.iowait".into(),
        ("%", "/proc/stat"),
        &s.times.iowait_percent,
    );
    push_u64(
        &mut out,
        "cpu.freq".into(),
        ("kHz", "/sys/devices/system/cpu/*/cpufreq/scaling_cur_freq"),
        &s.average_frequency_khz,
    );
    for c in &s.cores {
        push(
            &mut out,
            format!("cpu.core.{}", c.cpu),
            ("%", "/proc/stat"),
            &c.utilisation_percent,
        );
        push_u64(
            &mut out,
            format!("cpu.core.{}.freq", c.cpu),
            ("kHz", "/sys/devices/system/cpu/*/cpufreq/scaling_cur_freq"),
            &c.frequency_khz,
        );
    }
    out
}

pub fn from_memory(s: &MemorySample) -> Vec<Point> {
    let mut out = Vec::new();
    push(
        &mut out,
        "mem.used_percent".into(),
        ("%", "/proc/meminfo"),
        &s.used_percent,
    );
    push_u64(
        &mut out,
        "mem.used".into(),
        ("B", "/proc/meminfo"),
        &s.used_bytes,
    );
    push_u64(
        &mut out,
        "mem.available".into(),
        ("B", "/proc/meminfo"),
        &s.available_bytes,
    );
    push_u64(
        &mut out,
        "mem.cached".into(),
        ("B", "/proc/meminfo"),
        &s.cached_bytes,
    );
    push_u64(
        &mut out,
        "swap.used".into(),
        ("B", "/proc/meminfo"),
        &s.swap_used_bytes,
    );
    push(
        &mut out,
        "swap.in".into(),
        ("pages/s", "/proc/vmstat"),
        &s.swap_in_pages_per_s,
    );
    push(
        &mut out,
        "swap.out".into(),
        ("pages/s", "/proc/vmstat"),
        &s.swap_out_pages_per_s,
    );
    push(
        &mut out,
        "mem.pressure".into(),
        ("%", "/proc/pressure/memory"),
        &s.pressure.some_avg10,
    );
    out
}

pub fn from_load(s: &SystemLoad) -> Vec<Point> {
    let mut out = Vec::new();
    push(&mut out, "load.1".into(), ("", "/proc/loadavg"), &s.load1);
    push(
        &mut out,
        "cpu.pressure".into(),
        ("%", "/proc/pressure/cpu"),
        &s.cpu_pressure.some_avg10,
    );
    push(
        &mut out,
        "io.pressure".into(),
        ("%", "/proc/pressure/io"),
        &s.io_pressure.some_avg10,
    );
    out
}

pub fn from_battery(s: &BatteryReport) -> Vec<Point> {
    let mut out = Vec::new();
    for b in &s.batteries {
        push(
            &mut out,
            format!("battery.{}.percent", b.name),
            ("%", "/sys/class/power_supply"),
            &b.percent,
        );
        push(
            &mut out,
            format!("battery.{}.power", b.name),
            ("W", "/sys/class/power_supply"),
            &b.power_w,
        );
        push(
            &mut out,
            format!("battery.{}.energy", b.name),
            ("Wh", "/sys/class/power_supply"),
            &b.energy_now_wh,
        );
        if b.temperature_c.is_ok() {
            push(
                &mut out,
                format!("battery.{}.temp", b.name),
                ("°C", "/sys/class/power_supply"),
                &b.temperature_c,
            );
        }
    }
    out
}

pub fn from_thermal(s: &ThermalReport) -> Vec<Point> {
    let mut out = Vec::new();
    for t in &s.temperatures {
        push(
            &mut out,
            format!("temp.{}", t.id),
            ("°C", "/sys/class/hwmon, /sys/class/thermal"),
            &t.value_c,
        );
    }
    out
}

pub fn from_fans(s: &FanReport) -> Vec<Point> {
    let mut out = Vec::new();
    for f in &s.fans {
        push(
            &mut out,
            format!("fan.{}", f.id),
            ("RPM", "/sys/class/hwmon"),
            &f.rpm.map(|v| v as f64),
        );
    }
    out
}

pub fn from_gpu(s: &GpuReport) -> Vec<Point> {
    let mut out = Vec::new();
    for g in &s.gpus {
        let a = &g.pci_address;
        if g.busy_percent.state != crate::model::ReadingState::Unsupported {
            push(
                &mut out,
                format!("gpu.{a}.busy"),
                ("%", "/sys/bus/pci/devices (GPU driver sysfs and hwmon)"),
                &g.busy_percent,
            );
        }
        if g.temperature_c.state != crate::model::ReadingState::Unsupported {
            push(
                &mut out,
                format!("gpu.{a}.temp"),
                ("°C", "/sys/bus/pci/devices (GPU driver sysfs and hwmon)"),
                &g.temperature_c,
            );
        }
        if g.power_w.state != crate::model::ReadingState::Unsupported {
            push(
                &mut out,
                format!("gpu.{a}.power"),
                ("W", "/sys/bus/pci/devices (GPU driver sysfs and hwmon)"),
                &g.power_w,
            );
        }
        if g.vram_used_bytes.state != crate::model::ReadingState::Unsupported {
            push_u64(
                &mut out,
                format!("gpu.{a}.vram"),
                ("B", "/sys/bus/pci/devices (GPU driver sysfs and hwmon)"),
                &g.vram_used_bytes,
            );
        }
    }
    out
}

pub fn from_storage(s: &StorageReport) -> Vec<Point> {
    let mut out = Vec::new();
    for d in &s.devices {
        push(
            &mut out,
            format!("disk.{}.read", d.name),
            ("B/s", "/proc/diskstats"),
            &d.read_bps,
        );
        push(
            &mut out,
            format!("disk.{}.write", d.name),
            ("B/s", "/proc/diskstats"),
            &d.write_bps,
        );
        push(
            &mut out,
            format!("disk.{}.busy", d.name),
            ("%", "/proc/diskstats"),
            &d.busy_percent,
        );
    }
    for f in &s.filesystems {
        let mount = if f.mount_point == "/" {
            "root".to_string()
        } else {
            f.mount_point
                .trim_start_matches('/')
                .replace(['/', ' '], "_")
        };
        push(
            &mut out,
            format!("fs.{mount}.used_percent"),
            ("%", "statvfs"),
            &f.used_percent,
        );
    }
    out
}

pub fn from_network(s: &NetworkReport) -> Vec<Point> {
    let mut out = Vec::new();
    for i in s.interfaces.iter().filter(|i| !i.loopback) {
        push(
            &mut out,
            format!("net.{}.rx", i.name),
            ("B/s", "/sys/class/net/*/statistics"),
            &i.rx_bps,
        );
        push(
            &mut out,
            format!("net.{}.tx", i.name),
            ("B/s", "/sys/class/net/*/statistics"),
            &i.tx_bps,
        );
    }
    out
}

/// Memory-only series for the busiest processes.
pub fn from_processes(s: &ProcessList, top: usize) -> Vec<Point> {
    let mut by_cpu: Vec<_> = s.processes.iter().collect();
    by_cpu.sort_by(|a, b| {
        b.cpu_percent
            .value()
            .unwrap_or(0.0)
            .total_cmp(&a.cpu_percent.value().unwrap_or(0.0))
    });
    let mut by_mem: Vec<_> = s.processes.iter().collect();
    by_mem.sort_by_key(|p| std::cmp::Reverse(p.rss_bytes));
    let mut chosen: Vec<u32> = by_cpu
        .iter()
        .take(top)
        .chain(by_mem.iter().take(top))
        .map(|p| p.pid)
        .collect();
    chosen.sort_unstable();
    chosen.dedup();
    let mut out = Vec::new();
    for p in s.processes.iter().filter(|p| chosen.contains(&p.pid)) {
        push(
            &mut out,
            format!("proc.{}.cpu", p.pid),
            ("%", "/proc/PID/stat"),
            &p.cpu_percent,
        );
        if let Some(k) = key(format!("proc.{}.rss", p.pid)) {
            out.push((
                k,
                meta("B", "/proc/PID/stat", Quality::Calculated),
                Some(p.rss_bytes as f64),
            ));
        }
    }
    out
}

/// Whether a series is written to the history database.
pub fn persisted(key: &SeriesKey) -> bool {
    !key.as_str().starts_with("proc.") && !key.as_str().contains(".freq")
}

// --------------------------------------------------------- live buffers

/// One live series: what it is and `(unix ms, value)` points, oldest
/// first. `None` values are gaps.
type LiveSeries = (SeriesMeta, VecDeque<(u64, Option<f64>)>);

/// Bounded per-series ring buffers. Memory is capped by both the points per
/// series and the number of series.
#[derive(Debug)]
pub struct LiveBuffers {
    per_series: usize,
    max_series: usize,
    series: HashMap<SeriesKey, LiveSeries>,
}

impl LiveBuffers {
    pub fn new(per_series: usize, max_series: usize) -> Self {
        Self {
            per_series,
            max_series,
            series: HashMap::new(),
        }
    }

    pub fn push(&mut self, t: u64, (key, meta, v): Point) {
        if !self.series.contains_key(&key) && self.series.len() >= self.max_series {
            return;
        }
        let entry = self
            .series
            .entry(key)
            .or_insert_with(|| (meta, VecDeque::new()));
        if v.is_some() {
            entry.0 = meta;
        }
        if entry.1.len() >= self.per_series {
            entry.1.pop_front();
        }
        entry.1.push_back((t, v));
    }

    /// Record a gap in every known series (monitoring paused or stopped),
    /// so charts do not draw a line across the time nothing was measured.
    pub fn mark_gap(&mut self, t: u64) {
        for (_, points) in self.series.values_mut() {
            if points.back().is_some_and(|(_, v)| v.is_some()) {
                if points.len() >= self.per_series {
                    points.pop_front();
                }
                points.push_back((t, None));
            }
        }
    }

    pub fn query(&self, key: &SeriesKey, since_ms: u64) -> Option<(SeriesMeta, Vec<SeriesPoint>)> {
        let (meta, points) = self.series.get(key)?;
        Some((
            *meta,
            points
                .iter()
                .filter(|(t, _)| *t >= since_ms)
                .map(|(t, v)| SeriesPoint {
                    t: *t,
                    v: *v,
                    min: None,
                    max: None,
                })
                .collect(),
        ))
    }

    /// Forget process series of processes no longer tracked.
    pub fn retain_processes(&mut self, keep: &[u32]) {
        self.series.retain(|k, _| {
            let s = k.as_str();
            match s
                .strip_prefix("proc.")
                .and_then(|r| r.split('.').next())
                .and_then(|p| p.parse::<u32>().ok())
            {
                Some(pid) => keep.contains(&pid),
                None => true,
            }
        });
    }

    pub fn len(&self) -> usize {
        self.series.len()
    }

    pub fn is_empty(&self) -> bool {
        self.series.is_empty()
    }

    pub fn point_count(&self) -> usize {
        self.series.values().map(|(_, p)| p.len()).sum()
    }
}

// ------------------------------------------------------ history buckets

/// One aggregated history row.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub key: SeriesKey,
    pub unit: &'static str,
    pub source: &'static str,
    /// Quality of the bucket's values (of the last one, if it changed).
    pub quality: Quality,
    /// Start of the bucket, unix ms.
    pub t: u64,
    /// `None` when every sample in the bucket was missing (a gap).
    pub avg: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Samples with a value in this bucket (0 for a gap).
    pub samples: u32,
}

#[derive(Debug, Clone)]
struct Open {
    meta: SeriesMeta,
    start: u64,
    sum: f64,
    count: u32,
    missing: u32,
    min: f64,
    max: f64,
}

/// Folds samples into fixed-width buckets; a bucket is emitted when a
/// sample for a later bucket arrives or on `flush`.
#[derive(Debug)]
pub struct Bucketer {
    width_ms: u64,
    open: BTreeMap<SeriesKey, Open>,
}

impl Bucketer {
    pub fn new(width_ms: u64) -> Self {
        Self {
            width_ms: width_ms.max(1),
            open: BTreeMap::new(),
        }
    }

    fn close(key: SeriesKey, o: Open) -> HistoryRow {
        let has = o.count > 0;
        HistoryRow {
            key,
            unit: o.meta.unit,
            source: o.meta.source,
            quality: o.meta.quality,
            t: o.start,
            avg: has.then(|| o.sum / o.count as f64),
            min: has.then_some(o.min),
            max: has.then_some(o.max),
            samples: o.count,
        }
    }

    pub fn push(&mut self, t: u64, (key, meta, v): Point) -> Option<HistoryRow> {
        if !persisted(&key) {
            return None;
        }
        let start = t - t % self.width_ms;
        let mut closed = None;
        if let Some(o) = self.open.get(&key)
            && o.start != start
            && let Some(o) = self.open.remove(&key)
        {
            closed = Some(Self::close(key.clone(), o));
        }
        let o = self.open.entry(key).or_insert(Open {
            meta,
            start,
            sum: 0.0,
            count: 0,
            missing: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        });
        match v {
            Some(v) if v.is_finite() => {
                o.meta = meta;
                o.sum += v;
                o.count += 1;
                o.min = o.min.min(v);
                o.max = o.max.max(v);
            }
            _ => o.missing += 1,
        }
        closed
    }

    /// Close every open bucket (monitoring stops, daemon exits).
    pub fn flush(&mut self) -> Vec<HistoryRow> {
        std::mem::take(&mut self.open)
            .into_iter()
            .map(|(k, o)| Self::close(k, o))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(unit: &'static str) -> SeriesMeta {
        meta(unit, "test", Quality::Calculated)
    }

    fn k(s: &str) -> SeriesKey {
        SeriesKey::new(s).unwrap()
    }

    #[test]
    fn buffers_are_bounded() {
        let mut b = LiveBuffers::new(3, 2);
        for i in 0..10 {
            b.push(i, (k("cpu.total"), m("%"), Some(i as f64)));
        }
        b.push(0, (k("mem.used"), m("B"), Some(1.0)));
        b.push(0, (k("third"), m("B"), Some(1.0)));
        assert_eq!(b.len(), 2, "series cap");
        let (_, pts) = b.query(&k("cpu.total"), 0).unwrap();
        assert_eq!(pts.iter().map(|p| p.t).collect::<Vec<_>>(), [7, 8, 9]);
        assert_eq!(b.query(&k("cpu.total"), 9).unwrap().1.len(), 1);
    }

    #[test]
    fn gaps_are_recorded_not_bridged() {
        let mut b = LiveBuffers::new(10, 10);
        b.push(1, (k("cpu.total"), m("%"), Some(5.0)));
        b.mark_gap(2);
        b.mark_gap(3);
        b.push(4, (k("cpu.total"), m("%"), None));
        let (_, pts) = b.query(&k("cpu.total"), 0).unwrap();
        assert_eq!(
            pts.iter().map(|p| p.v).collect::<Vec<_>>(),
            [Some(5.0), None, None]
        );
    }

    #[test]
    fn buckets_average_and_keep_gaps() {
        let mut b = Bucketer::new(30_000);
        assert!(
            b.push(1_000, (k("cpu.total"), m("%"), Some(10.0)))
                .is_none()
        );
        assert!(
            b.push(11_000, (k("cpu.total"), m("%"), Some(30.0)))
                .is_none()
        );
        assert!(b.push(21_000, (k("cpu.total"), m("%"), None)).is_none());
        let row = b
            .push(31_000, (k("cpu.total"), m("%"), Some(50.0)))
            .unwrap();
        assert_eq!(
            (row.t, row.avg, row.min, row.max, row.samples),
            (0, Some(20.0), Some(10.0), Some(30.0), 2)
        );
        let row = b.push(61_000, (k("cpu.total"), m("%"), None)).unwrap();
        assert_eq!((row.t, row.avg), (30_000, Some(50.0)));
        // A bucket with only missing samples is a gap, not zero.
        let rows = b.flush();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].t, rows[0].avg, rows[0].samples), (60_000, None, 0));
    }

    #[test]
    fn process_series_are_never_persisted() {
        let mut b = Bucketer::new(1_000);
        assert!(b.push(0, (k("proc.42.cpu"), m("%"), Some(1.0))).is_none());
        assert!(b.flush().is_empty());
        assert!(!persisted(&k("proc.42.rss")));
        assert!(persisted(&k("cpu.total")));
    }

    #[test]
    fn process_buffers_are_pruned() {
        let mut b = LiveBuffers::new(10, 10);
        b.push(0, (k("proc.1.cpu"), m("%"), Some(1.0)));
        b.push(0, (k("proc.2.cpu"), m("%"), Some(1.0)));
        b.push(0, (k("cpu.total"), m("%"), Some(1.0)));
        b.retain_processes(&[2]);
        assert!(b.query(&k("proc.1.cpu"), 0).is_none());
        assert!(b.query(&k("proc.2.cpu"), 0).is_some());
        assert!(b.query(&k("cpu.total"), 0).is_some());
    }
}
