//! The contract between the Sentinel daemon and its clients.
//!
//! Transport: a Unix domain stream socket private to the user. Framing: one
//! JSON document per line: requests at most [`MAX_FRAME_BYTES`], responses at most
//! [`MAX_RESPONSE_BYTES`]. Every request
//! and response carries the protocol version and a request identifier.
//!
//! The request set is a closed enum. There is no request that carries a
//! command line, a path to execute or any other open-ended instruction, so
//! the socket cannot be used for arbitrary command execution.

pub mod client;
pub mod codec;
pub mod protocol;

pub use client::Client;
pub use protocol::{
    DaemonStatus, ErrorCode, InventoryAnswer, IpcError, MAX_FRAME_BYTES, MAX_RESPONSE_BYTES,
    MetricsAnswer, MonitoringStatus, Outcome, PROTOCOL_VERSION, ProcessAnswer, Request,
    RequestEnvelope, Response, ResponseEnvelope, UtilitiesAnswer,
};
