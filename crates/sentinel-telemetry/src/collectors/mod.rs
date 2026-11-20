//! One collector per area. Each owns its previous counters, reads only
//! through `SysRoot`, and reports its own health. A collector never fails
//! another: errors become reading states and collector health.

pub mod battery;
pub mod cpu;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod process;
pub mod storage;
pub mod system_load;
pub mod thermal;

use sentinel_hardware::source::ReadError;

use crate::model::{CollectorHealth, ReadingState};

/// Timing of one sample.
#[derive(Debug, Clone, Copy)]
pub struct SampleTime {
    /// Wall clock, unix milliseconds (for history and display).
    pub wall_ms: u64,
    /// Monotonic seconds (for intervals and rates).
    pub mono_s: f64,
    /// The interval the collector is scheduled at.
    pub interval_ms: u64,
}

/// What a collector run produced besides its section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub health: CollectorHealth,
    pub note: Option<String>,
}

impl RunReport {
    pub fn ok() -> Self {
        Self {
            health: CollectorHealth::Ok,
            note: None,
        }
    }
    pub fn partial(note: impl Into<String>) -> Self {
        Self {
            health: CollectorHealth::Partial,
            note: Some(note.into()),
        }
    }
    pub fn unsupported(note: impl Into<String>) -> Self {
        Self {
            health: CollectorHealth::Unsupported,
            note: Some(note.into()),
        }
    }
    pub fn failing(note: impl Into<String>) -> Self {
        Self {
            health: CollectorHealth::Failing,
            note: Some(note.into()),
        }
    }
}

pub fn state_of(error: &ReadError) -> ReadingState {
    match error {
        ReadError::Missing => ReadingState::Unsupported,
        ReadError::PermissionDenied => ReadingState::PermissionDenied,
        ReadError::Empty | ReadError::Refused => ReadingState::Unavailable,
        ReadError::TooLarge | ReadError::Other(_) => ReadingState::Error,
    }
}
