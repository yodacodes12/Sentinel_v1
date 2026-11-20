//! The alert engine: rules + evaluator + the "repeated thermal warnings"
//! meta-rule, configured by the user's thresholds.

use std::collections::VecDeque;

use sentinel_config::AlertThresholds;
use sentinel_telemetry::model::{AlertCategory, AlertSeverity, TelemetrySnapshot, ThresholdSource};

use crate::evaluator::{AlertEvent, Evaluator, Level, Observation};
use crate::rules;

const REPEAT_WINDOW_MS: u64 = 3_600_000;
const REPEAT_COUNT: usize = 3;

pub struct AlertEngine {
    pub evaluator: Evaluator,
    thresholds: AlertThresholds,
    thermal_raises: VecDeque<u64>,
}

impl AlertEngine {
    pub fn new(thresholds: AlertThresholds, first_id: u64) -> Self {
        let cooldown = u64::from(thresholds.cooldown_s) * 1000;
        Self {
            evaluator: Evaluator::new(first_id, cooldown),
            thresholds,
            thermal_raises: VecDeque::new(),
        }
    }

    pub fn thresholds(&self) -> &AlertThresholds {
        &self.thresholds
    }

    pub fn set_thresholds(&mut self, thresholds: AlertThresholds) {
        self.evaluator.cooldown_ms = u64::from(thresholds.cooldown_s) * 1000;
        self.thresholds = thresholds;
    }

    pub fn evaluate(&mut self, snapshot: &TelemetrySnapshot, now: u64) -> Vec<AlertEvent> {
        let mut events = self
            .evaluator
            .evaluate(rules::observe(snapshot, &self.thresholds), now);
        for e in &events {
            if let AlertEvent::Raised { alert, .. } = e
                && alert.rule == "temperature_high"
            {
                self.thermal_raises.push_back(now);
            }
        }
        while self
            .thermal_raises
            .front()
            .is_some_and(|t| now.saturating_sub(*t) > REPEAT_WINDOW_MS)
        {
            self.thermal_raises.pop_front();
        }
        let count = self.thermal_raises.len();
        let repeated = Observation {
            rule: "thermal_repeated",
            subject: None,
            category: AlertCategory::Thermal,
            metric: "temp".into(),
            level: if count >= REPEAT_COUNT {
                Level::Triggered(AlertSeverity::Info)
            } else {
                Level::Clear
            },
            duration_ms: 0,
            clear_after_ms: 0,
            title: "Repeated temperature warnings".into(),
            explanation: format!(
                "Temperature warnings were raised {count} times in the last hour."
            ),
            recommendation: Some(
                "A recurring heat problem may point to blocked vents or a heavy regular workload."
                    .into(),
            ),
            threshold: format!("{REPEAT_COUNT} warnings within an hour"),
            threshold_source: ThresholdSource::Default,
            evidence: format!("{count} temperature warnings in the last hour."),
        };
        events.extend(self.evaluator.evaluate(vec![repeated], now));
        events
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sentinel_telemetry::model::*;

    pub fn alert() -> Alert {
        Alert {
            id: 1,
            rule: "battery_low".into(),
            category: AlertCategory::Battery,
            severity: AlertSeverity::Warning,
            title: "Low battery".into(),
            explanation: "The battery has reached the configured low-battery threshold of 15%."
                .into(),
            recommendation: Some("Consider connecting your charger.".into()),
            metric: "battery.BAT0.percent".into(),
            subject: Some("BAT0".into()),
            threshold: "≤ 15%".into(),
            evidence: "BAT0: 14%.".into(),
            detected_at: 0,
            last_updated_at: 0,
            resolved_at: None,
            status: AlertStatus::Active,
            acknowledged: false,
            notified: true,
            threshold_source: ThresholdSource::Default,
        }
    }
}
