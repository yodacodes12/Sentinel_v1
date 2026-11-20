//! Delivery of alert notifications to the desktop.
//!
//! Only raised or escalated alerts that the evaluator marked for
//! notification are sent, and only when the user's preference allows it.
//! A failure is recorded and monitoring continues; the alert is still in
//! the local history.

use sentinel_telemetry::model::{Alert, AlertSeverity, NotificationStatus};

use crate::dbus::{self, BusTarget, Notification};

/// Something that can show a notification. The daemon uses the session
/// bus; tests use a recorder.
pub trait Notifier: Send {
    fn notify(&mut self, n: &Notification) -> Result<(), String>;
}

pub struct DbusNotifier {
    target: Option<BusTarget>,
    uid: u32,
}

impl DbusNotifier {
    /// From the daemon's environment: `DBUS_SESSION_BUS_ADDRESS`, falling
    /// back to `$XDG_RUNTIME_DIR/bus`.
    pub fn from_env(uid: u32) -> Self {
        let target = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .ok()
            .and_then(|a| dbus::parse_address(&a))
            .or_else(|| {
                let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
                let path = std::path::Path::new(&dir).join("bus");
                path.exists()
                    .then(|| BusTarget::Path(path.to_string_lossy().into_owned()))
            });
        Self { target, uid }
    }
}

impl Notifier for DbusNotifier {
    fn notify(&mut self, n: &Notification) -> Result<(), String> {
        match &self.target {
            Some(target) => dbus::send(target, self.uid, n),
            None => Err("no desktop session bus was found".into()),
        }
    }
}

/// Text of a notification. No actions, no commands.
pub fn notification_for(alert: &Alert) -> Notification {
    let mut body = alert.explanation.clone();
    if let Some(r) = &alert.recommendation {
        body.push_str("\n\n");
        body.push_str(r);
    }
    Notification {
        summary: format!("Sentinel — {}", alert.title),
        body,
        urgency: match alert.severity {
            AlertSeverity::Info => 0,
            AlertSeverity::Warning => 1,
            AlertSeverity::Critical => 2,
        },
    }
}

pub struct NotificationService {
    notifier: Box<dyn Notifier>,
    pub status: NotificationStatus,
}

impl NotificationService {
    pub fn new(notifier: Box<dyn Notifier>, enabled: bool) -> Self {
        Self {
            notifier,
            status: NotificationStatus {
                enabled,
                last_delivery_ok: None,
                last_error: None,
                delivered: 0,
                failed: 0,
            },
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.status.enabled = enabled;
    }

    /// Returns whether the notification was delivered.
    pub fn deliver(&mut self, alert: &Alert) -> bool {
        if !self.status.enabled {
            return false;
        }
        match self.notifier.notify(&notification_for(alert)) {
            Ok(()) => {
                self.status.delivered += 1;
                self.status.last_delivery_ok = Some(true);
                self.status.last_error = None;
                true
            }
            Err(e) => {
                self.status.failed += 1;
                self.status.last_delivery_ok = Some(false);
                self.status.last_error = Some(e);
                false
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct Recorder(pub Arc<Mutex<Vec<Notification>>>, pub bool);

    impl Notifier for Recorder {
        fn notify(&mut self, n: &Notification) -> Result<(), String> {
            if self.1 {
                return Err("bus down".into());
            }
            self.0.lock().unwrap().push(n.clone());
            Ok(())
        }
    }

    #[test]
    fn respects_preference_and_records_failures() {
        let rec = Recorder::default();
        let alert = crate::engine::tests::alert();
        let mut svc = NotificationService::new(Box::new(rec.clone()), false);
        assert!(!svc.deliver(&alert));
        assert!(
            rec.0.lock().unwrap().is_empty(),
            "disabled means nothing is sent"
        );
        svc.set_enabled(true);
        assert!(svc.deliver(&alert));
        let sent = rec.0.lock().unwrap()[0].clone();
        assert_eq!(sent.summary, "Sentinel — Low battery");
        assert!(sent.body.contains("Consider connecting your charger."));
        let mut broken =
            NotificationService::new(Box::new(Recorder(Default::default(), true)), true);
        assert!(!broken.deliver(&alert));
        assert_eq!(broken.status.failed, 1);
        assert_eq!(broken.status.last_error.as_deref(), Some("bus down"));
    }
}
