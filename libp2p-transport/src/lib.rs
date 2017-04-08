//! Transport and I/O primitives for libp2p.

extern crate futures;
extern crate tokio_io;

/// Multi-address re-export.
pub extern crate multiaddr;

use futures::*;
use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};

// libp2p interfaces built around futures-rs
// tokio was considered, but is primarily focused on request-response systems for the time being,
// while libp2p is more. We may provide a tokio-based wrapper for simple protocols in the future.

// Something more strongly-typed?
pub type ProtocolId = String;
pub type PeerId = String;

/// A logical wire between us and a peer. We can read and write through this asynchronously.
///
/// You can have multiple `Socket`s between you and any given peer.
pub trait Socket: AsyncRead + AsyncWrite { 
    /// Get the protocol ID this socket uses.
    fn protocol_id(&self) -> ProtocolId;

    /// Access the underlying connection.
    fn conn(&self) -> &Conn<Socket=Self>;
}

/// A connection between you and a peer.
pub trait Conn {
    /// The socket type this connection manages.
    type Socket;

    /// Initiate a socket between you and the peer on the given protocol.
    fn make_socket(&self, proto: ProtocolId) -> BoxFuture<Self::Socket, IoError>;
}

pub struct MultiAddr; // stub for multiaddr crate type.

/// A transport is a stream producing incoming connections.
/// These are transports or wrappers around them.
//
// Listening/Dialing hasn't really been sorted out yet.
// It's easy to do for simple multiaddrs, but for complex ones,
// particularly those with multiple hops, things get much fuzzier.
//
// One example which is difficult to make work is something like 
// `ip4/1.2.3.4/tcp/8888/p2p-circuit/p2p/DestPeer`
// 
// This address, when used for dialing, says "Connect to the peer DestPeer
// on any available address, through a relay node we will connect to via 
// tcp on port 8888 over the ipv4 address 1.2.3.4"
//
// We'll need to require dialers to handle the whole address,
// and give them a closure or similar required to instantiate connections 
// to different multi-addresses.
pub trait Transport {
    /// The raw connection.
    type RawConn: AsyncRead + AsyncWrite;

    /// The listener produces incoming connections.
    type Listener: Stream<Item=Self::RawConn>;

    /// A future which indicates currently dialing to a peer.
    type Dial: IntoFuture<Item=Self::RawConn, Error=IoError>;

    /// Listen on the given multi-addr.
    /// Returns the address back if it isn't supported.
    fn listen_on(&mut self, addr: MultiAddr) -> Result<Self::Listener, MultiAddr>;

    /// Dial to the given multi-addr.
    /// Returns either a future which may resolve to a connection,
    /// or gives back the multiaddress.
    fn dial(&mut self, addr: MultiAddr) -> Result<Self::Dial, MultiAddr>;
}