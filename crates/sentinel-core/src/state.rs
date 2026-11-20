//! Lifecycle state machines. Both are pure so every transition is unit-tested
//! without a running daemon.

use serde::{Deserialize, Serialize};

use crate::error::{Result, SentinelError};

/// Lifecycle of the daemon process itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonState {
    Starting,
    Running,
    ShuttingDown,
    Stopped,
}

impl DaemonState {
    /// Move to `next`, or explain why that is not a legal step.
    ///
    /// Legal path: `Starting -> Running -> ShuttingDown -> Stopped`, plus
    /// `Starting -> ShuttingDown` for a start-up that is aborted.
    pub fn transition(self, next: DaemonState) -> Result<DaemonState> {
        use DaemonState::*;
        match (self, next) {
            (Starting, Running)
            | (Starting, ShuttingDown)
            | (Running, ShuttingDown)
            | (ShuttingDown, Stopped) => Ok(next),
            _ => Err(SentinelError::InvalidTransition(format!(
                "daemon cannot go from {self:?} to {next:?}"
            ))),
        }
    }

    pub fn accepts_requests(self) -> bool {
        self == DaemonState::Running
    }
}

/// State of the monitoring engine.
///
/// Reported from the monitor itself, never from the stored preference:
/// `Running` only once the monitor thread is sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringState {
    Stopped,
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringCommand {
    Start,
    Stop,
    Pause,
    Resume,
}

impl MonitoringState {
    /// The state `command` would lead to, if the command is legal from here.
    ///
    /// This only answers "is the request coherent"; whether an engine can
    /// actually carry it out is decided by the daemon.
    pub fn after(self, command: MonitoringCommand) -> Result<MonitoringState> {
        use MonitoringCommand::*;
        use MonitoringState::*;
        match (self, command) {
            (Stopped, Start) => Ok(Running),
            // Stopping is idempotent: the requested state already holds.
            (Stopped, Stop) | (Running, Stop) | (Paused, Stop) => Ok(Stopped),
            (Running, Pause) => Ok(Paused),
            (Paused, Resume) => Ok(Running),
            (Running, Start) => Err(invalid("monitoring is already running")),
            (Paused, Start) => Err(invalid("monitoring is paused; resume it instead")),
            (Stopped, Pause) | (Paused, Pause) => Err(invalid("monitoring is not running")),
            (Stopped, Resume) | (Running, Resume) => Err(invalid("monitoring is not paused")),
        }
    }
}

fn invalid(reason: &str) -> SentinelError {
    SentinelError::InvalidTransition(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_follows_the_happy_path() {
        let state = DaemonState::Starting
            .transition(DaemonState::Running)
            .and_then(|s| s.transition(DaemonState::ShuttingDown))
            .and_then(|s| s.transition(DaemonState::Stopped))
            .unwrap();
        assert_eq!(state, DaemonState::Stopped);
    }

    #[test]
    fn daemon_start_up_can_be_aborted() {
        assert!(
            DaemonState::Starting
                .transition(DaemonState::ShuttingDown)
                .is_ok()
        );
    }

    #[test]
    fn daemon_rejects_illegal_transitions() {
        use DaemonState::*;
        for (from, to) in [
            (Stopped, Running),
            (Running, Starting),
            (ShuttingDown, Running),
            (Running, Stopped),
            (Running, Running),
        ] {
            assert!(from.transition(to).is_err(), "{from:?} -> {to:?} must fail");
        }
    }

    #[test]
    fn only_a_running_daemon_accepts_requests() {
        assert!(DaemonState::Running.accepts_requests());
        assert!(!DaemonState::Starting.accepts_requests());
        assert!(!DaemonState::ShuttingDown.accepts_requests());
    }

    #[test]
    fn monitoring_transitions() {
        use MonitoringCommand::*;
        use MonitoringState::*;
        assert_eq!(Stopped.after(Start).unwrap(), Running);
        assert_eq!(Running.after(Pause).unwrap(), Paused);
        assert_eq!(Paused.after(Resume).unwrap(), Running);
        for state in [Stopped, Running, Paused] {
            assert_eq!(state.after(Stop).unwrap(), Stopped);
        }
        for (state, command) in [
            (Running, Start),
            (Paused, Start),
            (Stopped, Pause),
            (Paused, Pause),
            (Stopped, Resume),
            (Running, Resume),
        ] {
            assert!(state.after(command).is_err(), "{state:?} + {command:?}");
        }
    }

    #[test]
    fn states_serialise_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&DaemonState::ShuttingDown).unwrap(),
            "\"shutting_down\""
        );
        assert_eq!(
            serde_json::to_string(&MonitoringState::Paused).unwrap(),
            "\"paused\""
        );
    }
}
