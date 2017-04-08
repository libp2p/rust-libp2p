//! "Host" abstraction for transports, listener addresses, peer store.

extern crate futures;
extern crate libp2p_transport as transport;

use futures::{Future, IntoFuture, BoxFuture};
use transport::{ProtocolId, MultiAddr, Socket};

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

/// Unimplemented. Maps peer IDs to connected addresses, protocols, and data.
pub trait PeerStore {}
  
/// This is a common abstraction over the low-level bits of libp2p.
///
/// It handles connecting over, adding and removing transports,
/// wraps an arbitrary event loop, and manages protocol IDs.
pub trait Host {
    type Socket: Socket;

    /// Get a handle to the peer store.
    fn peer_store(&self) -> &PeerStore;

    /// Get a handle to the underlying muxer.
    fn mux(&self) -> &Mux<Socket=Self::Socket>;

    /// Set the socket handler for a given protocol id.
    fn set_handler(&self, proto: ProtocolId, handler: BoxHandler<Self::Socket>) {
        self.mux().set_handler(proto, handler);
    }

    /// Remove the socket handler for the given protocol id, returning the old handler if it existed.
    fn remove_handler(&self, proto: &ProtocolId) -> Option<BoxHandler<Self::Socket>> {
        self.mux().remove_handler(proto)
    }

    /// Addresses we're listening on.
    fn listen_addrs(&self) -> Vec<MultiAddr>;
}