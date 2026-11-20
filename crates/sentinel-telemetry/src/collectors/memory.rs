//! Memory from `/proc/meminfo` (using `MemAvailable`), swap activity from
//! `/proc/vmstat`, pressure from `/proc/pressure/memory`.

use sentinel_hardware::SysRoot;
use sentinel_hardware::parsers::meminfo;

use super::{RunReport, SampleTime};
use crate::counters::{parse_pressure, parse_vmstat, rate};
use crate::model::{MemorySample, Pressure, Reading, ReadingState, round1};

#[derive(Default)]
pub struct MemoryCollector {
    prev_swap: Option<(f64, u64, u64)>,
}

pub fn pressure(root: &SysRoot, rel: &str) -> Pressure {
    match root.read(rel) {
        Ok(text) => {
            let (some, full) = parse_pressure(&text);
            let r = |v: Option<f64>| {
                v.map(Reading::direct)
                    .unwrap_or_else(|| Reading::missing(ReadingState::Unsupported))
            };
            Pressure {
                some_avg10: r(some.map(|p| p.avg10)),
                some_avg60: r(some.map(|p| p.avg60)),
                full_avg10: r(full.map(|p| p.avg10)),
                full_avg60: r(full.map(|p| p.avg60)),
            }
        }
        Err(e) => Pressure::missing(super::state_of(&e)),
    }
}

impl MemoryCollector {
    pub fn sample(&mut self, root: &SysRoot, t: SampleTime) -> (Option<MemorySample>, RunReport) {
        let info = match root.read("proc/meminfo") {
            Ok(text) => meminfo(&text),
            Err(e) => {
                return (
                    None,
                    RunReport::failing(format!("/proc/meminfo unreadable: {e:?}")),
                );
            }
        };
        let get = |k: &str| info.get(k).copied();
        let r = |v: Option<u64>| {
            v.map(Reading::direct)
                .unwrap_or_else(|| Reading::missing(ReadingState::Unsupported))
        };
        let total = get("MemTotal");
        let available = get("MemAvailable");
        // Used = total - available. Without MemAvailable (very old kernels)
        // used is not computed: "total - free" would count cache as used.
        let used = match (total, available) {
            (Some(t), Some(a)) => Some(t.saturating_sub(a)),
            _ => None,
        };
        let used_percent = match (used, total) {
            (Some(u), Some(t)) if t > 0 => Reading::calculated(round1(u as f64 / t as f64 * 100.0)),
            _ => Reading::missing(ReadingState::Unsupported),
        };
        let cached = match (get("Cached"), get("SReclaimable")) {
            (Some(c), Some(s)) => Reading::calculated(c + s),
            (Some(c), None) => Reading::direct(c),
            _ => Reading::missing(ReadingState::Unsupported),
        };
        let swap_used = match (get("SwapTotal"), get("SwapFree")) {
            (Some(t), Some(f)) => Reading::calculated(t.saturating_sub(f)),
            _ => Reading::missing(ReadingState::Unsupported),
        };
        let vm = root
            .read("proc/vmstat")
            .map(|t| parse_vmstat(&t))
            .unwrap_or_default();
        let (swap_in, swap_out) = match (vm.get("pswpin"), vm.get("pswpout")) {
            (Some(i), Some(o)) => {
                let rates = self
                    .prev_swap
                    .map(|(pt, pi, po)| (rate(pi, *i, t.mono_s - pt), rate(po, *o, t.mono_s - pt)));
                self.prev_swap = Some((t.mono_s, *i, *o));
                match rates {
                    Some((Some(a), Some(b))) => (
                        Reading::calculated(round1(a)),
                        Reading::calculated(round1(b)),
                    ),
                    Some(_) => (
                        Reading::missing(ReadingState::Error),
                        Reading::missing(ReadingState::Error),
                    ),
                    None => (Reading::pending(), Reading::pending()),
                }
            }
            _ => (
                Reading::missing(ReadingState::Unsupported),
                Reading::missing(ReadingState::Unsupported),
            ),
        };
        let report = if available.is_none() {
            RunReport::partial(
                "this kernel does not report MemAvailable; used memory is not calculated",
            )
        } else {
            RunReport::ok()
        };
        let sample = MemorySample {
            measured_at: t.wall_ms,
            interval_ms: t.interval_ms,
            total_bytes: r(total),
            available_bytes: r(available),
            used_bytes: used
                .map(Reading::calculated)
                .unwrap_or_else(|| Reading::missing(ReadingState::Unsupported)),
            used_percent,
            free_bytes: r(get("MemFree")),
            buffers_bytes: r(get("Buffers")),
            cached_bytes: cached,
            active_bytes: r(get("Active")),
            inactive_bytes: r(get("Inactive")),
            swap_total_bytes: r(get("SwapTotal")),
            swap_used_bytes: swap_used,
            swap_free_bytes: r(get("SwapFree")),
            swap_in_pages_per_s: swap_in,
            swap_out_pages_per_s: swap_out,
            pressure: pressure(root, "proc/pressure/memory"),
        };
        (Some(sample), report)
    }
}
