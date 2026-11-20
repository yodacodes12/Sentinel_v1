//! Sentinel's telemetry engine: read-only collectors, the central sampling
//! scheduler, time-series buffers and the shared telemetry model.
//! See `docs/TELEMETRY_ENGINE.md` and `docs/METRIC_REFERENCE.md`.

pub mod collectors;
pub mod counters;
pub mod engine;
pub mod model;
pub mod runtime;
pub mod scheduler;
pub mod series;

pub use engine::Engine;
pub use model::*;
pub use scheduler::Scheduler;
