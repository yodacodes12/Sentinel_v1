//! Sentinel's system inventory model.
//!
//! This crate holds types only: what a scan produces, where each value came
//! from ([`provenance`]), the motherboard identity contract for later
//! schematic work ([`board`]), the utility catalogue ([`utilities`]),
//! discovery progress ([`discovery`]), the hardware fingerprint
//! ([`fingerprint`]) and the cache format ([`cache`]). It performs no I/O.
//!
//! The discovery code lives in `sentinel-hardware` and `sentinel-utilities`;
//! the daemon, IPC protocol, client and desktop all share these types.

pub mod board;
pub mod cache;
pub mod components;
pub mod discovery;
pub mod fingerprint;
pub mod hardware;
pub mod provenance;
pub mod utilities;

pub use board::{BoardIdentification, BoardIdentity, IdentificationLevel};
pub use components::{
    COMPONENT_INDEX_VERSION, Component, ComponentCategory, ComponentIndex, ComponentKind, Link,
    ParentBasis, Relation, TelemetryRef,
};
pub use discovery::{
    DiscoveryJob, DiscoveryStatus, InventoryOrigin, JobProgress, JobState, ModuleId, ModuleOutcome,
    ModuleReport,
};
pub use fingerprint::HardwareFingerprint;
pub use hardware::*;
pub use provenance::{Availability, Confidence, Field};
pub use utilities::*;
