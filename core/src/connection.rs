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
pub use substream::{Substream, SubstreamEndpoint};
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

    /// Modifies the address of the remote stored in this struct.
    ///
    /// For `Dialer`, this modifies `address`. For `Listener`, this modifies `send_back_addr`.
    pub fn set_remote_address(&mut self, new_address: Multiaddr) {
        match self {
            ConnectedPoint::Dialer { address } => *address = new_address,
            ConnectedPoint::Listener { send_back_addr, .. } => *send_back_addr = new_address,
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

/// Event generated by a [`Connection`].
#[derive(Debug, Clone)]
pub enum Event<T> {
    /// Event generated by the [`ConnectionHandler`].
    Handler(T),
    /// Address of the remote has changed.
    AddressChange(Multiaddr),
}

/// A multiplexed connection to a peer with an associated `ConnectionHandler`.
pub struct Connection<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: ConnectionHandler<Substream = Substream<TMuxer>>,
{
    /// The substream multiplexer over the connection I/O stream.
    muxing: substream::Muxing<TMuxer, THandler::OutboundOpenInfo>,
    /// The connection handler for the substreams.
    handler: THandler,
    /// The operating state of the connection.
    state: ConnectionState,
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
            state: ConnectionState::Open,
        }
    }

    /// Notifies the connection handler of an event.
    ///
    /// Returns `Ok` if the event was delivered to the handler, `Err`
    /// if the connection is closing and the handler is already closed.
    pub fn inject_event(&mut self, event: THandler::InEvent) -> Result<(), THandler::InEvent> {
        match self.state {
            ConnectionState::Open | ConnectionState::CloseHandler => {
                self.handler.inject_event(event);
                Ok(())
            }
            _ => Err(event)
        }
    }

    /// Begins a graceful shutdown of the connection.
    ///
    /// The connection must continue to be `poll()`ed to drive the
    /// shutdown process to completion. Once connection shutdown is
    /// complete, `poll()` returns `Ok(None)`.
    pub fn start_close(&mut self) {
        if self.state == ConnectionState::Open {
            self.state = ConnectionState::CloseHandler;
        }
    }

    /// Whether the connection is open, i.e. neither closing nor already closed.
    pub fn is_open(&self) -> bool {
        self.state == ConnectionState::Open
    }

    /// Polls the connection for events produced by the associated handler
    /// as a result of I/O activity on the substream multiplexer.
    ///
    /// > **Note**: A return value of `Ok(None)` signals successful
    /// > connection shutdown, whereas an `Err` signals termination
    /// > of the connection due to an error. In either case, the
    /// > connection must be dropped; any further method calls
    /// > result in unspecified behaviour.
    pub fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Result<Option<Event<THandler::OutEvent>>, ConnectionError<THandler::Error>>>
    {
        loop {
            match self.state {
                ConnectionState::Closed => {
                    return Poll::Ready(Ok(None))
                }
                ConnectionState::CloseMuxer => {
                    match futures::ready!(self.muxing.poll_close(cx)) {
                        Ok(()) => {
                            self.state = ConnectionState::Closed;
                            return Poll::Ready(Ok(None))
                        }
                        Err(e) => {
                            return Poll::Ready(Err(ConnectionError::IO(e)))
                        }
                    }
                }
                ConnectionState::Open | ConnectionState::CloseHandler => {
                    // At this point the connection is either open or in the process
                    // of a graceful shutdown by the connection handler.
                    let mut io_pending = false;

                    // Perform I/O on the connection through the muxer, informing the handler
                    // of new substreams or other muxer events.
                    match self.muxing.poll(cx) {
                        Poll::Pending => io_pending = true,
                        Poll::Ready(Ok(SubstreamEvent::InboundSubstream { substream })) => {
                            // Drop new inbound substreams when closing. This is analogous
                            // to rejecting new connections.
                            if self.state == ConnectionState::Open {
                                self.handler.inject_substream(substream, SubstreamEndpoint::Listener)
                            } else {
                                log::trace!("Inbound substream dropped. Connection is closing.")
                            }
                        }
                        Poll::Ready(Ok(SubstreamEvent::OutboundSubstream { user_data, substream })) => {
                            let endpoint = SubstreamEndpoint::Dialer(user_data);
                            self.handler.inject_substream(substream, endpoint)
                        }
                        Poll::Ready(Ok(SubstreamEvent::AddressChange(address))) => {
                            self.handler.inject_address_change(&address);
                            return Poll::Ready(Ok(Some(Event::AddressChange(address))));
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(ConnectionError::IO(err))),
                    }

                    // Poll the handler for new events.
                    let poll =
                        if self.state == ConnectionState::Open {
                            self.handler.poll(cx).map_ok(Some)
                        } else {
                            self.handler.poll_close(cx).map_ok(|event|
                                event.map(ConnectionHandlerEvent::Custom))
                        };

                    match poll {
                        Poll::Pending => {
                            if io_pending {
                                return Poll::Pending // Nothing to do
                            }
                        }
                        Poll::Ready(Ok(Some(ConnectionHandlerEvent::OutboundSubstreamRequest(user_data)))) => {
                            self.muxing.open_substream(user_data);
                        }
                        Poll::Ready(Ok(Some(ConnectionHandlerEvent::Custom(event)))) => {
                            return Poll::Ready(Ok(Some(Event::Handler(event))));
                        }
                        Poll::Ready(Ok(Some(ConnectionHandlerEvent::Close))) => {
                            self.start_close()
                        }
                        Poll::Ready(Ok(None)) => {
                            // The handler is done, we can now close the muxer (i.e. connection).
                            self.state = ConnectionState::CloseMuxer;
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(ConnectionError::Handler(err))),
                    }
                }
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

/// The state of a [`Connection`] w.r.t. an active graceful close.
#[derive(Debug, PartialEq, Eq)]
enum ConnectionState {
    /// The connection is open, accepting new inbound and outbound
    /// substreams.
    Open,
    /// The connection is closing, rejecting new inbound substreams
    /// and not permitting new outbound substreams while the
    /// connection handler closes. [`ConnectionHandler::poll_close`]
    /// is called until completion which results in transitioning to
    /// `CloseMuxer`.
    CloseHandler,
    /// The connection is closing, rejecting new inbound substreams
    /// and not permitting new outbound substreams while the
    /// muxer is closing the transport connection. [`substream::Muxing::poll_close`]
    /// is called until completion, which results in transitioning
    /// to `Closed`.
    CloseMuxer,
    /// The connection is closed.
    Closed
}
