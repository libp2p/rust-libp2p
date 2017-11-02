//! Transport and I/O primitives for libp2p.

extern crate futures;
extern crate tokio_io;

/// Multi-address re-export.
pub extern crate multiaddr;

pub mod transport;

pub use self::transport::{ProtocolId, PeerId, Socket, Conn, Transport};
