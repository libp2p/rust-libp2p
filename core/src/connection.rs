// Copyright 2020 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

mod error;
mod handler;
mod listeners;
mod substream;

pub(crate) mod manager;
pub(crate) mod pool;

pub use error::{ConnectionError, PendingConnectionError};
pub use handler::{ConnectionHandler, ConnectionHandlerEvent, IntoConnectionHandler};
pub use listeners::{ListenerId, ListenersStream, ListenersEvent};
pub use manager::ConnectionId;
pub use substream::{Substream, SubstreamEndpoint, Close};
pub use pool::{EstablishedConnection, EstablishedConnectionIter, PendingConnection};

use crate::muxing::StreamMuxer;
use crate::{Multiaddr, PeerId};
use std::{error::Error, fmt, pin::Pin, task::Context, task::Poll};
use std::hash::Hash;
use substream::{Muxing, SubstreamEvent};

/// The endpoint roles associated with a peer-to-peer communication channel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// The socket comes from a dialer.
    Dialer,
    /// The socket comes from a listener.
    Listener,
}

impl std::ops::Not for Endpoint {
    type Output = Endpoint;

    fn not(self) -> Self::Output {
        match self {
            Endpoint::Dialer => Endpoint::Listener,
            Endpoint::Listener => Endpoint::Dialer
        }
    }
}

impl Endpoint {
    /// Is this endpoint a dialer?
    pub fn is_dialer(self) -> bool {
        if let Endpoint::Dialer = self {
            true
        } else {
            false
        }
    }

    /// Is this endpoint a listener?
    pub fn is_listener(self) -> bool {
        if let Endpoint::Listener = self {
            true
        } else {
            false
        }
    }
}

/// The endpoint roles associated with a peer-to-peer connection.
#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum ConnectedPoint {
    /// We dialed the node.
    Dialer {
        /// Multiaddress that was successfully dialed.
        address: Multiaddr,
    },
    /// We received the node.
    Listener {
        /// Local connection address.
        local_addr: Multiaddr,
        /// Stack of protocols used to send back data to the remote.
        send_back_addr: Multiaddr,
    }
}

impl From<&'_ ConnectedPoint> for Endpoint {
    fn from(endpoint: &'_ ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl From<ConnectedPoint> for Endpoint {
    fn from(endpoint: ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl ConnectedPoint {
    /// Turns the `ConnectedPoint` into the corresponding `Endpoint`.
    pub fn to_endpoint(&self) -> Endpoint {
        match self {
            ConnectedPoint::Dialer { .. } => Endpoint::Dialer,
            ConnectedPoint::Listener { .. } => Endpoint::Listener
        }
    }

    /// Returns true if we are `Dialer`.
    pub fn is_dialer(&self) -> bool {
        match self {
            ConnectedPoint::Dialer { .. } => true,
            ConnectedPoint::Listener { .. } => false
        }
    }

    /// Returns true if we are `Listener`.
    pub fn is_listener(&self) -> bool {
        match self {
            ConnectedPoint::Dialer { .. } => false,
            ConnectedPoint::Listener { .. } => true
        }
    }
}

/// Information about a successfully established connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected<I> {
    /// The connected endpoint, including network address information.
    pub endpoint: ConnectedPoint,
    /// Information obtained from the transport.
    pub info: I,
}

impl<I> Connected<I>
where
    I: ConnectionInfo
{
    pub fn peer_id(&self) -> &I::PeerId {
        self.info.peer_id()
    }
}

/// Information about a connection.
pub trait ConnectionInfo {
    /// Identity of the node we are connected to.
    type PeerId: Eq + Hash;

    /// Returns the identity of the node we are connected to on this connection.
    fn peer_id(&self) -> &Self::PeerId;
}

impl ConnectionInfo for PeerId {
    type PeerId = PeerId;

    fn peer_id(&self) -> &PeerId {
        self
    }
}

/// A multiplexed connection to a peer with an associated `ConnectionHandler`.
pub struct Connection<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: ConnectionHandler<Substream = Substream<TMuxer>>,
{
    /// Node that handles the muxing.
    muxing: substream::Muxing<TMuxer, THandler::OutboundOpenInfo>,
    /// Handler that processes substreams.
    handler: THandler,
}

impl<TMuxer, THandler> fmt::Debug for Connection<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: ConnectionHandler<Substream = Substream<TMuxer>> + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("muxing", &self.muxing)
            .field("handler", &self.handler)
            .finish()
    }
}

impl<TMuxer, THandler> Unpin for Connection<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: ConnectionHandler<Substream = Substream<TMuxer>>,
{
}

impl<TMuxer, THandler> Connection<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: ConnectionHandler<Substream = Substream<TMuxer>>,
{
    /// Builds a new `Connection` from the given substream multiplexer
    /// and connection handler.
    pub fn new(muxer: TMuxer, handler: THandler) -> Self {
        Connection {
            muxing: Muxing::new(muxer),
            handler,
        }
    }

    /// Returns a reference to the `ConnectionHandler`
    pub fn handler(&self) -> &THandler {
        &self.handler
    }

    /// Returns a mutable reference to the `ConnectionHandler`
    pub fn handler_mut(&mut self) -> &mut THandler {
        &mut self.handler
    }

    /// Notifies the connection handler of an event.
    pub fn inject_event(&mut self, event: THandler::InEvent) {
        self.handler.inject_event(event);
    }

    /// Begins an orderly shutdown of the connection, returning a
    /// `Future` that resolves when connection shutdown is complete.
    pub fn close(self) -> Close<TMuxer> {
        self.muxing.close().0
    }

    /// Polls the connection for events produced by the associated handler
    /// as a result of I/O activity on the substream multiplexer.
    pub fn poll(mut self: Pin<&mut Self>, cx: &mut Context)
        -> Poll<Result<THandler::OutEvent, ConnectionError<THandler::Error>>>
    {
        loop {
            let mut io_pending = false;

            // Perform I/O on the connection through the muxer, informing the handler
            // of new substreams.
            match self.muxing.poll(cx) {
                Poll::Pending => io_pending = true,
                Poll::Ready(Ok(SubstreamEvent::InboundSubstream { substream })) => {
                    self.handler.inject_substream(substream, SubstreamEndpoint::Listener)
                }
                Poll::Ready(Ok(SubstreamEvent::OutboundSubstream { user_data, substream })) => {
                    let endpoint = SubstreamEndpoint::Dialer(user_data);
                    self.handler.inject_substream(substream, endpoint)
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(ConnectionError::IO(err))),
            }

            // Poll the handler for new events.
            match self.handler.poll(cx) {
                Poll::Pending => {
                    if io_pending {
                        return Poll::Pending // Nothing to do
                    }
                }
                Poll::Ready(Ok(ConnectionHandlerEvent::OutboundSubstreamRequest(user_data))) => {
                    self.muxing.open_substream(user_data);
                }
                Poll::Ready(Ok(ConnectionHandlerEvent::Custom(event))) => {
                    return Poll::Ready(Ok(event));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(ConnectionError::Handler(err))),
            }
        }
    }
}

/// Borrowed information about an incoming connection currently being negotiated.
#[derive(Debug, Copy, Clone)]
pub struct IncomingInfo<'a> {
    /// Local connection address.
    pub local_addr: &'a Multiaddr,
    /// Stack of protocols used to send back data to the remote.
    pub send_back_addr: &'a Multiaddr,
}

impl<'a> IncomingInfo<'a> {
    /// Builds the `ConnectedPoint` corresponding to the incoming connection.
    pub fn to_connected_point(&self) -> ConnectedPoint {
        ConnectedPoint::Listener {
            local_addr: self.local_addr.clone(),
            send_back_addr: self.send_back_addr.clone(),
        }
    }
}

/// Borrowed information about an outgoing connection currently being negotiated.
#[derive(Debug, Copy, Clone)]
pub struct OutgoingInfo<'a, TPeerId> {
    pub address: &'a Multiaddr,
    pub peer_id: Option<&'a TPeerId>,
}

impl<'a, TPeerId> OutgoingInfo<'a, TPeerId> {
    /// Builds a `ConnectedPoint` corresponding to the outgoing connection.
    pub fn to_connected_point(&self) -> ConnectedPoint {
        ConnectedPoint::Dialer {
            address: self.address.clone()
        }
    }
}

/// Information about a connection limit.
#[derive(Debug, Clone)]
pub struct ConnectionLimit {
    /// The maximum number of connections.
    pub limit: usize,
    /// The current number of connections.
    pub current: usize,
}

impl fmt::Display for ConnectionLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.current, self.limit)
    }
}

/// A `ConnectionLimit` can represent an error if it has been exceeded.
impl Error for ConnectionLimit {}
