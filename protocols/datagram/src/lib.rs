//! Unreliable datagrams over libp2p connections.
//!
//! Datagrams may be dropped, reordered, or duplicated; the caller owns
//! reliability. Only QUIC carries them today; sends on other transports fail.

mod behaviour;
mod control;
mod handler;

pub use behaviour::{Behaviour, IncomingDatagrams};
pub use control::{Control, SendError};
