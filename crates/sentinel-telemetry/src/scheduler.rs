//! Central sampling schedule.
//!
//! One clock drives every collector. Each collector has an interval from
//! the sampling profile; its next due time advances on a fixed grid
//! (`previous due + interval`), so collectors with related intervals keep
//! waking together and the monitor thread wakes as rarely as possible.
//!
//! Adaptive rules:
//! - a collector whose source does not exist is re-checked every 5 minutes;
//! - a failing collector backs off exponentially, up to 5 minutes;
//! - the process collector, the most expensive one, runs at its profile
//!   interval only while someone looks at process data (the desktop page or
//!   the CLI asked in the last minute) and otherwise once a minute.

use std::collections::BTreeMap;

use crate::model::{CollectorHealth, CollectorKind, SamplingProfile};

pub const UNSUPPORTED_RECHECK_S: f64 = 300.0;
pub const MAX_BACKOFF_S: f64 = 300.0;
pub const PROCESS_IDLE_INTERVAL_S: f64 = 60.0;
pub const PROCESS_INTEREST_S: f64 = 60.0;
/// Collectors due within this slack run together.
pub const SLACK_S: f64 = 0.25;

/// Interval in milliseconds for `kind` under `profile` (the plan's table).
pub fn profile_interval_ms(profile: SamplingProfile, kind: CollectorKind) -> u64 {
    use CollectorKind::*;
    use SamplingProfile::*;
    let s: f64 = match (profile, kind) {
        (Eco, Cpu | Memory | Storage | Network | SystemLoad) => 5.0,
        (Eco, Processes | Thermal | Gpu | Fans) => 10.0,
        (Eco, Battery) => 30.0,
        (Balanced, Cpu | Memory | Storage | Network | SystemLoad) => 2.0,
        (Balanced, Processes | Thermal | Gpu | Fans) => 5.0,
        (Balanced, Battery) => 15.0,
        (Performance, Cpu | Memory | Storage | Network | SystemLoad) => 1.0,
        (Performance, Processes | Thermal | Gpu | Fans) => 2.0,
        (Performance, Battery) => 10.0,
    };
    (s * 1000.0) as u64
}

#[derive(Debug, Clone)]
pub struct Scheduler {
    profile: SamplingProfile,
    next_due: BTreeMap<CollectorKind, f64>,
    failures: BTreeMap<CollectorKind, u32>,
    unsupported: BTreeMap<CollectorKind, bool>,
    process_interest_until: f64,
}

impl Scheduler {
    /// Everything is due immediately at `now`.
    pub fn new(profile: SamplingProfile, now: f64) -> Self {
        Self {
            profile,
            next_due: CollectorKind::ALL.iter().map(|k| (*k, now)).collect(),
            failures: BTreeMap::new(),
            unsupported: BTreeMap::new(),
            process_interest_until: f64::NEG_INFINITY,
        }
    }

    pub fn profile(&self) -> SamplingProfile {
        self.profile
    }

    /// Change profile; collectors due later than the new interval allows
    /// are pulled in, none is pushed out.
    pub fn set_profile(&mut self, profile: SamplingProfile, now: f64) {
        self.profile = profile;
        for kind in CollectorKind::ALL {
            let interval = self.interval_s(kind, now);
            if let Some(due) = self.next_due.get_mut(&kind) {
                *due = due.min(now + interval);
            }
        }
    }

    /// Someone looked at process data: sample processes at full rate.
    pub fn note_process_interest(&mut self, now: f64) {
        let was_idle = now >= self.process_interest_until;
        self.process_interest_until = now + PROCESS_INTEREST_S;
        if was_idle {
            self.next_due.insert(CollectorKind::Processes, now);
        }
    }

    pub fn process_interest(&self, now: f64) -> bool {
        now < self.process_interest_until
    }

    /// Effective interval in seconds, after adaptation and backoff.
    pub fn interval_s(&self, kind: CollectorKind, now: f64) -> f64 {
        if self.unsupported.get(&kind).copied().unwrap_or(false) {
            return UNSUPPORTED_RECHECK_S;
        }
        let mut base = profile_interval_ms(self.profile, kind) as f64 / 1000.0;
        if kind == CollectorKind::Processes && !self.process_interest(now) {
            base = base.max(PROCESS_IDLE_INTERVAL_S);
        }
        let failures = self.failures.get(&kind).copied().unwrap_or(0);
        if failures > 0 {
            base = (base * 2f64.powi(failures.min(10) as i32)).min(MAX_BACKOFF_S);
        }
        base
    }

    pub fn interval_ms(&self, kind: CollectorKind, now: f64) -> u64 {
        (self.interval_s(kind, now) * 1000.0).round() as u64
    }

    /// Collectors due now (within the slack), in a fixed order.
    pub fn due(&self, now: f64) -> Vec<CollectorKind> {
        CollectorKind::ALL
            .into_iter()
            .filter(|k| self.next_due.get(k).is_some_and(|d| *d <= now + SLACK_S))
            .collect()
    }

    /// Record a run and schedule the next one on the collector's grid.
    pub fn record(&mut self, kind: CollectorKind, health: CollectorHealth, now: f64) {
        match health {
            CollectorHealth::Failing => *self.failures.entry(kind).or_insert(0) += 1,
            _ => {
                self.failures.remove(&kind);
            }
        }
        self.unsupported
            .insert(kind, health == CollectorHealth::Unsupported);
        let interval = self.interval_s(kind, now);
        let prev = self.next_due.get(&kind).copied().unwrap_or(now);
        let mut next = prev + interval;
        // Fell behind (suspend, slow run): restart the grid from now.
        if next <= now {
            next = now + interval;
        }
        self.next_due.insert(kind, next);
    }

    pub fn consecutive_failures(&self, kind: CollectorKind) -> u32 {
        self.failures.get(&kind).copied().unwrap_or(0)
    }

    /// When the monitor thread must next wake.
    pub fn next_wake(&self) -> f64 {
        self.next_due
            .values()
            .copied()
            .fold(f64::INFINITY, f64::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CollectorKind::*;

    #[test]
    fn profile_tables_match_the_plan() {
        assert_eq!(profile_interval_ms(SamplingProfile::Eco, Cpu), 5_000);
        assert_eq!(profile_interval_ms(SamplingProfile::Eco, Battery), 30_000);
        assert_eq!(profile_interval_ms(SamplingProfile::Balanced, Cpu), 2_000);
        assert_eq!(
            profile_interval_ms(SamplingProfile::Balanced, Processes),
            5_000
        );
        assert_eq!(
            profile_interval_ms(SamplingProfile::Balanced, Battery),
            15_000
        );
        assert_eq!(
            profile_interval_ms(SamplingProfile::Performance, Cpu),
            1_000
        );
        assert_eq!(
            profile_interval_ms(SamplingProfile::Performance, Thermal),
            2_000
        );
        assert_eq!(
            profile_interval_ms(SamplingProfile::Performance, Battery),
            10_000
        );
    }

    #[test]
    fn everything_runs_first_then_on_a_grid() {
        let mut s = Scheduler::new(SamplingProfile::Balanced, 100.0);
        assert_eq!(s.due(100.0).len(), CollectorKind::ALL.len());
        for k in CollectorKind::ALL {
            s.record(k, CollectorHealth::Ok, 100.0);
        }
        assert_eq!(s.next_wake(), 102.0);
        assert_eq!(s.due(102.0), [Cpu, Memory, Storage, Network, SystemLoad]);
        // A run that finished late keeps the grid (102 -> 104, not 102.3 + 2).
        for k in s.due(102.0) {
            s.record(k, CollectorHealth::Ok, 102.3);
        }
        assert_eq!(s.next_wake(), 104.0);
        // At t=110 temperature, GPU, fans (5 s) and CPU-class (2 s) coincide.
        let mut s2 = Scheduler::new(SamplingProfile::Balanced, 0.0);
        for t in [0.0, 2.0, 4.0, 5.0, 6.0, 8.0] {
            for k in s2.due(t) {
                s2.record(k, CollectorHealth::Ok, t);
            }
        }
        let at_ten = s2.due(10.0);
        assert!(at_ten.contains(&Cpu) && at_ten.contains(&Thermal) && at_ten.contains(&Gpu));
    }

    #[test]
    fn processes_run_slowly_without_interest() {
        let mut s = Scheduler::new(SamplingProfile::Performance, 0.0);
        assert_eq!(s.interval_s(Processes, 0.0), PROCESS_IDLE_INTERVAL_S);
        s.note_process_interest(10.0);
        assert_eq!(s.interval_s(Processes, 10.0), 2.0);
        assert_eq!(s.due(10.0).iter().filter(|k| **k == Processes).count(), 1);
        assert_eq!(
            s.interval_s(Processes, 80.0),
            PROCESS_IDLE_INTERVAL_S,
            "interest expires"
        );
    }

    #[test]
    fn failures_back_off_and_unsupported_sources_are_rechecked_rarely() {
        let mut s = Scheduler::new(SamplingProfile::Balanced, 0.0);
        s.record(Gpu, CollectorHealth::Failing, 0.0);
        assert_eq!(s.interval_s(Gpu, 0.0), 10.0);
        s.record(Gpu, CollectorHealth::Failing, 10.0);
        assert_eq!(s.interval_s(Gpu, 10.0), 20.0);
        for i in 0..20 {
            s.record(Gpu, CollectorHealth::Failing, 20.0 + i as f64);
        }
        assert_eq!(s.interval_s(Gpu, 100.0), MAX_BACKOFF_S);
        s.record(Gpu, CollectorHealth::Ok, 500.0);
        assert_eq!(s.interval_s(Gpu, 500.0), 5.0, "recovers immediately");
        s.record(Fans, CollectorHealth::Unsupported, 0.0);
        assert_eq!(s.interval_s(Fans, 0.0), UNSUPPORTED_RECHECK_S);
    }

    #[test]
    fn profile_change_pulls_collectors_in() {
        let mut s = Scheduler::new(SamplingProfile::Eco, 0.0);
        for k in CollectorKind::ALL {
            s.record(k, CollectorHealth::Ok, 0.0);
        }
        assert_eq!(s.next_wake(), 5.0);
        s.set_profile(SamplingProfile::Performance, 1.0);
        assert_eq!(s.next_wake(), 2.0);
    }
}
