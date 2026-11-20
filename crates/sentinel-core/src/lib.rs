//! Shared foundation for every Sentinel component.
//!
//! This crate deliberately has no knowledge of IPC, storage or the UI. It
//! holds the things all of them must agree on: the error type, the
//! application's directories, the lifecycle state machines and the platform
//! guards.
//!
//! Safety note: nothing in this crate (or any Sentinel crate) modifies the
//! host system. The only filesystem writes are to Sentinel's own directories
//! resolved by [`paths::AppPaths`].

pub mod error;
pub mod instance;
pub mod paths;
pub mod platform;
pub mod state;

pub use error::{Result, SentinelError};
pub use paths::AppPaths;
pub use state::{DaemonState, MonitoringCommand, MonitoringState};

/// Human-readable application name.
pub const APP_NAME: &str = "Sentinel";

/// Directory name used under each XDG base directory.
pub const APP_DIR_NAME: &str = "sentinel";

/// Version of the Sentinel workspace, shared by every binary.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
