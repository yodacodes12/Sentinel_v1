//! Remaining battery runtime: remaining energy / average discharge power.
//!
//! The estimate is withheld until there are enough valid discharge samples
//! spanning enough time, and it is refused when power readings are too
//! unstable to be meaningful. It always carries a confidence level and is
//! labelled as an estimate. Charge-reporting batteries use remaining charge
//! / average discharge current, which is the same calculation in Ah and A.

use std::collections::VecDeque;

use crate::model::{EstimateConfidence, RuntimeEstimate};

#[derive(Debug, Clone)]
pub struct RuntimeEstimator {
    /// `(monotonic seconds, rate)`; rate is W (or A).
    samples: VecDeque<(f64, f64)>,
    window_s: f64,
    min_samples: u32,
    min_span_s: f64,
}

impl Default for RuntimeEstimator {
    fn default() -> Self {
        Self::new(300.0, 4, 60.0)
    }
}

/// Discharge rates below this are treated as "not a valid reading" (some
/// firmware reports 0 or a few mW right after unplugging).
const MIN_VALID_RATE: f64 = 0.05;
/// Relative spread above which no estimate is shown.
const MAX_VARIATION: f64 = 0.8;

impl RuntimeEstimator {
    pub fn new(window_s: f64, min_samples: u32, min_span_s: f64) -> Self {
        Self {
            samples: VecDeque::new(),
            window_s,
            min_samples,
            min_span_s,
        }
    }

    /// Feed one battery reading and get the current estimate.
    ///
    /// `remaining` is energy now (Wh) or charge now (Ah); `rate` is power
    /// (W) or current (A), in the same family.
    pub fn update(
        &mut self,
        t: f64,
        discharging: bool,
        remaining: Option<f64>,
        rate: Option<f64>,
    ) -> RuntimeEstimate {
        self.update_scaled(t, discharging, remaining, rate, Some(1.0))
    }

    /// As [`Self::update`], with `watts_per_unit` converting the average
    /// rate to watts for display: the present voltage for a battery that
    /// reports current, `None` when that is unknown.
    pub fn update_scaled(
        &mut self,
        t: f64,
        discharging: bool,
        remaining: Option<f64>,
        rate: Option<f64>,
        watts_per_unit: Option<f64>,
    ) -> RuntimeEstimate {
        if !discharging {
            self.samples.clear();
            return RuntimeEstimate::NotDischarging;
        }
        if let Some(rate) = rate
            .map(f64::abs)
            .filter(|r| r.is_finite() && *r >= MIN_VALID_RATE)
        {
            self.samples.push_back((t, rate));
        }
        while let Some((first, _)) = self.samples.front() {
            if t - first > self.window_s {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        let Some(remaining) = remaining.filter(|r| r.is_finite() && *r >= 0.0) else {
            return RuntimeEstimate::Insufficient {
                reason: "The battery does not report its remaining energy or charge.".into(),
            };
        };
        let n = self.samples.len() as u32;
        let span = match (self.samples.front(), self.samples.back()) {
            (Some(a), Some(b)) => b.0 - a.0,
            _ => 0.0,
        };
        if n < self.min_samples || span < self.min_span_s {
            if n == 0 && rate.is_none() {
                return RuntimeEstimate::Insufficient {
                    reason: "The battery does not report its discharge power or current.".into(),
                };
            }
            return RuntimeEstimate::Calculating {
                samples: n,
                needed: self.min_samples,
            };
        }
        let rates: Vec<f64> = self.samples.iter().map(|(_, r)| *r).collect();
        let mean = rates.iter().sum::<f64>() / rates.len() as f64;
        let variance = rates.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rates.len() as f64;
        let variation = variance.sqrt() / mean;
        if variation > MAX_VARIATION {
            return RuntimeEstimate::Insufficient {
                reason: "Power use is changing too much for a reliable estimate.".into(),
            };
        }
        let confidence = if variation < 0.15 && n >= 8 {
            EstimateConfidence::High
        } else if variation < 0.35 {
            EstimateConfidence::Medium
        } else {
            EstimateConfidence::Low
        };
        // Rounded to whole minutes: anything finer would be false precision.
        let seconds = ((remaining / mean * 3600.0) / 60.0).round() as u64 * 60;
        RuntimeEstimate::Estimated {
            seconds,
            confidence,
            average_power_w: watts_per_unit.map(|k| crate::model::round2(mean * k)),
            samples: n,
            window_s: span.round() as u64,
        }
    }
}

/// Full capacity as a share of design capacity, only from a matching pair.
pub fn health_percent(full: Option<f64>, design: Option<f64>) -> Option<f64> {
    match (full, design) {
        (Some(f), Some(d)) if f > 0.0 && d > 0.0 => Some(crate::model::round1(f / d * 100.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_discharging_resets() {
        let mut e = RuntimeEstimator::default();
        assert_eq!(
            e.update(0.0, false, Some(40.0), Some(10.0)),
            RuntimeEstimate::NotDischarging
        );
    }

    #[test]
    fn waits_for_enough_samples_and_time() {
        let mut e = RuntimeEstimator::default();
        for i in 0..3 {
            assert!(matches!(
                e.update(i as f64 * 30.0, true, Some(40.0), Some(10.0)),
                RuntimeEstimate::Calculating { .. }
            ));
        }
        // Four samples but only 20 s apart in total: still calculating.
        let mut quick = RuntimeEstimator::default();
        for i in 0..5 {
            assert!(matches!(
                quick.update(i as f64 * 5.0, true, Some(40.0), Some(10.0)),
                RuntimeEstimate::Calculating { .. }
            ));
        }
    }

    #[test]
    fn steady_discharge_gives_a_confident_estimate() {
        let mut e = RuntimeEstimator::default();
        let mut last = RuntimeEstimate::NotDischarging;
        for i in 0..10 {
            last = e.update(
                i as f64 * 15.0,
                true,
                Some(40.0),
                Some(10.0 + (i % 2) as f64 * 0.2),
            );
        }
        match last {
            RuntimeEstimate::Estimated {
                seconds,
                confidence,
                average_power_w,
                samples,
                ..
            } => {
                // 40 Wh at ~10.1 W is about 3 h 58 min.
                assert_eq!(seconds, 238 * 60);
                assert_eq!(confidence, EstimateConfidence::High);
                assert_eq!(average_power_w, Some(10.1));
                assert_eq!(samples, 10);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn invalid_and_unstable_readings_are_not_estimated() {
        let mut e = RuntimeEstimator::default();
        // Zero readings are ignored entirely.
        for i in 0..10 {
            let r = e.update(i as f64 * 15.0, true, Some(40.0), Some(0.0));
            assert!(
                matches!(r, RuntimeEstimate::Calculating { samples: 0, .. }),
                "{r:?}"
            );
        }
        let mut wild = RuntimeEstimator::default();
        let mut last = RuntimeEstimate::NotDischarging;
        for (i, p) in [1.0, 30.0, 0.5, 40.0, 1.0, 35.0].iter().enumerate() {
            last = wild.update(i as f64 * 20.0, true, Some(40.0), Some(*p));
        }
        assert!(
            matches!(last, RuntimeEstimate::Insufficient { .. }),
            "{last:?}"
        );
        let mut no_energy = RuntimeEstimator::default();
        assert!(matches!(
            no_energy.update(0.0, true, None, Some(5.0)),
            RuntimeEstimate::Insufficient { .. }
        ));
        let mut no_rate = RuntimeEstimator::default();
        assert!(matches!(
            no_rate.update(0.0, true, Some(40.0), None),
            RuntimeEstimate::Insufficient { .. }
        ));
    }

    #[test]
    fn health() {
        assert_eq!(health_percent(Some(2.988), Some(3.645)), Some(82.0));
        assert_eq!(health_percent(Some(1.0), None), None);
        assert_eq!(health_percent(Some(1.0), Some(0.0)), None);
    }
}
