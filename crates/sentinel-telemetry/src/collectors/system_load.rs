//! Load averages, task counts, uptime and CPU/I/O pressure.

use sentinel_hardware::SysRoot;

use super::{RunReport, SampleTime};
use crate::collectors::memory::pressure;
use crate::counters::parse_loadavg;
use crate::model::{Reading, ReadingState, SystemLoad};

pub fn sample(root: &SysRoot, t: SampleTime) -> (Option<SystemLoad>, RunReport) {
    let raw = root.read("proc/loadavg");
    let load = raw.as_ref().ok().and_then(|s| parse_loadavg(s));
    // Unreadable keeps the reader's reason; readable but malformed is an error.
    let load_state = match &raw {
        Ok(_) => ReadingState::Error,
        Err(e) => super::state_of(e),
    };
    let uptime = root
        .read("proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok());
    let missing = || Reading::missing(load_state);
    let report = match (&raw, load) {
        (_, Some(_)) => RunReport::ok(),
        (Ok(_), None) => RunReport::failing("/proc/loadavg is malformed"),
        (Err(_), None) => RunReport::failing("/proc/loadavg unreadable"),
    };
    let sample = SystemLoad {
        measured_at: t.wall_ms,
        interval_ms: t.interval_ms,
        load1: load.map(|l| Reading::direct(l.0)).unwrap_or_else(missing),
        load5: load.map(|l| Reading::direct(l.1)).unwrap_or_else(missing),
        load15: load.map(|l| Reading::direct(l.2)).unwrap_or_else(missing),
        runnable_tasks: load
            .map(|l| Reading::direct(l.3))
            .unwrap_or_else(|| Reading::missing(load_state)),
        total_tasks: load
            .map(|l| Reading::direct(l.4))
            .unwrap_or_else(|| Reading::missing(load_state)),
        uptime_s: uptime
            .map(|u| Reading::direct(u as u64))
            .unwrap_or_else(|| Reading::missing(ReadingState::Unsupported)),
        cpu_pressure: pressure(root, "proc/pressure/cpu"),
        io_pressure: pressure(root, "proc/pressure/io"),
    };
    (Some(sample), report)
}
