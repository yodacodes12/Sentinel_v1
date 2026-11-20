//! The single error type shared by the daemon, CLI and desktop shell.
//!
//! Every variant carries a message an ordinary user can act on. Lower-level
//! detail (the `source` errors) is kept for local logs and is never required
//! to understand the message.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, SentinelError>;

#[derive(Debug, Error)]
pub enum SentinelError {
    /// No daemon is listening on the IPC socket.
    #[error("the Sentinel daemon is not running")]
    DaemonUnavailable,

    /// A daemon already holds the instance lock.
    #[error("a Sentinel daemon is already running for this user")]
    AlreadyRunning,

    /// The socket exists but the conversation failed.
    #[error("could not communicate with the Sentinel daemon: {0}")]
    Ipc(String),

    /// The peer sent something that is not valid protocol.
    #[error("invalid IPC message: {0}")]
    Protocol(String),

    /// The daemon understood the request and refused or failed it.
    #[error("{message}")]
    Daemon { code: String, message: String },

    #[error("timed out {0}")]
    Timeout(String),

    #[error("permission denied for {}", path.display())]
    PermissionDenied {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("could not prepare directory {}: {reason}", path.display())]
    Directory { path: PathBuf, reason: String },

    #[error("storage error: {0}")]
    Storage(String),

    #[error("could not launch {what}: {reason}")]
    Launch { what: String, reason: String },

    #[error("unsupported environment: {0}")]
    UnsupportedPlatform(String),

    #[error("invalid state transition: {0}")]
    InvalidTransition(String),

    #[error("logging could not be initialised: {0}")]
    Logging(String),

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
}

impl SentinelError {
    /// Wrap an I/O error, promoting permission failures to their own variant
    /// so callers can explain them properly.
    pub fn io(context: impl Into<String>, path: impl Into<PathBuf>, source: io::Error) -> Self {
        if source.kind() == io::ErrorKind::PermissionDenied {
            SentinelError::PermissionDenied {
                path: path.into(),
                source,
            }
        } else {
            SentinelError::Io {
                context: context.into(),
                source,
            }
        }
    }

    /// Process exit code for the CLI and daemon binaries.
    ///
    /// `2` is reserved for argument errors (emitted by clap).
    pub fn exit_code(&self) -> u8 {
        match self {
            SentinelError::DaemonUnavailable => 3,
            SentinelError::Ipc(_) | SentinelError::Protocol(_) => 4,
            SentinelError::InvalidConfig(_) => 5,
            SentinelError::PermissionDenied { .. } => 6,
            SentinelError::UnsupportedPlatform(_) => 7,
            SentinelError::Launch { .. } => 8,
            SentinelError::Timeout(_) => 9,
            SentinelError::AlreadyRunning => 10,
            SentinelError::Daemon { .. } => 11,
            SentinelError::Directory { .. }
            | SentinelError::Storage(_)
            | SentinelError::InvalidTransition(_)
            | SentinelError::Logging(_)
            | SentinelError::Io { .. } => 1,
        }
    }

    /// Stable machine-readable identifier, used by the desktop bridge so the
    /// frontend can react to a category without parsing prose.
    pub fn kind(&self) -> &'static str {
        match self {
            SentinelError::DaemonUnavailable => "daemon_unavailable",
            SentinelError::AlreadyRunning => "already_running",
            SentinelError::Ipc(_) => "ipc",
            SentinelError::Protocol(_) => "protocol",
            SentinelError::Daemon { .. } => "daemon",
            SentinelError::Timeout(_) => "timeout",
            SentinelError::PermissionDenied { .. } => "permission_denied",
            SentinelError::InvalidConfig(_) => "invalid_config",
            SentinelError::Directory { .. } => "directory",
            SentinelError::Storage(_) => "storage",
            SentinelError::Launch { .. } => "launch",
            SentinelError::UnsupportedPlatform(_) => "unsupported_platform",
            SentinelError::InvalidTransition(_) => "invalid_transition",
            SentinelError::Logging(_) => "logging",
            SentinelError::Io { .. } => "io",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_errors_are_promoted() {
        let err = SentinelError::io(
            "open",
            "/x/y",
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert!(matches!(err, SentinelError::PermissionDenied { .. }));
        assert_eq!(err.exit_code(), 6);
        assert_eq!(err.to_string(), "permission denied for /x/y");
    }

    #[test]
    fn other_io_errors_keep_context() {
        let err = SentinelError::io("reading config", "/x", io::Error::other("boom"));
        assert!(matches!(err, SentinelError::Io { .. }));
        assert_eq!(err.to_string(), "reading config: boom");
    }

    #[test]
    fn exit_codes_are_distinct_for_user_facing_categories() {
        let codes = [
            SentinelError::DaemonUnavailable.exit_code(),
            SentinelError::Ipc(String::new()).exit_code(),
            SentinelError::InvalidConfig(String::new()).exit_code(),
            SentinelError::UnsupportedPlatform(String::new()).exit_code(),
            SentinelError::Launch {
                what: String::new(),
                reason: String::new(),
            }
            .exit_code(),
            SentinelError::Timeout(String::new()).exit_code(),
            SentinelError::AlreadyRunning.exit_code(),
        ];
        let mut unique = codes.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), codes.len());
        // 0 is success and 2 belongs to clap's usage errors.
        assert!(!codes.contains(&0) && !codes.contains(&2));
    }

    #[test]
    fn messages_do_not_leak_debug_formatting() {
        let err = SentinelError::Daemon {
            code: "monitoring_unavailable".into(),
            message: "the monitoring engine is not available".into(),
        };
        assert_eq!(err.to_string(), "the monitoring engine is not available");
        assert_eq!(err.kind(), "daemon");
    }
}
