// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::muxing::StreamMuxer;
use crate::{
    Endpoint, Multiaddr, PeerId,
    nodes::{
        collection::{
            CollectionEvent,
            CollectionNodeAccept,
            CollectionReachEvent,
            CollectionStream,
            ReachAttemptId
        },
        handled_node::{
            HandledNodeError,
            NodeHandler
        },
        handled_node_tasks::IntoNodeHandler,
        node::Substream
    },
    nodes::listeners::{ListenersEvent, ListenersStream},
    transport::Transport,
    transport::TransportError,
};
use fnv::FnvHashMap;
use futures::{prelude::*, future};
use std::{
    collections::hash_map::{Entry, OccupiedEntry},
    error,
    fmt,
    hash::Hash,
    num::NonZeroUsize,
};

mod tests;

/// Implementation of `Stream` that handles the nodes.
pub struct RawSwarm<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId = PeerId>
where
    TTrans: Transport,
{
    /// Listeners for incoming connections.
    listeners: ListenersStream<TTrans>,

    /// The nodes currently active.
    active_nodes: CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TPeerId>, THandlerErr, (), TPeerId>,

    /// The reach attempts of the swarm.
    /// This needs to be a separate struct in order to handle multiple mutable borrows issues.
    reach_attempts: ReachAttempts<TPeerId>,

    /// Max numer of incoming connections.
    incoming_limit: Option<u32>,
}

impl<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId> fmt::Debug for
    RawSwarm<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport + fmt::Debug,
    TPeerId: fmt::Debug + Eq + Hash,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ReachAttempts")
            .field("listeners", &self.listeners)
            .field("active_nodes", &self.active_nodes)
            .field("reach_attempts", &self.reach_attempts)
            .field("incoming_limit", &self.incoming_limit)
            .finish()
    }
}

struct ReachAttempts<TPeerId> {
    /// Peer ID of the node we control.
    local_peer_id: TPeerId,

    /// Attempts to reach a peer.
    /// May contain nodes we are already connected to, because we don't cancel outgoing attempts.
    out_reach_attempts: FnvHashMap<TPeerId, OutReachAttempt>,

    /// Reach attempts for incoming connections, and outgoing connections for which we don't know
    /// the peer ID.
    other_reach_attempts: Vec<(ReachAttemptId, ConnectedPoint)>,

    /// For each peer ID we're connected to, contains the endpoint we're connected to.
    /// Always in sync with `active_nodes`.
    connected_points: FnvHashMap<TPeerId, ConnectedPoint>,
}

impl<TPeerId> fmt::Debug for ReachAttempts<TPeerId>
where
    TPeerId: fmt::Debug + Eq + Hash,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ReachAttempts")
            .field("local_peer_id", &self.local_peer_id)
            .field("out_reach_attempts", &self.out_reach_attempts)
            .field("other_reach_attempts", &self.other_reach_attempts)
            .field("connected_points", &self.connected_points)
            .finish()
    }
}

/// Attempt to reach a peer.
#[derive(Debug, Clone)]
struct OutReachAttempt {
    /// Identifier for the reach attempt.
    id: ReachAttemptId,
    /// Multiaddr currently being attempted.
    cur_attempted: Multiaddr,
    /// Multiaddresses to attempt if the current one fails.
    next_attempts: Vec<Multiaddr>,
}

/// Event that can happen on the `RawSwarm`.
pub enum RawSwarmEvent<'a, TTrans: 'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a, THandlerErr: 'a, TPeerId: 'a = PeerId>
where
    TTrans: Transport,
{
    /// One of the listeners gracefully closed.
    ListenerClosed {
        /// Address of the listener which closed.
        listen_addr: Multiaddr,
        /// The listener which closed.
        listener: TTrans::Listener,
        /// The error that happened. `Ok` if gracefully closed.
        result: Result<(), <TTrans::Listener as Stream>::Error>,
    },

    /// A new connection arrived on a listener.
    IncomingConnection(IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>),

    /// A new connection was arriving on a listener, but an error happened when negotiating it.
    ///
    /// This can include, for example, an error during the handshake of the encryption layer, or
    /// the connection unexpectedly closed.
    IncomingConnectionError {
        /// Address of the listener which received the connection.
        listen_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
        /// The error that happened.
        error: IncomingError<TTrans::Error>,
    },

    /// A new connection to a peer has been opened.
    Connected {
        /// Id of the peer.
        peer_id: TPeerId,
        /// If `Listener`, then we received the connection. If `Dial`, then it's a connection that
        /// we opened.
        endpoint: ConnectedPoint,
    },

    /// A connection to a peer has been replaced with a new one.
    Replaced {
        /// Id of the peer.
        peer_id: TPeerId,
        /// Endpoint we were connected to.
        closed_endpoint: ConnectedPoint,
        /// If `Listener`, then we received the connection. If `Dial`, then it's a connection that
        /// we opened.
        endpoint: ConnectedPoint,
    },

    /// A connection to a node has been closed.
    ///
    /// This happens once both the inbound and outbound channels are closed, and no more outbound
    /// substream attempt is pending.
    NodeClosed {
        /// Identifier of the node.
        peer_id: TPeerId,
        /// Endpoint we were connected to.
        endpoint: ConnectedPoint,
    },

    /// The handler of a node has produced an error.
    NodeError {
        /// Identifier of the node.
        peer_id: TPeerId,
        /// Endpoint we were connected to.
        endpoint: ConnectedPoint,
        /// The error that happened.
        error: HandledNodeError<THandlerErr>,
    },

    /// Failed to reach a peer that we were trying to dial.
    DialError {
        /// New state of a peer.
        new_state: PeerState,

        /// Id of the peer we were trying to dial.
        peer_id: TPeerId,

        /// The multiaddr we failed to reach.
        multiaddr: Multiaddr,

        /// The error that happened.
        error: RawSwarmReachError<TTrans::Error, TPeerId>,
    },

    /// Failed to reach a peer that we were trying to dial.
    UnknownPeerDialError {
        /// The multiaddr we failed to reach.
        multiaddr: Multiaddr,

        /// The error that happened.
        error: UnknownPeerDialErr<TTrans::Error>,

        /// The handler that was passed to `dial()`.
        handler: THandler,
    },

    /// A node produced a custom event.
    NodeEvent {
        /// Id of the node that produced the event.
        peer_id: TPeerId,
        /// Event that was produced by the node.
        event: TOutEvent,
    },
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId> fmt::Debug for
    RawSwarmEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TOutEvent: fmt::Debug,
    TTrans: Transport,
    TTrans::Error: fmt::Debug,
    THandlerErr: fmt::Debug,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            RawSwarmEvent::ListenerClosed { ref listen_addr, ref result, .. } => {
                f.debug_struct("ListenerClosed")
                    .field("listen_addr", listen_addr)
                    .field("result", result)
                    .finish()
            }
            RawSwarmEvent::IncomingConnection( IncomingConnectionEvent { ref listen_addr, ref send_back_addr, .. } ) => {
                f.debug_struct("IncomingConnection")
                    .field("listen_addr", listen_addr)
                    .field("send_back_addr", send_back_addr)
                    .finish()
            }
            RawSwarmEvent::IncomingConnectionError { ref listen_addr, ref send_back_addr, ref error} => {
                f.debug_struct("IncomingConnectionError")
                    .field("listen_addr", listen_addr)
                    .field("send_back_addr", send_back_addr)
                    .field("error", error)
                    .finish()
            }
            RawSwarmEvent::Connected { ref peer_id, ref endpoint } => {
                f.debug_struct("Connected")
                    .field("peer_id", peer_id)
                    .field("endpoint", endpoint)
                    .finish()
            }
            RawSwarmEvent::Replaced { ref peer_id, ref closed_endpoint, ref endpoint } => {
                f.debug_struct("Replaced")
                    .field("peer_id", peer_id)
                    .field("closed_endpoint", closed_endpoint)
                    .field("endpoint", endpoint)
                    .finish()
            }
            RawSwarmEvent::NodeClosed { ref peer_id, ref endpoint } => {
                f.debug_struct("NodeClosed")
                    .field("peer_id", peer_id)
                    .field("endpoint", endpoint)
                    .finish()
            }
            RawSwarmEvent::NodeError { ref peer_id, ref endpoint, ref error } => {
                f.debug_struct("NodeError")
                    .field("peer_id", peer_id)
                    .field("endpoint", endpoint)
                    .field("error", error)
                    .finish()
            }
            RawSwarmEvent::DialError { ref new_state, ref peer_id, ref multiaddr, ref error } => {
                f.debug_struct("DialError")
                    .field("new_state", new_state)
                    .field("peer_id", peer_id)
                    .field("multiaddr", multiaddr)
                    .field("error", error)
                    .finish()
            }
            RawSwarmEvent::UnknownPeerDialError { ref multiaddr, ref error, .. } => {
                f.debug_struct("UnknownPeerDialError")
                    .field("multiaddr", multiaddr)
                    .field("error", error)
                    .finish()
            }
            RawSwarmEvent::NodeEvent { ref peer_id, ref event } => {
                f.debug_struct("NodeEvent")
                    .field("peer_id", peer_id)
                    .field("event", event)
                    .finish()
            }
        }
    }
}

/// Internal error type that contains all the possible errors that can happen in a reach attempt.
#[derive(Debug)]
enum InternalReachErr<TTransErr, TPeerId> {
    /// Error in the transport layer.
    Transport(TransportError<TTransErr>),
    /// We successfully reached the peer, but there was a mismatch between the expected id and the
    /// actual id of the peer.
    PeerIdMismatch {
        /// The peer id that the node reports.
        obtained: TPeerId,
    },
    /// The negotiated `PeerId` is the same as the one of the local node.
    FoundLocalPeerId,
}

impl<TTransErr, TPeerId> fmt::Display for InternalReachErr<TTransErr, TPeerId>
where
    TTransErr: fmt::Display,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InternalReachErr::Transport(err) => write!(f, "{}", err),
            InternalReachErr::PeerIdMismatch { obtained } => {
                write!(f, "Peer ID mismatch, obtained: {:?}", obtained)
            },
            InternalReachErr::FoundLocalPeerId => {
                write!(f, "Remote has the same PeerId as us")
            }
        }
    }
}

impl<TTransErr, TPeerId> error::Error for InternalReachErr<TTransErr, TPeerId>
where
    TTransErr: error::Error + 'static,
    TPeerId: fmt::Debug,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            InternalReachErr::Transport(err) => Some(err),
            InternalReachErr::PeerIdMismatch { .. } => None,
            InternalReachErr::FoundLocalPeerId => None,
        }
    }
}

/// State of a peer.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerState {
    /// We are connected to this peer.
    Connected,
    /// We are currently trying to reach this peer.
    Dialing {
        /// Number of addresses we are trying to dial.
        num_pending_addresses: NonZeroUsize,
    },
    /// We are not connected to this peer.
    NotConnected,
}

/// Error that can happen when trying to reach a node.
#[derive(Debug)]
pub enum RawSwarmReachError<TTransErr, TPeerId> {
    /// Error in the transport layer.
    Transport(TransportError<TTransErr>),

    /// We successfully reached the peer, but there was a mismatch between the expected id and the
    /// actual id of the peer.
    PeerIdMismatch {
        /// The peer id that the node reports.
        obtained: TPeerId,
    }
}

impl<TTransErr, TPeerId> fmt::Display for RawSwarmReachError<TTransErr, TPeerId>
where
    TTransErr: fmt::Display,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RawSwarmReachError::Transport(err) => write!(f, "{}", err),
            RawSwarmReachError::PeerIdMismatch { obtained } => {
                write!(f, "Peer ID mismatch, obtained: {:?}", obtained)
            },
        }
    }
}

impl<TTransErr, TPeerId> error::Error for RawSwarmReachError<TTransErr, TPeerId>
where
    TTransErr: error::Error + 'static,
    TPeerId: fmt::Debug,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            RawSwarmReachError::Transport(err) => Some(err),
            RawSwarmReachError::PeerIdMismatch { .. } => None,
        }
    }
}

/// Error that can happen when dialing a node with an unknown peer ID.
#[derive(Debug)]
pub enum UnknownPeerDialErr<TTransErr> {
    /// Error in the transport layer.
    Transport(TransportError<TTransErr>),
    /// The negotiated `PeerId` is the same as the local node.
    FoundLocalPeerId,
}

impl<TTransErr> fmt::Display for UnknownPeerDialErr<TTransErr>
where TTransErr: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnknownPeerDialErr::Transport(err) => write!(f, "{}", err),
            UnknownPeerDialErr::FoundLocalPeerId => {
                write!(f, "Unknown peer has same PeerId as us")
            },
        }
    }
}

impl<TTransErr> error::Error for UnknownPeerDialErr<TTransErr>
where TTransErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            UnknownPeerDialErr::Transport(err) => Some(err),
            UnknownPeerDialErr::FoundLocalPeerId => None,
        }
    }
}

/// Error that can happen on an incoming connection.
#[derive(Debug)]
pub enum IncomingError<TTransErr> {
    /// Error in the transport layer.
    // TODO: just TTransError should be enough?
    Transport(TransportError<TTransErr>),
    /// Denied the incoming connection because we're already connected to this peer as a dialer
    /// and we have a higher priority than the remote.
    DeniedLowerPriority,
    /// The negotiated `PeerId` is the same as the local node.
    FoundLocalPeerId,
}

impl<TTransErr> fmt::Display for IncomingError<TTransErr>
where TTransErr: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IncomingError::Transport(err) => write!(f, "{}", err),
            IncomingError::DeniedLowerPriority => {
                write!(f, "Denied because of lower priority")
            },
            IncomingError::FoundLocalPeerId => {
                write!(f, "Incoming connection has same PeerId as us")
            },
        }
    }
}

impl<TTransErr> error::Error for IncomingError<TTransErr>
where TTransErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            IncomingError::Transport(err) => Some(err),
            IncomingError::DeniedLowerPriority => None,
            IncomingError::FoundLocalPeerId => None,
        }
    }
}

/// A new connection arrived on a listener.
pub struct IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where TTrans: Transport
{
    /// The produced upgrade.
    upgrade: TTrans::ListenerUpgrade,
    /// PeerId of the local node.
    local_peer_id: TPeerId,
    /// Address of the listener which received the connection.
    listen_addr: Multiaddr,
    /// Address used to send back data to the remote.
    send_back_addr: Multiaddr,
    /// Reference to the `active_nodes` field of the swarm.
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TPeerId>, THandlerErr, (), TPeerId>,
    /// Reference to the `other_reach_attempts` field of the swarm.
    other_reach_attempts: &'a mut Vec<(ReachAttemptId, ConnectedPoint)>,
}

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler, THandlerErr, TPeerId>
    IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport<Output = (TPeerId, TMuxer)>,
    TTrans::Error: Send + 'static,
    TTrans::ListenerUpgrade: Send + 'static,
    THandler: IntoNodeHandler<TPeerId> + Send + 'static,
    THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
    <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    THandlerErr: error::Error + Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    TPeerId: fmt::Debug + Eq + Hash + Clone + Send + 'static,
{
    /// Starts processing the incoming connection and sets the handler to use for it.
    #[inline]
    pub fn accept(self, handler: THandler) {
        self.accept_with_builder(|_| handler)
    }

    /// Same as `accept`, but accepts a closure that turns a `IncomingInfo` into a handler.
    pub fn accept_with_builder<TBuilder>(self, builder: TBuilder)
    where TBuilder: FnOnce(IncomingInfo<'_>) -> THandler
    {
        let connected_point = self.to_connected_point();
        let handler = builder(self.info());
        let local_peer_id = self.local_peer_id;
        let upgrade = self.upgrade
            .map_err(|err| InternalReachErr::Transport(TransportError::Other(err)))
            .and_then(move |(peer_id, muxer)| {
                if peer_id == local_peer_id {
                    Err(InternalReachErr::FoundLocalPeerId)
                } else {
                    Ok((peer_id, muxer))
                }
            });
        let id = self.active_nodes.add_reach_attempt(upgrade, handler);
        self.other_reach_attempts.push((
            id,
            connected_point,
        ));
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
    IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where TTrans: Transport
{
    /// Returns the `IncomingInfo` corresponding to this incoming connection.
    #[inline]
    pub fn info(&self) -> IncomingInfo<'_> {
        IncomingInfo {
            listen_addr: &self.listen_addr,
            send_back_addr: &self.send_back_addr,
        }
    }

    /// Address of the listener that received the connection.
    #[inline]
    pub fn listen_addr(&self) -> &Multiaddr {
        &self.listen_addr
    }

    /// Address used to send back data to the dialer.
    #[inline]
    pub fn send_back_addr(&self) -> &Multiaddr {
        &self.send_back_addr
    }

    /// Builds the `ConnectedPoint` corresponding to the incoming connection.
    #[inline]
    pub fn to_connected_point(&self) -> ConnectedPoint {
        self.info().to_connected_point()
    }
}

/// How we connected to a node.
// TODO: move definition
#[derive(Debug, Clone)]
pub enum ConnectedPoint {
    /// We dialed the node.
    Dialer {
        /// Multiaddress that was successfully dialed.
        address: Multiaddr,
    },
    /// We received the node.
    Listener {
        /// Address of the listener that received the connection.
        listen_addr: Multiaddr,
        /// Stack of protocols used to send back data to the remote.
        send_back_addr: Multiaddr,
    },
}

impl<'a> From<&'a ConnectedPoint> for Endpoint {
    #[inline]
    fn from(endpoint: &'a ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl From<ConnectedPoint> for Endpoint {
    #[inline]
    fn from(endpoint: ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl ConnectedPoint {
    /// Turns the `ConnectedPoint` into the corresponding `Endpoint`.
    #[inline]
    pub fn to_endpoint(&self) -> Endpoint {
        match *self {
            ConnectedPoint::Dialer { .. } => Endpoint::Dialer,
            ConnectedPoint::Listener { .. } => Endpoint::Listener,
        }
    }

    /// Returns true if we are `Dialer`.
    #[inline]
    pub fn is_dialer(&self) -> bool {
        match *self {
            ConnectedPoint::Dialer { .. } => true,
            ConnectedPoint::Listener { .. } => false,
        }
    }

    /// Returns true if we are `Listener`.
    #[inline]
    pub fn is_listener(&self) -> bool {
        match *self {
            ConnectedPoint::Dialer { .. } => false,
            ConnectedPoint::Listener { .. } => true,
        }
    }
}

/// Information about an incoming connection currently being negotiated.
#[derive(Debug, Copy, Clone)]
pub struct IncomingInfo<'a> {
    /// Address of the listener that received the connection.
    pub listen_addr: &'a Multiaddr,
    /// Stack of protocols used to send back data to the remote.
    pub send_back_addr: &'a Multiaddr,
}

impl<'a> IncomingInfo<'a> {
    /// Builds the `ConnectedPoint` corresponding to the incoming connection.
    #[inline]
    pub fn to_connected_point(&self) -> ConnectedPoint {
        ConnectedPoint::Listener {
            listen_addr: self.listen_addr.clone(),
            send_back_addr: self.send_back_addr.clone(),
        }
    }
}

impl<TTrans, TInEvent, TOutEvent, TMuxer, THandler, THandlerErr, TPeerId>
    RawSwarm<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport + Clone,
    TMuxer: StreamMuxer,
    THandler: IntoNodeHandler<TPeerId> + Send + 'static,
    THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
    <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    THandlerErr: error::Error + Send + 'static,
    TPeerId: fmt::Debug + Eq + Hash + Clone + AsRef<[u8]> + Send + 'static,
{
    /// Creates a new node events stream.
    #[inline]
    pub fn new(transport: TTrans, local_peer_id: TPeerId) -> Self {
        // TODO: with_capacity?
        RawSwarm {
            listeners: ListenersStream::new(transport),
            active_nodes: CollectionStream::new(),
            reach_attempts: ReachAttempts {
                local_peer_id,
                out_reach_attempts: Default::default(),
                other_reach_attempts: Vec::new(),
                connected_points: Default::default(),
            },
            incoming_limit: None,
        }
    }

    /// Creates a new node event stream with incoming connections limit.
    #[inline]
    pub fn new_with_incoming_limit(transport: TTrans,
        local_peer_id: TPeerId, incoming_limit: Option<u32>) -> Self
    {
        RawSwarm {
            incoming_limit,
            listeners: ListenersStream::new(transport),
            active_nodes: CollectionStream::new(),
            reach_attempts: ReachAttempts {
                local_peer_id,
                out_reach_attempts: Default::default(),
                other_reach_attempts: Vec::new(),
                connected_points: Default::default(),
            },
        }
    }

    /// Returns the transport passed when building this object.
    #[inline]
    pub fn transport(&self) -> &TTrans {
        self.listeners.transport()
    }

    /// Start listening on the given multiaddress.
    #[inline]
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<Multiaddr, TransportError<TTrans::Error>> {
        self.listeners.listen_on(addr)
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    #[inline]
    pub fn listeners(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.listeners()
    }

    /// Returns limit on incoming connections.
    #[inline]
    pub fn incoming_limit(&self) -> Option<u32> {
        self.incoming_limit
    }

    /// Call this function in order to know which address remotes should dial in order to access
    /// your local node.
    ///
    /// `observed_addr` should be an address a remote observes you as, which can be obtained for
    /// example with the identify protocol.
    ///
    /// For each listener, calls `nat_traversal` with the observed address and returns the outcome.
    #[inline]
    pub fn nat_traversal<'a>(
        &'a self,
        observed_addr: &'a Multiaddr,
    ) -> impl Iterator<Item = Multiaddr> + 'a
        where TMuxer: 'a,
              THandler: 'a,
    {
        self.listeners()
            .flat_map(move |server| self.transport().nat_traversal(server, observed_addr))
    }

    /// Returns the peer id of the local node.
    ///
    /// This is the same value as was passed to `new()`.
    #[inline]
    pub fn local_peer_id(&self) -> &TPeerId {
        &self.reach_attempts.local_peer_id
    }

    /// Dials a multiaddress without knowing the peer ID we're going to obtain.
    ///
    /// The second parameter is the handler to use if we manage to reach a node.
    pub fn dial(&mut self, addr: Multiaddr, handler: THandler) -> Result<(), TransportError<TTrans::Error>>
    where
        TTrans: Transport<Output = (TPeerId, TMuxer)>,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
    {
        let local_peer_id = self.reach_attempts.local_peer_id.clone();
        let future = self.transport().clone().dial(addr.clone())?
            .map_err(|err| InternalReachErr::Transport(TransportError::Other(err)))
            .and_then(move |(peer_id, muxer)| {
                if peer_id == local_peer_id {
                    Err(InternalReachErr::FoundLocalPeerId)
                } else {
                    Ok((peer_id, muxer))
                }
            });

        let connected_point = ConnectedPoint::Dialer { address: addr };
        let reach_id = self.active_nodes.add_reach_attempt(future, handler);
        self.reach_attempts.other_reach_attempts.push((reach_id, connected_point));
        Ok(())
    }

    /// Returns the number of incoming connections that are currently in the process of being
    /// negotiated.
    ///
    /// We don't know anything about these connections yet, so all we can do is know how many of
    /// them we have.
    #[deprecated(note = "Use incoming_negotiated().count() instead")]
    #[inline]
    pub fn num_incoming_negotiated(&self) -> usize {
        self.reach_attempts.other_reach_attempts
            .iter()
            .filter(|&(_, endpoint)| endpoint.is_listener())
            .count()
    }

    /// Returns the list of incoming connections that are currently in the process of being
    /// negotiated. We don't know the `PeerId` of these nodes yet.
    #[inline]
    pub fn incoming_negotiated(&self) -> impl Iterator<Item = IncomingInfo<'_>> {
        self.reach_attempts
            .other_reach_attempts
            .iter()
            .filter_map(|&(_, ref endpoint)| {
                match endpoint {
                    ConnectedPoint::Listener { listen_addr, send_back_addr } => {
                        Some(IncomingInfo {
                            listen_addr,
                            send_back_addr,
                        })
                    },
                    ConnectedPoint::Dialer { .. } => None,
                }
            })
    }

    /// Sends an event to all nodes.
    #[inline]
    pub fn broadcast_event(&mut self, event: &TInEvent)
    where TInEvent: Clone,
    {
        self.active_nodes.broadcast_event(event)
    }

    /// Grants access to a struct that represents a peer.
    #[inline]
    pub fn peer(&mut self, peer_id: TPeerId) -> Peer<'_, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId> {
        if peer_id == self.reach_attempts.local_peer_id {
            return Peer::LocalNode;
        }

        // TODO: we do `peer_mut(...).is_some()` followed with `peer_mut(...).unwrap()`, otherwise
        // the borrow checker yells at us.

        if self.active_nodes.peer_mut(&peer_id).is_some() {
            return Peer::Connected(PeerConnected {
                active_nodes: &mut self.active_nodes,
                peer_id,
                connected_points: &mut self.reach_attempts.connected_points,
                out_reach_attempts: &mut self.reach_attempts.out_reach_attempts,
            });
        }

        // The state of `connected_points` always follows `self.active_nodes`.
        debug_assert!(!self.reach_attempts.connected_points.contains_key(&peer_id));

        if self.reach_attempts.out_reach_attempts.get_mut(&peer_id).is_some() {
            return Peer::PendingConnect(PeerPendingConnect {
                attempt: match self.reach_attempts.out_reach_attempts.entry(peer_id.clone()) {
                    Entry::Occupied(e) => e,
                    Entry::Vacant(_) => panic!("we checked for Some just above"),
                },
                active_nodes: &mut self.active_nodes,
            });
        }

        Peer::NotConnected(PeerNotConnected {
            nodes: self,
            peer_id,
        })
    }

    /// Starts dialing out a multiaddress. `rest` is the list of multiaddresses to attempt if
    /// `first` fails.
    ///
    /// It is a logic error to call this method if we already have an outgoing attempt to the
    /// given peer.
    fn start_dial_out(&mut self, peer_id: TPeerId, handler: THandler, first: Multiaddr, rest: Vec<Multiaddr>)
    where
        TTrans: Transport<Output = (TPeerId, TMuxer)>,
        TTrans::Dial: Send + 'static,
        TTrans::Error: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
    {
        let reach_id = match self.transport().clone().dial(first.clone()) {
            Ok(fut) => {
                let expected_peer_id = peer_id.clone();
                let fut = fut
                    .map_err(|err| InternalReachErr::Transport(TransportError::Other(err)))
                    .and_then(move |(actual_peer_id, muxer)| {
                        if actual_peer_id == expected_peer_id {
                            Ok((actual_peer_id, muxer))
                        } else {
                            Err(InternalReachErr::PeerIdMismatch { obtained: actual_peer_id })
                        }
                    });
                self.active_nodes.add_reach_attempt(fut, handler)
            },
            Err(err) => {
                let fut = future::err(InternalReachErr::Transport(err));
                self.active_nodes.add_reach_attempt(fut, handler)
            },
        };

        let former = self.reach_attempts.out_reach_attempts.insert(
            peer_id,
            OutReachAttempt {
                id: reach_id,
                cur_attempted: first,
                next_attempts: rest,
            },
        );

        debug_assert!(former.is_none());
    }

    /// Provides an API similar to `Stream`, except that it cannot error.
    pub fn poll(&mut self) -> Async<RawSwarmEvent<'_, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>>
    where
        TTrans: Transport<Output = (TPeerId, TMuxer)>,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
        TTrans::ListenerUpgrade: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        THandler: IntoNodeHandler<TPeerId> + Send + 'static,
        THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
        <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
        THandlerErr: error::Error + Send + 'static,
    {
        // Start by polling the listeners for events, but only
        // if numer of incoming connection does not exceed the limit.
        match self.incoming_limit {
            Some(x) if self.incoming_negotiated().count() >= (x as usize)
                => (),
            _ => {
                match self.listeners.poll() {
                    Async::NotReady => (),
                    Async::Ready(ListenersEvent::Incoming {
                        upgrade, listen_addr, send_back_addr }) =>
                    {
                        let event = IncomingConnectionEvent {
                            upgrade,
                            local_peer_id:
                                self.reach_attempts.local_peer_id.clone(),
                            listen_addr,
                            send_back_addr,
                            active_nodes: &mut self.active_nodes,
                            other_reach_attempts: &mut self.reach_attempts.other_reach_attempts,
                        };
                        return Async::Ready(RawSwarmEvent::IncomingConnection(event));
                     },
                    Async::Ready(ListenersEvent::Closed {
                        listen_addr, listener, result }) =>
                    {
                        return Async::Ready(RawSwarmEvent::ListenerClosed {
                            listen_addr,
                            listener,
                            result,
                        });
                    }

                }
            }
        }

        // Poll the existing nodes.
        let (action, out_event);
        match self.active_nodes.poll() {
            Async::NotReady => return Async::NotReady,
            Async::Ready(CollectionEvent::NodeReached(reach_event)) => {
                let (a, e) = handle_node_reached(&mut self.reach_attempts, reach_event);
                action = a;
                out_event = e;
            }
            Async::Ready(CollectionEvent::ReachError { id, error, handler }) => {
                let (a, e) = handle_reach_error(&mut self.reach_attempts, id, error, handler);
                action = a;
                out_event = e;
            }
            Async::Ready(CollectionEvent::NodeError {
                peer_id,
                error,
                ..
            }) => {
                let endpoint = self.reach_attempts.connected_points.remove(&peer_id)
                    .expect("We insert into connected_points whenever a connection is \
                             opened and remove only when a connection is closed; the \
                             underlying API is guaranteed to always deliver a connection \
                             closed message after it has been opened, and no two closed \
                             messages; QED");
                action = Default::default();
                out_event = RawSwarmEvent::NodeError {
                    peer_id,
                    endpoint,
                    error,
                };
            }
            Async::Ready(CollectionEvent::NodeClosed { peer_id, .. }) => {
                let endpoint = self.reach_attempts.connected_points.remove(&peer_id)
                    .expect("We insert into connected_points whenever a connection is \
                             opened and remove only when a connection is closed; the \
                             underlying API is guaranteed to always deliver a connection \
                             closed message after it has been opened, and no two closed \
                             messages; QED");
                action = Default::default();
                out_event = RawSwarmEvent::NodeClosed { peer_id, endpoint };
            }
            Async::Ready(CollectionEvent::NodeEvent { peer, event }) => {
                action = Default::default();
                out_event = RawSwarmEvent::NodeEvent { peer_id: peer.id().clone(), event };
            }
        }

        if let Some((peer_id, handler, first, rest)) = action.start_dial_out {
            self.start_dial_out(peer_id, handler, first, rest);
        }

        if let Some((peer_id, interrupt)) = action.take_over {
            // TODO: improve proof or remove; this is too complicated right now
            let interrupted = self.active_nodes
                .interrupt(interrupt)
                .expect("take_over is guaranteed to be gathered from `out_reach_attempts`;
                         we insert in out_reach_attempts only when we call \
                         active_nodes.add_reach_attempt, and we remove only when we call \
                         interrupt or when a reach attempt succeeds or errors; therefore the \
                         out_reach_attempts should always be in sync with the actual \
                         attempts; QED");
            self.active_nodes.peer_mut(&peer_id).unwrap().take_over(interrupted);
        }

        Async::Ready(out_event)
    }
}

/// Internal struct indicating an action to perform of the swarm.
#[derive(Debug)]
#[must_use]
struct ActionItem<THandler, TPeerId> {
    start_dial_out: Option<(TPeerId, THandler, Multiaddr, Vec<Multiaddr>)>,
    /// The `ReachAttemptId` should be interrupted, and the task for the given `PeerId` should take
    /// over it.
    take_over: Option<(TPeerId, ReachAttemptId)>,
}

impl<THandler, TPeerId> Default for ActionItem<THandler, TPeerId> {
    fn default() -> Self {
        ActionItem {
            start_dial_out: None,
            take_over: None,
        }
    }
}

/// Handles a node reached event from the collection.
///
/// Returns an event to return from the stream.
///
/// > **Note**: The event **must** have been produced by the collection of nodes, otherwise
/// >           panics will likely happen.
fn handle_node_reached<'a, TTrans, TMuxer, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>(
    reach_attempts: &mut ReachAttempts<TPeerId>,
    event: CollectionReachEvent<'_, TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TPeerId>, THandlerErr, (), TPeerId>,
) -> (ActionItem<THandler, TPeerId>, RawSwarmEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>)
where
    TTrans: Transport<Output = (TPeerId, TMuxer)> + Clone,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    TPeerId: fmt::Debug + Eq + Hash + Clone + AsRef<[u8]> + Send + 'static,
{
    // We first start looking in the incoming attempts. While this makes the code less optimal,
    // it also makes the logic easier.
    if let Some(in_pos) = reach_attempts
        .other_reach_attempts
        .iter()
        .position(|i| i.0 == event.reach_attempt_id())
    {
        let (_, opened_endpoint) = reach_attempts.other_reach_attempts.swap_remove(in_pos);
        let has_dial_prio = has_dial_prio(&reach_attempts.local_peer_id, event.peer_id());

        // If we already have an active connection to this peer, a priority system comes into play.
        // If we have a lower peer ID than the incoming one, we drop an incoming connection.
        if event.would_replace() && has_dial_prio {
            if let Some(ConnectedPoint::Dialer { .. }) = reach_attempts.connected_points.get(event.peer_id()) {
                if let ConnectedPoint::Listener { listen_addr, send_back_addr } = opened_endpoint {
                    return (Default::default(), RawSwarmEvent::IncomingConnectionError {
                        listen_addr,
                        send_back_addr,
                        error: IncomingError::DeniedLowerPriority,
                    });
                }
            }
        }

        // Set the endpoint for this peer.
        let closed_endpoint = reach_attempts.connected_points.insert(event.peer_id().clone(), opened_endpoint.clone());

        // If we have dial priority, we keep the current outgoing attempt because it may already
        // have succeeded without us knowing. It is possible that the remote has already closed
        // its ougoing attempt because it sees our outgoing attempt as a success.
        // However we cancel any further multiaddress to attempt in any situation.
        let action = if has_dial_prio {
            if let Some(attempt) = reach_attempts.out_reach_attempts.get_mut(&event.peer_id()) {
                debug_assert_ne!(attempt.id, event.reach_attempt_id());
                attempt.next_attempts.clear();
            }
            ActionItem::default()
        } else {
            if let Some(attempt) = reach_attempts.out_reach_attempts.remove(&event.peer_id()) {
                debug_assert_ne!(attempt.id, event.reach_attempt_id());
                ActionItem {
                    take_over: Some((event.peer_id().clone(), attempt.id)),
                    .. Default::default()
                }
            } else {
                ActionItem::default()
            }
        };

        let (outcome, peer_id) = event.accept(());
        if let CollectionNodeAccept::ReplacedExisting(()) = outcome {
            let closed_endpoint = closed_endpoint
                .expect("We insert into connected_points whenever a connection is opened and \
                         remove only when a connection is closed; the underlying API is \
                         guaranteed to always deliver a connection closed message after it has \
                         been opened, and no two closed messages; QED");
            return (action, RawSwarmEvent::Replaced {
                peer_id,
                endpoint: opened_endpoint,
                closed_endpoint,
            });
        } else {
            return (action, RawSwarmEvent::Connected { peer_id, endpoint: opened_endpoint });
        }
    }

    // Otherwise, try for outgoing attempts.
    let is_outgoing_and_ok = if let Some(attempt) = reach_attempts.out_reach_attempts.get(event.peer_id()) {
        attempt.id == event.reach_attempt_id()
    } else {
        false
    };

    // We only remove the attempt from `out_reach_attempts` if it both matches the reach id
    // and the expected peer id.
    if is_outgoing_and_ok {
        let attempt = reach_attempts.out_reach_attempts.remove(event.peer_id())
            .expect("is_outgoing_and_ok is true only if reach_attempts.out_reach_attempts.get(event.peer_id()) \
                        returned Some");

        let opened_endpoint = ConnectedPoint::Dialer {
            address: attempt.cur_attempted,
        };

        let closed_endpoint = reach_attempts.connected_points
            .insert(event.peer_id().clone(), opened_endpoint.clone());

        let (outcome, peer_id) = event.accept(());
        if let CollectionNodeAccept::ReplacedExisting(()) = outcome {
            let closed_endpoint = closed_endpoint
                .expect("We insert into connected_points whenever a connection is opened and \
                        remove only when a connection is closed; the underlying API is guaranteed \
                        to always deliver a connection closed message after it has been opened, \
                        and no two closed messages; QED");
            return (Default::default(), RawSwarmEvent::Replaced {
                peer_id,
                endpoint: opened_endpoint,
                closed_endpoint,
            });
        } else {
            return (Default::default(), RawSwarmEvent::Connected { peer_id, endpoint: opened_endpoint });
        }
    }

    // We didn't find any entry in neither the outgoing connections not ingoing connections.
    // TODO: improve proof or remove; this is too complicated right now
    panic!("The API of collection guarantees that the id sent back in NodeReached (which is where \
            we call handle_node_reached) is one that was passed to add_reach_attempt. Whenever we \
            call add_reach_attempt, we also insert at the same time an entry either in \
            out_reach_attempts or in other_reach_attempts. It is therefore guaranteed that we \
            find back this ID in either of these two sets");
}

/// Returns true if `local` has dialing priority over `other`.
///
/// This means that if `local` and `other` both dial each other, the connection from `local` should
/// be kept and the one from `other` will be dropped.
#[inline]
fn has_dial_prio<TPeerId>(local: &TPeerId, other: &TPeerId) -> bool
where
    TPeerId: AsRef<[u8]>,
{
    local.as_ref() < other.as_ref()
}

/// Handles a reach error event from the collection.
///
/// Optionally returns an event to return from the stream.
///
/// > **Note**: The event **must** have been produced by the collection of nodes, otherwise
/// >           panics will likely happen.
fn handle_reach_error<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>(
    reach_attempts: &mut ReachAttempts<TPeerId>,
    reach_id: ReachAttemptId,
    error: InternalReachErr<TTrans::Error, TPeerId>,
    handler: THandler,
) -> (ActionItem<THandler, TPeerId>, RawSwarmEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>)
where
    TTrans: Transport,
    TPeerId: Eq + Hash + Clone,
{
    // Search for the attempt in `out_reach_attempts`.
    // TODO: could be more optimal than iterating over everything
    let out_reach_peer_id = reach_attempts
        .out_reach_attempts
        .iter()
        .find(|(_, a)| a.id == reach_id)
        .map(|(p, _)| p.clone());
    if let Some(peer_id) = out_reach_peer_id {
        let attempt = reach_attempts.out_reach_attempts.remove(&peer_id)
            .expect("out_reach_peer_id is a key that is grabbed from out_reach_attempts");

        let num_remain = attempt.next_attempts.len();
        let failed_addr = attempt.cur_attempted.clone();

        let new_state = if reach_attempts.connected_points.contains_key(&peer_id) {
            PeerState::Connected
        } else if num_remain == 0 {
            PeerState::NotConnected
        } else {
            PeerState::Dialing {
                num_pending_addresses: NonZeroUsize::new(num_remain)
                    .expect("We check that num_remain is not 0 right above; QED"),
            }
        };

        let action = if !attempt.next_attempts.is_empty() {
            let mut attempt = attempt;
            let next_attempt = attempt.next_attempts.remove(0);
            ActionItem {
                start_dial_out: Some((peer_id.clone(), handler, next_attempt, attempt.next_attempts)),
                .. Default::default()
            }
        } else {
            Default::default()
        };

        let error = match error {
            InternalReachErr::Transport(err) => RawSwarmReachError::Transport(err),
            InternalReachErr::PeerIdMismatch { obtained } => {
                RawSwarmReachError::PeerIdMismatch { obtained }
            },
            InternalReachErr::FoundLocalPeerId => {
                unreachable!("We only generate FoundLocalPeerId within dial() or accept(); neither \
                              of these methods add an entry to out_reach_attempts; QED")
            },
        };

        return (action, RawSwarmEvent::DialError {
            new_state,
            peer_id,
            multiaddr: failed_addr,
            error,
        });
    }

    // If this is not an outgoing reach attempt, check the incoming reach attempts.
    if let Some(in_pos) = reach_attempts
        .other_reach_attempts
        .iter()
        .position(|i| i.0 == reach_id)
    {
        let (_, endpoint) = reach_attempts.other_reach_attempts.swap_remove(in_pos);
        match endpoint {
            ConnectedPoint::Dialer { address } => {
                let error = match error {
                    InternalReachErr::Transport(err) => UnknownPeerDialErr::Transport(err),
                    InternalReachErr::FoundLocalPeerId => UnknownPeerDialErr::FoundLocalPeerId,
                    InternalReachErr::PeerIdMismatch { .. } => {
                        unreachable!("We only generate PeerIdMismatch within start_dial_out(),
                                      which doesn't add any entry in other_reach_attempts; QED")
                    },
                };
                return (Default::default(), RawSwarmEvent::UnknownPeerDialError {
                    multiaddr: address,
                    error,
                    handler,
                });
            }
            ConnectedPoint::Listener { listen_addr, send_back_addr } => {
                let error = match error {
                    InternalReachErr::Transport(err) => IncomingError::Transport(err),
                    InternalReachErr::FoundLocalPeerId => IncomingError::FoundLocalPeerId,
                    InternalReachErr::PeerIdMismatch { .. } => {
                        unreachable!("We only generate PeerIdMismatch within start_dial_out(),
                                      which doesn't add any entry in other_reach_attempts; QED")
                    },
                };
                return (Default::default(), RawSwarmEvent::IncomingConnectionError {
                    listen_addr,
                    send_back_addr,
                    error
                });
            }
        }
    }

    // The id was neither in the outbound list nor the inbound list.
    // TODO: improve proof or remove; this is too complicated right now
    panic!("The API of collection guarantees that the id sent back in ReachError events \
            (which is where we call handle_reach_error) is one that was passed to \
            add_reach_attempt. Whenever we call add_reach_attempt, we also insert \
            at the same time an entry either in out_reach_attempts or in \
            other_reach_attempts. It is therefore guaranteed that we find back this ID in \
            either of these two sets");
}

/// State of a peer in the system.
pub enum Peer<'a, TTrans: 'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a, THandlerErr: 'a, TPeerId: 'a>
where
    TTrans: Transport,
{
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>),

    /// We are not connected to this peer at all.
    ///
    /// > **Note**: It is however possible that a pending incoming connection is being negotiated
    /// > and will connect to this peer, but we don't know it yet.
    NotConnected(PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>),

    /// The requested peer is the local node.
    LocalNode,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId> fmt::Debug for
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport,
    TPeerId: Eq + Hash + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            Peer::Connected( PeerConnected { ref peer_id, ref connected_points, .. }) => {
                f.debug_struct("Connected")
                    .field("peer_id", peer_id)
                    .field("connected_points", connected_points)
                    .finish()
            }
            Peer::PendingConnect( PeerPendingConnect { ref attempt, .. } ) => {
                f.debug_struct("PendingConnect")
                    .field("attempt", attempt)
                    .finish()
            }
            Peer::NotConnected(PeerNotConnected { ref peer_id, .. }) => {
                f.debug_struct("NotConnected")
                    .field("peer_id", peer_id)
                    .finish()
            }
            Peer::LocalNode => {
                f.debug_struct("LocalNode")
                    .finish()
            }
        }
    }
}

// TODO: add other similar methods that wrap to the ones of `PeerNotConnected`
impl<'a, TTrans, TMuxer, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport<Output = (TPeerId, TMuxer)> + Clone,
    TTrans::Error: Send + 'static,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    THandler: IntoNodeHandler<TPeerId> + Send + 'static,
    THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
    <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    THandlerErr: error::Error + Send + 'static,
    TPeerId: fmt::Debug + Eq + Hash + Clone + AsRef<[u8]> + Send + 'static,
{
    /// If we are connected, returns the `PeerConnected`.
    #[inline]
    pub fn into_connected(self) -> Option<PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>> {
        match self {
            Peer::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    #[inline]
    pub fn into_pending_connect(self) -> Option<PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>> {
        match self {
            Peer::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }

    /// If we are not connected, returns the `PeerNotConnected`.
    #[inline]
    pub fn into_not_connected(self) -> Option<PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>> {
        match self {
            Peer::NotConnected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If we're not connected, opens a new connection to this peer using the given multiaddr.
    ///
    /// If we reach a peer but the `PeerId` doesn't correspond to the one we're expecting, then
    /// the whole connection is immediately closed.
    ///
    /// Returns an error if we are `LocalNode`.
    #[inline]
    pub fn or_connect(self, addr: Multiaddr, handler: THandler)
        -> Result<PeerPotentialConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>, Self>
    {
        self.or_connect_with(move |_| addr, handler)
    }

    /// If we're not connected, calls the function passed as parameter and opens a new connection
    /// using the returned address.
    ///
    /// If we reach a peer but the `PeerId` doesn't correspond to the one we're expecting, then
    /// the whole connection is immediately closed.
    ///
    /// Returns an error if we are `LocalNode`.
    #[inline]
    pub fn or_connect_with<TFn>(self, addr: TFn, handler: THandler)
        -> Result<PeerPotentialConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>, Self>
    where
        TFn: FnOnce(&TPeerId) -> Multiaddr,
    {
        match self {
            Peer::Connected(peer) => Ok(PeerPotentialConnect::Connected(peer)),
            Peer::PendingConnect(peer) => Ok(PeerPotentialConnect::PendingConnect(peer)),
            Peer::NotConnected(peer) => {
                let addr = addr(&peer.peer_id);
                Ok(PeerPotentialConnect::PendingConnect(peer.connect(addr, handler)))
            },
            Peer::LocalNode => Err(Peer::LocalNode),
        }
    }
}

/// Peer we are potentially going to connect to.
pub enum PeerPotentialConnect<'a, TTrans: 'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a, THandlerErr: 'a, TPeerId: 'a>
where
    TTrans: Transport
{
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>),
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
    PeerPotentialConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport,
    TPeerId: Eq + Hash + Clone,
{
    /// Closes the connection or the connection attempt.
    // TODO: consider returning a `PeerNotConnected`
    #[inline]
    pub fn close(self) {
        match self {
            PeerPotentialConnect::Connected(peer) => peer.close(),
            PeerPotentialConnect::PendingConnect(peer) => peer.interrupt(),
        }
    }

    /// If we are connected, returns the `PeerConnected`.
    #[inline]
    pub fn into_connected(self) -> Option<PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>> {
        match self {
            PeerPotentialConnect::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    #[inline]
    pub fn into_pending_connect(self) -> Option<PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>> {
        match self {
            PeerPotentialConnect::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }
}

/// Access to a peer we are connected to.
pub struct PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where TTrans: Transport,
{
    /// Reference to the `active_nodes` of the parent.
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TPeerId>, THandlerErr, (), TPeerId>,
    /// Reference to the `connected_points` field of the parent.
    connected_points: &'a mut FnvHashMap<TPeerId, ConnectedPoint>,
    /// Reference to the `out_reach_attempts` field of the parent.
    out_reach_attempts: &'a mut FnvHashMap<TPeerId, OutReachAttempt>,
    peer_id: TPeerId,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId> PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport,
    TPeerId: Eq + Hash + Clone,
{
    /// Closes the connection to this node.
    ///
    /// No `NodeClosed` message will be generated for this node.
    // TODO: consider returning a `PeerNotConnected`; however this makes all the borrows things
    // much more annoying to deal with
    pub fn close(self) {
        if let Some(reach_attempt) = self.out_reach_attempts.remove(&self.peer_id) {
            self.active_nodes
                .interrupt(reach_attempt.id)
                .expect("Elements in out_reach_attempts are in sync with active_nodes; QED");
        }

        self.connected_points.remove(&self.peer_id);
        self.active_nodes.peer_mut(&self.peer_id)
            .expect("A PeerConnected is always created with a PeerId in active_nodes; QED")
            .close();
    }

    /// Returns the endpoint we're connected to.
    #[inline]
    pub fn endpoint(&self) -> &ConnectedPoint {
        self.connected_points.get(&self.peer_id)
            .expect("We insert into connected_points whenever a connection is opened and remove \
                     only when a connection is closed; the underlying API is guaranteed to always \
                     deliver a connection closed message after it has been opened, and no two \
                     closed messages; QED")
    }

    /// Sends an event to the node.
    #[inline]
    pub fn send_event(&mut self, event: TInEvent) {
        self.active_nodes.peer_mut(&self.peer_id)
            .expect("A PeerConnected is always created with a PeerId in active_nodes; QED")
            .send_event(event)
    }
}

/// Access to a peer we are attempting to connect to.
#[derive(Debug)]
pub struct PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport
{
    attempt: OccupiedEntry<'a, TPeerId, OutReachAttempt>,
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TPeerId>, THandlerErr, (), TPeerId>,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
    PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport,
    TPeerId: Eq + Hash + Clone,
{
    /// Interrupt this connection attempt.
    // TODO: consider returning a PeerNotConnected; however that is really pain in terms of
    // borrows
    #[inline]
    pub fn interrupt(self) {
        let attempt = self.attempt.remove();
        if self.active_nodes.interrupt(attempt.id).is_err() {
            // TODO: improve proof or remove; this is too complicated right now
            panic!("We retreived this attempt.id from out_reach_attempts. We insert in \
                    out_reach_attempts only at the same time as we call add_reach_attempt. \
                    Whenever we receive a NodeReached, NodeReplaced or ReachError event, which \
                    invalidate the attempt.id, we also remove the corresponding entry in \
                    out_reach_attempts.");
        }
    }

    /// Returns the multiaddress we're currently trying to dial.
    #[inline]
    pub fn attempted_multiaddr(&self) -> &Multiaddr {
        &self.attempt.get().cur_attempted
    }

    /// Returns a list of the multiaddresses we're going to try if the current dialing fails.
    #[inline]
    pub fn pending_multiaddrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.attempt.get().next_attempts.iter()
    }

    /// Adds new multiaddrs to attempt if the current dialing fails.
    ///
    /// Doesn't do anything for multiaddresses that are already in the queue.
    pub fn append_multiaddr_attempts(&mut self, addrs: impl IntoIterator<Item = Multiaddr>) {
        for addr in addrs {
            self.append_multiaddr_attempt(addr);
        }
    }

    /// Adds a new multiaddr to attempt if the current dialing fails.
    ///
    /// Doesn't do anything if that multiaddress is already in the queue.
    pub fn append_multiaddr_attempt(&mut self, addr: Multiaddr) {
        if self.attempt.get().next_attempts.iter().any(|a| a == &addr) {
            return;
        }

        self.attempt.get_mut().next_attempts.push(addr);
    }
}

/// Access to a peer we're not connected to.
pub struct PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport,
{
    peer_id: TPeerId,
    nodes: &'a mut RawSwarm<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId> fmt::Debug for
    PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("PeerNotConnected")
            .field("peer_id", &self.peer_id)
            .finish()
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler, THandlerErr, TPeerId>
    PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
where
    TTrans: Transport<Output = (TPeerId, TMuxer)> + Clone,
    TTrans::Error: Send + 'static,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    THandler: IntoNodeHandler<TPeerId> + Send + 'static,
    THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
    <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    THandlerErr: error::Error + Send + 'static,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
{
    /// Attempts a new connection to this node using the given multiaddress.
    ///
    /// If we reach a peer but the `PeerId` doesn't correspond to the one we're expecting, then
    /// the whole connection is immediately closed.
    #[inline]
    pub fn connect(self, addr: Multiaddr, handler: THandler)
        -> PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
    where
        TPeerId: fmt::Debug + Eq + Hash + Clone + AsRef<[u8]> + Send + 'static,
    {
        self.connect_inner(handler, addr, Vec::new())
    }

    /// Attempts a new connection to this node using the given multiaddresses.
    ///
    /// The multiaddresses passed as parameter will be tried one by one.
    ///
    /// Returns an error if the iterator is empty.
    ///
    /// If we reach a peer but the `PeerId` doesn't correspond to the one we're expecting, then
    /// the whole connection is immediately closed.
    #[inline]
    pub fn connect_iter<TIter>(self, addrs: TIter, handler: THandler)
        -> Result<PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>, Self>
    where
        TIter: IntoIterator<Item = Multiaddr>,
        TPeerId: fmt::Debug + Eq + Hash + Clone + AsRef<[u8]> + Send + 'static,
    {
        let mut addrs = addrs.into_iter();
        let first = match addrs.next() {
            Some(f) => f,
            None => return Err(self)
        };
        let rest = addrs.collect();
        Ok(self.connect_inner(handler, first, rest))
    }

    /// Inner implementation of `connect`.
    fn connect_inner(self, handler: THandler, first: Multiaddr, rest: Vec<Multiaddr>)
        -> PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TPeerId>
    where
        TPeerId: fmt::Debug + Eq + Hash + Clone + AsRef<[u8]> + Send + 'static,
    {
        self.nodes.start_dial_out(self.peer_id.clone(), handler, first, rest);
        PeerPendingConnect {
            attempt: match self.nodes.reach_attempts.out_reach_attempts.entry(self.peer_id) {
                Entry::Occupied(e) => e,
                Entry::Vacant(_) => {
                    panic!("We called out_reach_attempts.insert with this peer id just above")
                },
            },
            active_nodes: &mut self.nodes.active_nodes,
        }
    }
}
