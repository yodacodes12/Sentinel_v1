//! Sentinel's user configuration: a small, versioned TOML file.
//!
//! Loading never fails the caller into an unusable state:
//!
//! * missing file  -> defaults (optionally written to disk);
//! * corrupt file  -> the file is moved aside, untouched, and defaults are used;
//! * older version -> migrated forward;
//! * newer version -> left alone; defaults are used in memory and saving is
//!   refused so a newer Sentinel's settings are never clobbered.
//!
//! Every value takes effect: `monitoring_enabled` decides whether the daemon
//! monitors at start-up, `power_mode` is the sampling profile, and the
//! `alerts` and `retention` sections drive the alert engine and history.

mod model;
mod store;

pub use model::{
    AlertThresholds, CONFIG_VERSION, ConfigPatch, DocumentSettings, PowerMode, Retention,
    SentinelConfig, Theme,
};
pub use store::{ConfigStore, LoadStatus, parse};
