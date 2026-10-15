//! Unreliable datagrams over libp2p connections, per [libp2p/specs#680].
//!
//! Datagrams ride a QUIC `/dg/1` control stream that pins them to one
//! application protocol; they may be dropped, reordered, or duplicated, and the
//! caller owns reliability. Only QUIC carries them today; other transports drop.
//!
//! [libp2p/specs#680]: https://github.com/libp2p/specs/pull/680

mod behaviour;
mod control;
mod framing;
mod handler;
mod protocol;

pub use behaviour::{Behaviour, IncomingDatagrams};
pub use control::{Control, SendError};
