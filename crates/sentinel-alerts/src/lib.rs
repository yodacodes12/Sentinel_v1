//! Sentinel's alert engine: rules with sustained durations, hysteresis,
//! cooldowns and recovery events; evidence-based health indicators; and
//! desktop notifications over the session D-Bus. Alerts and notifications
//! only inform. See `docs/ALERT_ENGINE.md`.

pub mod dbus;
pub mod engine;
pub mod evaluator;
pub mod health;
pub mod notifications;
pub mod rules;

pub use engine::AlertEngine;
pub use evaluator::{AlertEvent, Evaluator};
pub use notifications::{DbusNotifier, NotificationService, Notifier};
