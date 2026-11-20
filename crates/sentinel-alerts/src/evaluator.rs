//! The alert state machine.
//!
//! For each `(rule, subject)` pair:
//!
//! ```text
//! Normal --triggered--> Pending --held for `duration`--> Active
//! Pending --not triggered--> Normal            (a spike is not an alert)
//! Active --clear for `clear_after`--> Resolved (recovery event)
//! Active --in the hysteresis band--> stays Active
//! Active --higher severity--> escalated (notified again)
//! ```
//!
//! One alert exists per condition at a time (duplicate suppression). A
//! condition that resolved less than `cooldown` ago and comes back is
//! recorded but not notified again.

use std::collections::{BTreeMap, HashMap, VecDeque};

use sentinel_telemetry::model::{
    Alert, AlertCategory, AlertSeverity, AlertStatus, ThresholdSource,
};

/// What one rule says about one subject in one evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub rule: &'static str,
    pub subject: Option<String>,
    pub category: AlertCategory,
    pub metric: String,
    pub level: Level,
    /// How long the condition must hold before an alert (ms).
    pub duration_ms: u64,
    /// How long it must be clear before the alert resolves (ms).
    pub clear_after_ms: u64,
    pub title: String,
    pub explanation: String,
    pub recommendation: Option<String>,
    pub threshold: String,
    pub threshold_source: ThresholdSource,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Over the threshold, at this severity.
    Triggered(AlertSeverity),
    /// Under the threshold but inside the hysteresis band: no change.
    Band,
    /// Clearly back to normal.
    Clear,
    /// No data this time: no change in either direction.
    Unknown,
}

#[derive(Debug, Clone)]
enum State {
    Pending { since: u64, severity: AlertSeverity },
    Active { id: u64, clear_since: Option<u64> },
}

/// What happened in an evaluation, for notifications and history.
#[derive(Debug, Clone, PartialEq)]
pub enum AlertEvent {
    Raised { alert: Alert, notify: bool },
    Escalated { alert: Alert, notify: bool },
    Resolved { alert: Alert },
}

impl AlertEvent {
    pub fn alert(&self) -> &Alert {
        match self {
            AlertEvent::Raised { alert, .. }
            | AlertEvent::Escalated { alert, .. }
            | AlertEvent::Resolved { alert } => alert,
        }
    }
}

pub struct Evaluator {
    states: HashMap<(String, Option<String>), State>,
    active: BTreeMap<u64, Alert>,
    resolved: VecDeque<Alert>,
    last_resolved_at: HashMap<(String, Option<String>), u64>,
    next_id: u64,
    pub cooldown_ms: u64,
}

const RESOLVED_KEPT: usize = 200;

impl Evaluator {
    pub fn new(first_id: u64, cooldown_ms: u64) -> Self {
        Self {
            states: HashMap::new(),
            active: BTreeMap::new(),
            resolved: VecDeque::new(),
            last_resolved_at: HashMap::new(),
            next_id: first_id.max(1),
            cooldown_ms,
        }
    }

    pub fn active(&self) -> Vec<Alert> {
        self.active.values().cloned().collect()
    }

    pub fn recently_resolved(&self) -> Vec<Alert> {
        self.resolved.iter().cloned().collect()
    }

    /// Remember that a condition resolved at `at` (an alert from an earlier
    /// session), so that its return within the cooldown is recorded but
    /// not notified again: restarting the daemon or monitoring must not
    /// repeat a notification the user has just seen.
    pub fn remember_resolved(&mut self, rule: &str, subject: Option<String>, at: u64) {
        let last = self
            .last_resolved_at
            .entry((rule.to_string(), subject))
            .or_insert(at);
        *last = (*last).max(at);
    }

    /// Mark an alert as seen. Changes nothing else: the condition is still
    /// evaluated and other alerts are unaffected.
    pub fn acknowledge(&mut self, id: u64) -> Option<Alert> {
        if let Some(a) = self.active.get_mut(&id) {
            a.acknowledged = true;
            return Some(a.clone());
        }
        let a = self.resolved.iter_mut().find(|a| a.id == id)?;
        a.acknowledged = true;
        Some(a.clone())
    }

    pub fn evaluate(&mut self, observations: Vec<Observation>, now: u64) -> Vec<AlertEvent> {
        let mut events = Vec::new();
        for obs in observations {
            let key = (obs.rule.to_string(), obs.subject.clone());
            let state = self.states.remove(&key);
            let next = match (state, obs.level) {
                (None, Level::Triggered(severity)) => {
                    self.pending_or_raise(&key, now, severity, now, &obs, &mut events)
                }
                (None, _) => None,
                (Some(State::Pending { since, severity }), Level::Triggered(new)) => {
                    self.pending_or_raise(&key, since, severity.max(new), now, &obs, &mut events)
                }
                (Some(State::Pending { .. }), Level::Clear | Level::Band) => None,
                (Some(p @ State::Pending { .. }), Level::Unknown) => Some(p),
                (Some(State::Active { id, .. }), Level::Triggered(severity)) => {
                    if let Some(alert) = self.active.get_mut(&id) {
                        alert.last_updated_at = now;
                        alert.evidence = obs.evidence.clone();
                        if severity > alert.severity {
                            alert.severity = severity;
                            alert.title = obs.title.clone();
                            alert.explanation = obs.explanation.clone();
                            alert.threshold = obs.threshold.clone();
                            alert.acknowledged = false;
                            alert.notified = true;
                            events.push(AlertEvent::Escalated {
                                alert: alert.clone(),
                                notify: true,
                            });
                        }
                    }
                    Some(State::Active {
                        id,
                        clear_since: None,
                    })
                }
                (Some(State::Active { id, .. }), Level::Band) => Some(State::Active {
                    id,
                    clear_since: None,
                }),
                (Some(State::Active { id, clear_since }), Level::Clear) => {
                    let since = clear_since.unwrap_or(now);
                    if now.saturating_sub(since) >= obs.clear_after_ms {
                        if let Some(mut alert) = self.active.remove(&id) {
                            alert.status = AlertStatus::Resolved;
                            alert.resolved_at = Some(now);
                            alert.last_updated_at = now;
                            alert.evidence = obs.evidence.clone();
                            self.last_resolved_at.insert(key.clone(), now);
                            self.resolved.push_front(alert.clone());
                            self.resolved.truncate(RESOLVED_KEPT);
                            events.push(AlertEvent::Resolved { alert });
                        }
                        None
                    } else {
                        Some(State::Active {
                            id,
                            clear_since: Some(since),
                        })
                    }
                }
                (Some(a @ State::Active { .. }), Level::Unknown) => Some(a),
            };
            if let Some(next) = next {
                self.states.insert(key, next);
            }
        }
        events
    }

    fn pending_or_raise(
        &mut self,
        key: &(String, Option<String>),
        since: u64,
        severity: AlertSeverity,
        now: u64,
        obs: &Observation,
        events: &mut Vec<AlertEvent>,
    ) -> Option<State> {
        if now.saturating_sub(since) < obs.duration_ms {
            return Some(State::Pending { since, severity });
        }
        let id = self.next_id;
        self.next_id += 1;
        let in_cooldown = self
            .last_resolved_at
            .get(key)
            .is_some_and(|t| now.saturating_sub(*t) < self.cooldown_ms);
        let notify = !in_cooldown;
        let alert = Alert {
            id,
            rule: obs.rule.to_string(),
            category: obs.category,
            severity,
            title: obs.title.clone(),
            explanation: obs.explanation.clone(),
            recommendation: obs.recommendation.clone(),
            metric: obs.metric.clone(),
            subject: obs.subject.clone(),
            threshold: obs.threshold.clone(),
            evidence: obs.evidence.clone(),
            detected_at: since,
            last_updated_at: now,
            resolved_at: None,
            status: AlertStatus::Active,
            acknowledged: false,
            notified: notify,
            threshold_source: obs.threshold_source,
        };
        self.active.insert(id, alert.clone());
        events.push(AlertEvent::Raised { alert, notify });
        Some(State::Active {
            id,
            clear_since: None,
        })
    }

    /// Monitoring stopped: pending conditions are forgotten; active alerts
    /// stay visible (they are resolved when monitoring sees them clear).
    pub fn reset_pending(&mut self) {
        self.states.retain(|_, s| matches!(s, State::Active { .. }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(level: Level) -> Observation {
        Observation {
            rule: "cpu_high",
            subject: None,
            category: AlertCategory::Cpu,
            metric: "cpu.total".into(),
            level,
            duration_ms: 120_000,
            clear_after_ms: 30_000,
            title: "High CPU".into(),
            explanation: "x".into(),
            recommendation: None,
            threshold: "90%".into(),
            threshold_source: ThresholdSource::Default,
            evidence: "e".into(),
        }
    }
    const W: Level = Level::Triggered(AlertSeverity::Warning);

    #[test]
    fn a_condition_from_an_earlier_session_is_not_notified_again_within_the_cooldown() {
        let mut e = Evaluator::new(10, 600_000);
        // The previous daemon resolved this alert when it stopped, at t = 50 s.
        e.remember_resolved("cpu_high", None, 50_000);
        e.evaluate(vec![obs(W)], 60_000);
        let ev = e.evaluate(vec![obs(W)], 180_000);
        assert!(matches!(
            &ev[..],
            [AlertEvent::Raised { notify: false, .. }]
        ));
        // Recorded all the same.
        assert_eq!(e.active().len(), 1);
        assert!(!e.active()[0].notified);

        // Long after the cooldown it notifies as usual.
        let mut e = Evaluator::new(10, 600_000);
        e.remember_resolved("cpu_high", None, 50_000);
        e.evaluate(vec![obs(W)], 700_000);
        let ev = e.evaluate(vec![obs(W)], 820_000);
        assert!(matches!(&ev[..], [AlertEvent::Raised { notify: true, .. }]));
    }

    #[test]
    fn a_short_spike_is_not_an_alert() {
        let mut e = Evaluator::new(1, 600_000);
        assert!(e.evaluate(vec![obs(W)], 0).is_empty());
        assert!(e.evaluate(vec![obs(W)], 60_000).is_empty());
        assert!(e.evaluate(vec![obs(Level::Clear)], 62_000).is_empty());
        assert!(
            e.evaluate(vec![obs(W)], 64_000).is_empty(),
            "pending restarts"
        );
        assert!(e.active().is_empty());
    }

    #[test]
    fn sustained_condition_raises_once_then_resolves_with_hysteresis() {
        let mut e = Evaluator::new(1, 600_000);
        e.evaluate(vec![obs(W)], 0);
        let ev = e.evaluate(vec![obs(W)], 120_000);
        assert!(matches!(&ev[..], [AlertEvent::Raised { notify: true, .. }]));
        assert_eq!(e.active()[0].detected_at, 0);
        // Still high: no duplicate alert or notification.
        assert!(e.evaluate(vec![obs(W)], 122_000).is_empty());
        assert_eq!(e.active().len(), 1);
        // In the hysteresis band: stays active.
        assert!(e.evaluate(vec![obs(Level::Band)], 124_000).is_empty());
        // Clear, but not yet for 30 s.
        assert!(e.evaluate(vec![obs(Level::Clear)], 126_000).is_empty());
        // Back in the band resets the clear timer.
        e.evaluate(vec![obs(Level::Band)], 140_000);
        assert!(e.evaluate(vec![obs(Level::Clear)], 150_000).is_empty());
        let ev = e.evaluate(vec![obs(Level::Clear)], 180_000);
        match &ev[..] {
            [AlertEvent::Resolved { alert }] => {
                assert_eq!(alert.status, AlertStatus::Resolved);
                assert_eq!(alert.resolved_at, Some(180_000));
            }
            other => panic!("{other:?}"),
        }
        assert!(e.active().is_empty());
        assert_eq!(e.recently_resolved().len(), 1);
    }

    #[test]
    fn missing_data_changes_nothing() {
        let mut e = Evaluator::new(1, 0);
        e.evaluate(vec![obs(W)], 0);
        e.evaluate(vec![obs(W)], 120_000);
        for t in [130_000, 500_000] {
            assert!(e.evaluate(vec![obs(Level::Unknown)], t).is_empty());
        }
        assert_eq!(
            e.active().len(),
            1,
            "an unknown reading does not resolve an alert"
        );
    }

    #[test]
    fn escalation_notifies_again_and_cooldown_suppresses_repeat_notifications() {
        let mut e = Evaluator::new(1, 600_000);
        let mut zero = obs(W);
        zero.duration_ms = 0;
        zero.clear_after_ms = 0;
        assert!(matches!(
            &e.evaluate(vec![zero.clone()], 0)[..],
            [AlertEvent::Raised { notify: true, .. }]
        ));
        let mut crit = zero.clone();
        crit.level = Level::Triggered(AlertSeverity::Critical);
        assert!(matches!(
            &e.evaluate(vec![crit], 1_000)[..],
            [AlertEvent::Escalated { notify: true, .. }]
        ));
        let mut clear = zero.clone();
        clear.level = Level::Clear;
        assert!(matches!(
            &e.evaluate(vec![clear], 2_000)[..],
            [AlertEvent::Resolved { .. }]
        ));
        // Comes back within the cooldown: recorded, not notified.
        match &e.evaluate(vec![zero.clone()], 3_000)[..] {
            [AlertEvent::Raised { notify, alert }] => {
                assert!(!notify);
                assert!(!alert.notified);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn acknowledging_touches_only_that_alert() {
        let mut e = Evaluator::new(1, 0);
        let mut a = obs(W);
        a.duration_ms = 0;
        let mut b = a.clone();
        b.rule = "memory_low";
        e.evaluate(vec![a.clone(), b], 0);
        let id = e.active()[0].id;
        assert!(e.acknowledge(id).unwrap().acknowledged);
        let alerts = e.active();
        assert_eq!(alerts.iter().filter(|x| x.acknowledged).count(), 1);
        // The acknowledged condition is still evaluated.
        let mut clear = a;
        clear.level = Level::Clear;
        clear.clear_after_ms = 0;
        assert_eq!(e.evaluate(vec![clear], 1_000).len(), 1);
        assert!(e.acknowledge(9_999).is_none());
    }
}
