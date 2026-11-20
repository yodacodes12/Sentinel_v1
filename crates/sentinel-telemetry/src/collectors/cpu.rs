//! CPU utilisation from `/proc/stat` counter deltas; per-core frequency
//! from `cpufreq/scaling_cur_freq`.

use std::collections::BTreeMap;

use sentinel_hardware::SysRoot;

use super::{RunReport, SampleTime};
use crate::counters::{CpuCounters, cpu_shares, parse_proc_stat};
use crate::model::{CoreSample, CpuSample, CpuTimes, Reading, ReadingState, round1};

#[derive(Default)]
pub struct CpuCollector {
    prev_total: Option<CpuCounters>,
    prev_cores: BTreeMap<u32, CpuCounters>,
}

impl CpuCollector {
    pub fn sample(&mut self, root: &SysRoot, t: SampleTime) -> (Option<CpuSample>, RunReport) {
        let stat = match root.read("proc/stat") {
            Ok(text) => parse_proc_stat(&text),
            Err(e) => {
                return (
                    None,
                    RunReport::failing(format!("/proc/stat unreadable: {e:?}")),
                );
            }
        };
        let Some(total) = stat.total else {
            return (None, RunReport::failing("/proc/stat has no cpu line"));
        };
        let shares = self.prev_total.and_then(|p| cpu_shares(&p, &total));
        let pct = |f: fn(&crate::counters::CpuShares) -> f64| match &shares {
            Some(s) => Reading::calculated(round1(f(s))),
            None => Reading::pending(),
        };
        let times = CpuTimes {
            user_percent: pct(|s| s.user),
            nice_percent: pct(|s| s.nice),
            system_percent: pct(|s| s.system),
            idle_percent: pct(|s| s.idle),
            iowait_percent: pct(|s| s.iowait),
            irq_percent: pct(|s| s.irq),
            softirq_percent: pct(|s| s.softirq),
            steal_percent: pct(|s| s.steal),
        };
        let mut cores = Vec::new();
        let mut freqs = Vec::new();
        for (cpu, counters) in &stat.per_cpu {
            // A CPU that just came online has no previous reading: pending.
            let util = match self
                .prev_cores
                .get(cpu)
                .and_then(|p| cpu_shares(p, counters))
            {
                Some(s) => Reading::calculated(round1(s.busy)),
                None => Reading::pending(),
            };
            let freq = match root.read_trimmed(&format!(
                "sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq"
            )) {
                Ok(v) => match v.parse::<u64>() {
                    Ok(khz) => {
                        freqs.push(khz);
                        Reading::direct(khz)
                    }
                    Err(_) => Reading::missing(ReadingState::Error),
                },
                Err(e) => Reading::missing(super::state_of(&e)),
            };
            cores.push(CoreSample {
                cpu: *cpu,
                utilisation_percent: util,
                frequency_khz: freq,
            });
        }
        let average_frequency_khz = if freqs.is_empty() {
            Reading::missing(ReadingState::Unsupported)
        } else {
            Reading::calculated(freqs.iter().sum::<u64>() / freqs.len() as u64)
        };
        let report = if self.prev_total.is_some() && shares.is_none() {
            RunReport::partial(
                "no CPU time elapsed or the counters went backwards; this interval was skipped",
            )
        } else {
            RunReport::ok()
        };
        self.prev_total = Some(total);
        self.prev_cores = stat.per_cpu.clone();
        let sample = CpuSample {
            measured_at: t.wall_ms,
            interval_ms: t.interval_ms,
            logical_cpus: stat.per_cpu.len() as u32,
            total_percent: shares
                .map(|s| Reading::calculated(round1(s.busy)))
                .unwrap_or_else(Reading::pending),
            times,
            cores,
            average_frequency_khz,
        };
        (Some(sample), report)
    }
}
