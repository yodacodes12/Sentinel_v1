//! Read-only Linux hardware discovery.
//!
//! [`manager::discover`] runs the modules in [`modules`] against a
//! [`source::SysRoot`] and returns a [`sentinel_inventory::SystemInventory`].
//! Everything here reads; nothing writes, executes or changes the system.
//! See `docs/HARDWARE_DISCOVERY.md`.

pub mod capabilities;
pub mod context;
pub mod fields;
pub mod manager;
pub mod modules;
pub mod parsers;
pub mod source;

pub use context::{CancelToken, DiscoveryContext, Stopped};
pub use manager::{Event, ScanOutcome, discover};
pub use source::SysRoot;
