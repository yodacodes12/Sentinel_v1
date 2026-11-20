//! The Sentinel daemon.
//!
//! A single-threaded, event-driven process that runs as the logged-in user,
//! listens on a private Unix socket and answers lifecycle, status and
//! configuration requests. When no client is talking to it, it is parked in
//! `epoll_wait` and does nothing: no timers, no polling, no scanning.
//!
//! Hardware and utility scans run only when a client asks, on a worker
//! thread that exists only while a scan runs ([`discovery`]). Monitoring
//! ([`monitoring`]) runs the telemetry engine on its own thread, which
//! exists only while monitoring is on.

pub mod context;
pub mod discovery;
pub mod documents;
pub mod handler;
pub mod monitoring;
pub mod run;
pub mod server;
pub mod socket;
pub mod telemetry_requests;

pub use run::{DaemonOptions, run};
