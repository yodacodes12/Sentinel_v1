//! Motherboard documentation discovery and verification (Phase 6).
//!
//! Opt-in and read-only: nothing here changes the system. Online discovery
//! is off by default; when it is off only the user's local document library
//! is searched. The network is touched only by [`net`].

pub mod classify;
pub mod identity;
pub mod matching;
pub mod model;
pub mod net;
pub mod query;
pub mod trust;
pub mod vendors;

pub use model::*;
pub mod download;
pub mod engine;
pub mod provider;
pub mod providers;
