//! Rust libp2p implementation

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
//
// What do we call it? Go-libp2p calls them "Streams", but "Streams" in futures-rs specifically
// denote the "writing" half. "Connections" have a different meaning in the libp2p definition.
// `Duplex`?
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

/// Produces a future for each incoming `Socket`.
pub trait Handler<S: Socket> {
    type Future: IntoFuture<Item=(), Error=()>;

    /// Handle the incoming socket, producing a future which should resolve 
    /// when the handler is finished.
    fn handle(&self, socket: S) -> Self::Future;
    fn boxed(self) -> BoxHandler<S> where 
        Self: Sized + Send + 'static, 
        <Self::Future as IntoFuture>::Future: Send + 'static
    {
        BoxHandler(Box::new(move |socket|
            self.handle(socket).into_future().boxed()
        ))
    }
}

impl<S: Socket, F, U> Handler<S> for F 
    where F: Fn(S) -> U, U: IntoFuture<Item=(), Error=()>
{
    type Future = U;

    fn handle(&self, socket: S) -> U { (self)(socket) }
}

/// A boxed handler.
pub struct BoxHandler<S: Socket>(Box<Handler<S, Future=BoxFuture<(), ()>>>);

impl<S: Socket> Handler<S> for BoxHandler<S> {
    type Future = BoxFuture<(), ()>;

    fn handle(&self, socket: S) -> Self::Future {
        self.0.handle(socket)
    }
}

/// Multiplexes sockets onto handlers by protocol-id.
pub trait Mux: Sync {
    /// The socket type this manages.
    type Socket: Socket;

    /// Attach an incoming socket.
    fn push(&self, socket: Self::Socket);
  
    /// Set the socket handler for a given protocol id.
    fn set_handler(&self, proto: ProtocolId, handler: BoxHandler<Self::Socket>);

    /// Remove the socket handler for the given protocol id, returning the old handler if it existed.
    fn remove_handler(&self, proto: &ProtocolId) -> Option<BoxHandler<Self::Socket>>;
}
  
pub struct MultiAddr; // stub for multiaddr crate type.

/// A transport is a stream producing incoming connections.
/// These are transports or wrappers around them.
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
    /// or 
    fn dial(&mut self, addr: MultiAddr) -> Result<Self::Dial, MultiAddr>;
}