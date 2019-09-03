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
    ConnectedPoint, Multiaddr, PeerId, address_translation,
    nodes::{
        collection::{
            CollectionEvent,
            CollectionNodeAccept,
            CollectionReachEvent,
            CollectionStream,
            ConnectionInfo,
            ReachAttemptId,
            InterruptedReachAttempt
        },
        handled_node::{
            HandledNodeError,
            NodeHandler
        },
        handled_node::IntoNodeHandler,
        node::Substream
    },
    nodes::listeners::{ListenersEvent, ListenerId, ListenersStream},
    transport::{Transport, TransportError}
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

pub use crate::nodes::collection::StartTakeOver;

mod tests;

/// Implementation of `Stream` that handles the nodes.
pub struct Network<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo = PeerId, TPeerId = PeerId>
where
    TTrans: Transport,
{
    /// Listeners for incoming connections.
    listeners: ListenersStream<TTrans>,

    /// The nodes currently active.
    active_nodes: CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TConnInfo>, THandlerErr, (), (TConnInfo, ConnectedPoint), TPeerId>,

    /// The reach attempts of the network.
    /// This needs to be a separate struct in order to handle multiple mutable borrows issues.
    reach_attempts: ReachAttempts<TPeerId>,

    /// Max numer of incoming connections.
    incoming_limit: Option<u32>,

    /// Unfinished take over message to be delivered.
    ///
    /// If the pair's second element is `AsyncSink::NotReady`, the take over
    /// message has yet to be sent using `PeerMut::start_take_over`.
    ///
    /// If the pair's second element is `AsyncSink::Ready`, the take over
    /// message has been sent and needs to be flushed using
    /// `PeerMut::complete_take_over`.
    take_over_to_complete: Option<(TPeerId, AsyncSink<InterruptedReachAttempt<TInEvent, (TConnInfo, ConnectedPoint), ()>>)>
}

impl<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId> fmt::Debug for
    Network<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: fmt::Debug + Transport,
    TConnInfo: fmt::Debug,
    TPeerId: fmt::Debug + Eq + Hash,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ReachAttempts")
            .field("listeners", &self.listeners)
            .field("active_nodes", &self.active_nodes)
            .field("reach_attempts", &self.reach_attempts)
            .field("incoming_limit", &self.incoming_limit)
            .field("take_over_to_complete", &self.take_over_to_complete)
            .finish()
    }
}

impl<TConnInfo> ConnectionInfo for (TConnInfo, ConnectedPoint)
where
    TConnInfo: ConnectionInfo
{
    type PeerId = TConnInfo::PeerId;

    fn peer_id(&self) -> &Self::PeerId {
        self.0.peer_id()
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

/// Event that can happen on the `Network`.
pub enum NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo = PeerId, TPeerId = PeerId>
where
    TTrans: Transport,
{
    /// One of the listeners gracefully closed.
    ListenerClosed {
        /// The listener ID that closed.
        listener_id: ListenerId,
        /// The listener which closed.
        listener: TTrans::Listener,
    },

    /// One of the listeners errored.
    ListenerError {
        /// The listener that errored.
        listener_id: ListenerId,
        /// The listener error.
        error: <TTrans::Listener as Stream>::Error
    },

    /// One of the listeners is now listening on an additional address.
    NewListenerAddress {
        /// The listener that is listening on the new address.
        listener_id: ListenerId,
        /// The new address the listener is now also listening on.
        listen_addr: Multiaddr
    },

    /// One of the listeners is no longer listening on some address.
    ExpiredListenerAddress {
        /// The listener that is no longer listening on some address.
        listener_id: ListenerId,
        /// The expired address.
        listen_addr: Multiaddr
    },

    /// A new connection arrived on a listener.
    IncomingConnection(IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>),

    /// A new connection was arriving on a listener, but an error happened when negotiating it.
    ///
    /// This can include, for example, an error during the handshake of the encryption layer, or
    /// the connection unexpectedly closed.
    IncomingConnectionError {
        /// Local connection address.
        local_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
        /// The error that happened.
        error: IncomingError<TTrans::Error>,
    },

    /// A new connection to a peer has been opened.
    Connected {
        /// Information about the connection, including the peer ID.
        conn_info: TConnInfo,
        /// If `Listener`, then we received the connection. If `Dial`, then it's a connection that
        /// we opened.
        endpoint: ConnectedPoint,
    },

    /// A connection to a peer has been replaced with a new one.
    Replaced {
        /// Information about the new connection. The `TPeerId` is the same as the one as the one
        /// in `old_info`.
        new_info: TConnInfo,
        /// Information about the old connection. The `TPeerId` is the same as the one as the one
        /// in `new_info`.
        old_info: TConnInfo,
        /// Endpoint we were connected to.
        closed_endpoint: ConnectedPoint,
        /// If `Listener`, then we received the connection. If `Dial`, then it's a connection that
        /// we opened.
        endpoint: ConnectedPoint,
    },

    /// The handler of a node has produced an error.
    NodeClosed {
        /// Information about the connection that has been closed.
        conn_info: TConnInfo,
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
        error: NetworkReachError<TTrans::Error, TConnInfo>,
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
        /// Connection that produced the event.
        conn_info: TConnInfo,
        /// Event that was produced by the node.
        event: TOutEvent,
    },
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId> fmt::Debug for
    NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TOutEvent: fmt::Debug,
    TTrans: Transport,
    TTrans::Error: fmt::Debug,
    THandlerErr: fmt::Debug,
    TConnInfo: fmt::Debug,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            NetworkEvent::NewListenerAddress { listener_id, listen_addr } => {
                f.debug_struct("NewListenerAddress")
                    .field("listener_id", listener_id)
                    .field("listen_addr", listen_addr)
                    .finish()
            }
            NetworkEvent::ExpiredListenerAddress { listener_id, listen_addr } => {
                f.debug_struct("ExpiredListenerAddress")
                    .field("listener_id", listener_id)
                    .field("listen_addr", listen_addr)
                    .finish()
            }
            NetworkEvent::ListenerClosed { listener_id, .. } => {
                f.debug_struct("ListenerClosed")
                    .field("listener_id", listener_id)
                    .finish()
            }
            NetworkEvent::ListenerError { listener_id, error } => {
                f.debug_struct("ListenerError")
                    .field("listener_id", listener_id)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::IncomingConnection(event) => {
                f.debug_struct("IncomingConnection")
                    .field("local_addr", &event.local_addr)
                    .field("send_back_addr", &event.send_back_addr)
                    .finish()
            }
            NetworkEvent::IncomingConnectionError { local_addr, send_back_addr, error } => {
                f.debug_struct("IncomingConnectionError")
                    .field("local_addr", local_addr)
                    .field("send_back_addr", send_back_addr)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::Connected { conn_info, endpoint } => {
                f.debug_struct("Connected")
                    .field("conn_info", conn_info)
                    .field("endpoint", endpoint)
                    .finish()
            }
            NetworkEvent::Replaced { new_info, old_info, closed_endpoint, endpoint } => {
                f.debug_struct("Replaced")
                    .field("new_info", new_info)
                    .field("old_info", old_info)
                    .field("closed_endpoint", closed_endpoint)
                    .field("endpoint", endpoint)
                    .finish()
            }
            NetworkEvent::NodeClosed { conn_info, endpoint, error } => {
                f.debug_struct("NodeClosed")
                    .field("conn_info", conn_info)
                    .field("endpoint", endpoint)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::DialError { new_state, peer_id, multiaddr, error } => {
                f.debug_struct("DialError")
                    .field("new_state", new_state)
                    .field("peer_id", peer_id)
                    .field("multiaddr", multiaddr)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::UnknownPeerDialError { multiaddr, error, .. } => {
                f.debug_struct("UnknownPeerDialError")
                    .field("multiaddr", multiaddr)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::NodeEvent { conn_info, event } => {
                f.debug_struct("NodeEvent")
                    .field("conn_info", conn_info)
                    .field("event", event)
                    .finish()
            }
        }
    }
}

/// Internal error type that contains all the possible errors that can happen in a reach attempt.
#[derive(Debug)]
enum InternalReachErr<TTransErr, TConnInfo> {
    /// Error in the transport layer.
    Transport(TransportError<TTransErr>),
    /// We successfully reached the peer, but there was a mismatch between the expected id and the
    /// actual id of the peer.
    PeerIdMismatch {
        /// The information about the bad connection.
        obtained: TConnInfo,
    },
    /// The negotiated `PeerId` is the same as the one of the local node.
    FoundLocalPeerId,
}

impl<TTransErr, TConnInfo> fmt::Display for InternalReachErr<TTransErr, TConnInfo>
where
    TTransErr: fmt::Display,
    TConnInfo: fmt::Debug,
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

impl<TTransErr, TConnInfo> error::Error for InternalReachErr<TTransErr, TConnInfo>
where
    TTransErr: error::Error + 'static,
    TConnInfo: fmt::Debug,
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
pub enum NetworkReachError<TTransErr, TConnInfo> {
    /// Error in the transport layer.
    Transport(TransportError<TTransErr>),

    /// We successfully reached the peer, but there was a mismatch between the expected id and the
    /// actual id of the peer.
    PeerIdMismatch {
        /// The information about the other connection.
        obtained: TConnInfo,
    }
}

impl<TTransErr, TConnInfo> fmt::Display for NetworkReachError<TTransErr, TConnInfo>
where
    TTransErr: fmt::Display,
    TConnInfo: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkReachError::Transport(err) => write!(f, "{}", err),
            NetworkReachError::PeerIdMismatch { obtained } => {
                write!(f, "Peer ID mismatch, obtained: {:?}", obtained)
            },
        }
    }
}

impl<TTransErr, TConnInfo> error::Error for NetworkReachError<TTransErr, TConnInfo>
where
    TTransErr: error::Error + 'static,
    TConnInfo: fmt::Debug,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            NetworkReachError::Transport(err) => Some(err),
            NetworkReachError::PeerIdMismatch { .. } => None,
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
pub struct IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where TTrans: Transport
{
    /// The listener who received the connection.
    listener_id: ListenerId,
    /// The produced upgrade.
    upgrade: TTrans::ListenerUpgrade,
    /// PeerId of the local node.
    local_peer_id: TPeerId,
    /// Local connection address.
    local_addr: Multiaddr,
    /// Address used to send back data to the remote.
    send_back_addr: Multiaddr,
    /// Reference to the `active_nodes` field of the `Network`.
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TConnInfo>, THandlerErr, (), (TConnInfo, ConnectedPoint), TPeerId>,
    /// Reference to the `other_reach_attempts` field of the `Network`.
    other_reach_attempts: &'a mut Vec<(ReachAttemptId, ConnectedPoint)>,
}

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler, THandlerErr, TConnInfo, TPeerId>
    IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport<Output = (TConnInfo, TMuxer)>,
    TTrans::Error: Send + 'static,
    TTrans::ListenerUpgrade: Send + 'static,
    THandler: IntoNodeHandler<(TConnInfo, ConnectedPoint)> + Send + 'static,
    THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
    <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    THandlerErr: error::Error + Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone + Send + 'static,
{
    /// The ID of the listener with the incoming connection.
    pub fn listener_id(&self) -> ListenerId {
        self.listener_id
    }

    /// Starts processing the incoming connection and sets the handler to use for it.
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
            .and_then({
                let connected_point = connected_point.clone();
                move |(peer_id, muxer)| {
                    if *peer_id.peer_id() == local_peer_id {
                        Err(InternalReachErr::FoundLocalPeerId)
                    } else {
                        Ok(((peer_id, connected_point), muxer))
                    }
                }
            });
        let id = self.active_nodes.add_reach_attempt(upgrade, handler);
        self.other_reach_attempts.push((
            id,
            connected_point,
        ));
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
    IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where TTrans: Transport
{
    /// Returns the `IncomingInfo` corresponding to this incoming connection.
    pub fn info(&self) -> IncomingInfo<'_> {
        IncomingInfo {
            local_addr: &self.local_addr,
            send_back_addr: &self.send_back_addr,
        }
    }

    /// Local connection address.
    pub fn local_addr(&self) -> &Multiaddr {
        &self.local_addr
    }

    /// Address used to send back data to the dialer.
    pub fn send_back_addr(&self) -> &Multiaddr {
        &self.send_back_addr
    }

    /// Builds the `ConnectedPoint` corresponding to the incoming connection.
    pub fn to_connected_point(&self) -> ConnectedPoint {
        self.info().to_connected_point()
    }
}

/// Information about an incoming connection currently being negotiated.
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

impl<TTrans, TInEvent, TOutEvent, TMuxer, THandler, THandlerErr, TConnInfo, TPeerId>
    Network<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport + Clone,
    TMuxer: StreamMuxer,
    THandler: IntoNodeHandler<(TConnInfo, ConnectedPoint)> + Send + 'static,
    THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
    <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    THandlerErr: error::Error + Send + 'static,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone,
{
    /// Creates a new node events stream.
    pub fn new(transport: TTrans, local_peer_id: TPeerId) -> Self {
        // TODO: with_capacity?
        Network {
            listeners: ListenersStream::new(transport),
            active_nodes: CollectionStream::new(),
            reach_attempts: ReachAttempts {
                local_peer_id,
                out_reach_attempts: Default::default(),
                other_reach_attempts: Vec::new(),
                connected_points: Default::default(),
            },
            incoming_limit: None,
            take_over_to_complete: None
        }
    }

    /// Creates a new node event stream with incoming connections limit.
    pub fn new_with_incoming_limit(transport: TTrans,
        local_peer_id: TPeerId, incoming_limit: Option<u32>) -> Self
    {
        Network {
            incoming_limit,
            listeners: ListenersStream::new(transport),
            active_nodes: CollectionStream::new(),
            reach_attempts: ReachAttempts {
                local_peer_id,
                out_reach_attempts: Default::default(),
                other_reach_attempts: Vec::new(),
                connected_points: Default::default(),
            },
            take_over_to_complete: None
        }
    }

    /// Returns the transport passed when building this object.
    pub fn transport(&self) -> &TTrans {
        self.listeners.transport()
    }

    /// Start listening on the given multiaddress.
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<TTrans::Error>> {
        self.listeners.listen_on(addr)
    }

    /// Remove a previously added listener.
    pub fn remove_listener(&mut self, id: ListenerId) -> Option<TTrans::Listener> {
        self.listeners.remove_listener(id)
    }

    /// Returns an iterator that produces the list of addresses we are listening on.
    pub fn listen_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.listen_addrs()
    }

    /// Returns limit on incoming connections.
    pub fn incoming_limit(&self) -> Option<u32> {
        self.incoming_limit
    }

    /// Call this function in order to know which address remotes should dial to
    /// access your local node.
    ///
    /// When receiving an observed address on a tcp connection that we initiated, the observed
    /// address contains our tcp dial port, not our tcp listen port. We know which port we are
    /// listening on, thereby we can replace the port within the observed address.
    ///
    /// When receiving an observed address on a tcp connection that we did **not** initiated, the
    /// observed address should contain our listening port. In case it differs from our listening
    /// port there might be a proxy along the path.
    ///
    /// # Arguments
    ///
    /// * `observed_addr` - should be an address a remote observes you as, which can be obtained for
    /// example with the identify protocol.
    ///
    pub fn address_translation<'a>(&'a self, observed_addr: &'a Multiaddr)
        -> impl Iterator<Item = Multiaddr> + 'a
    where
        TMuxer: 'a,
        THandler: 'a,
    {
        self.listen_addrs().flat_map(move |server| address_translation(server, observed_addr))
    }

    /// Returns the peer id of the local node.
    ///
    /// This is the same value as was passed to `new()`.
    pub fn local_peer_id(&self) -> &TPeerId {
        &self.reach_attempts.local_peer_id
    }

    /// Dials a multiaddress without knowing the peer ID we're going to obtain.
    ///
    /// The second parameter is the handler to use if we manage to reach a node.
    pub fn dial(&mut self, addr: Multiaddr, handler: THandler) -> Result<(), TransportError<TTrans::Error>>
    where
        TTrans: Transport<Output = (TConnInfo, TMuxer)>,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        TConnInfo: Send + 'static,
        TPeerId: Send + 'static,
    {
        let local_peer_id = self.reach_attempts.local_peer_id.clone();
        let connected_point = ConnectedPoint::Dialer { address: addr.clone() };
        let future = self.transport().clone().dial(addr)?
            .map_err(|err| InternalReachErr::Transport(TransportError::Other(err)))
            .and_then({
                let connected_point = connected_point.clone();
                move |(peer_id, muxer)| {
                    if *peer_id.peer_id() == local_peer_id {
                        Err(InternalReachErr::FoundLocalPeerId)
                    } else {
                        Ok(((peer_id, connected_point), muxer))
                    }
                }
            });

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
    pub fn num_incoming_negotiated(&self) -> usize {
        self.reach_attempts.other_reach_attempts
            .iter()
            .filter(|&(_, endpoint)| endpoint.is_listener())
            .count()
    }

    /// Returns the list of incoming connections that are currently in the process of being
    /// negotiated. We don't know the `PeerId` of these nodes yet.
    pub fn incoming_negotiated(&self) -> impl Iterator<Item = IncomingInfo<'_>> {
        self.reach_attempts
            .other_reach_attempts
            .iter()
            .filter_map(|&(_, ref endpoint)| {
                match endpoint {
                    ConnectedPoint::Listener { local_addr, send_back_addr } => {
                        Some(IncomingInfo { local_addr, send_back_addr })
                    },
                    ConnectedPoint::Dialer { .. } => None,
                }
            })
    }

    /// Start sending an event to all nodes.
    ///
    /// Make sure to complete the broadcast with `complete_broadcast`.
    #[must_use]
    pub fn start_broadcast(&mut self, event: &TInEvent) -> AsyncSink<()>
    where
        TInEvent: Clone
    {
        self.active_nodes.start_broadcast(event)
    }

    /// Complete a broadcast initiated with `start_broadcast`.
    #[must_use]
    pub fn complete_broadcast(&mut self) -> Async<()> {
        self.active_nodes.complete_broadcast()
    }

    /// Returns a list of all the peers we are currently connected to.
    ///
    /// Calling `peer()` with each `PeerId` is guaranteed to produce a `PeerConnected`.
    // TODO: ideally this would return a list of `PeerConnected` structs, but this is quite
    //       complicated to do in terms of implementation
    pub fn connected_peers(&self) -> impl Iterator<Item = &TPeerId> {
        self.active_nodes.connections()
    }

    /// Returns a list of all the nodes we are currently trying to reach.
    ///
    /// Calling `peer()` with each `PeerId` is guaranteed to produce a `PeerPendingConnect`
    // TODO: ideally this would return a list of `PeerPendingConnect` structs, but this is quite
    //       complicated to do in terms of implementation
    pub fn pending_connection_peers(&self) -> impl Iterator<Item = &TPeerId> {
        self.reach_attempts
            .out_reach_attempts
            .keys()
            .filter(move |p| !self.active_nodes.has_connection(p))
    }

    /// Returns the list of addresses we're currently dialing without knowing the `PeerId` of.
    pub fn unknown_dials(&self) -> impl Iterator<Item = &Multiaddr> {
        self.reach_attempts
            .other_reach_attempts
            .iter()
            .filter_map(|&(_, ref endpoint)| {
                match endpoint {
                    ConnectedPoint::Dialer { address } => Some(address),
                    ConnectedPoint::Listener { .. } => None,
                }
            })
    }

    /// Grants access to a struct that represents a peer.
    pub fn peer(&mut self, peer_id: TPeerId) -> Peer<'_, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId> {
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
        TTrans: Transport<Output = (TConnInfo, TMuxer)>,
        TTrans::Dial: Send + 'static,
        TTrans::Error: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        TPeerId: Send + 'static,
    {
        let reach_id = match self.transport().clone().dial(first.clone()) {
            Ok(fut) => {
                let expected_peer_id = peer_id.clone();
                let connected_point = ConnectedPoint::Dialer { address: first.clone() };
                let fut = fut
                    .map_err(|err| InternalReachErr::Transport(TransportError::Other(err)))
                    .and_then(move |(actual_conn_info, muxer)| {
                        if *actual_conn_info.peer_id() == expected_peer_id {
                            Ok(((actual_conn_info, connected_point), muxer))
                        } else {
                            Err(InternalReachErr::PeerIdMismatch { obtained: actual_conn_info })
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
    pub fn poll(&mut self) -> Async<NetworkEvent<'_, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>>
    where
        TTrans: Transport<Output = (TConnInfo, TMuxer)>,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
        TTrans::ListenerUpgrade: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        THandler: IntoNodeHandler<(TConnInfo, ConnectedPoint)> + Send + 'static,
        THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
        THandlerErr: error::Error + Send + 'static,
        TConnInfo: Clone,
        TPeerId: AsRef<[u8]> + Send + 'static,
    {
        // Start by polling the listeners for events, but only if the number
        // of incoming connections does not exceed the limit.
        match self.incoming_limit {
            Some(x) if self.incoming_negotiated().count() >= (x as usize)
                => (),
            _ => {
                match self.listeners.poll() {
                    Async::NotReady => (),
                    Async::Ready(ListenersEvent::Incoming { listener_id, upgrade, local_addr, send_back_addr }) => {
                        let event = IncomingConnectionEvent {
                            listener_id,
                            upgrade,
                            local_peer_id: self.reach_attempts.local_peer_id.clone(),
                            local_addr,
                            send_back_addr,
                            active_nodes: &mut self.active_nodes,
                            other_reach_attempts: &mut self.reach_attempts.other_reach_attempts,
                        };
                        return Async::Ready(NetworkEvent::IncomingConnection(event));
                    }
                    Async::Ready(ListenersEvent::NewAddress { listener_id, listen_addr }) => {
                        return Async::Ready(NetworkEvent::NewListenerAddress { listener_id, listen_addr })
                    }
                    Async::Ready(ListenersEvent::AddressExpired { listener_id, listen_addr }) => {
                        return Async::Ready(NetworkEvent::ExpiredListenerAddress { listener_id, listen_addr })
                    }
                    Async::Ready(ListenersEvent::Closed { listener_id, listener }) => {
                        return Async::Ready(NetworkEvent::ListenerClosed { listener_id, listener })
                    }
                    Async::Ready(ListenersEvent::Error { listener_id, error }) => {
                        return Async::Ready(NetworkEvent::ListenerError { listener_id, error })
                    }
                }
            }
        }

        // Attempt to deliver any pending take over messages.
        if let Some((id, interrupted)) = self.take_over_to_complete.take() {
            if let Some(mut peer) = self.active_nodes.peer_mut(&id) {
                if let AsyncSink::NotReady(i) = interrupted {
                    if let StartTakeOver::NotReady(i) = peer.start_take_over(i) {
                        self.take_over_to_complete = Some((id, AsyncSink::NotReady(i)))
                    } else if let Ok(Async::NotReady) = peer.complete_take_over() {
                        self.take_over_to_complete = Some((id, AsyncSink::Ready))
                    }
                } else if let Ok(Async::NotReady) = peer.complete_take_over() {
                    self.take_over_to_complete = Some((id, AsyncSink::Ready))
                }
            }
        }
        if self.take_over_to_complete.is_some() {
            return Async::NotReady
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
            Async::Ready(CollectionEvent::NodeClosed {
                conn_info,
                error,
                ..
            }) => {
                let endpoint = self.reach_attempts.connected_points.remove(conn_info.peer_id())
                    .expect("We insert into connected_points whenever a connection is \
                             opened and remove only when a connection is closed; the \
                             underlying API is guaranteed to always deliver a connection \
                             closed message after it has been opened, and no two closed \
                             messages; QED");
                action = Default::default();
                out_event = NetworkEvent::NodeClosed {
                    conn_info: conn_info.0,
                    endpoint,
                    error,
                };
            }
            Async::Ready(CollectionEvent::NodeEvent { peer, event }) => {
                action = Default::default();
                out_event = NetworkEvent::NodeEvent { conn_info: peer.info().0.clone(), event };
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
            let mut peer = self.active_nodes.peer_mut(&peer_id).unwrap();
            if let StartTakeOver::NotReady(i) = peer.start_take_over(interrupted) {
                self.take_over_to_complete = Some((peer_id, AsyncSink::NotReady(i)));
                return Async::NotReady
            }
            if let Ok(Async::NotReady) = peer.complete_take_over() {
                self.take_over_to_complete = Some((peer_id, AsyncSink::Ready));
                return Async::NotReady
            }
        }

        Async::Ready(out_event)
    }
}

/// Internal struct indicating an action to perform on the network.
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
fn handle_node_reached<'a, TTrans, TMuxer, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>(
    reach_attempts: &mut ReachAttempts<TPeerId>,
    event: CollectionReachEvent<'_, TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TConnInfo>, THandlerErr, (), (TConnInfo, ConnectedPoint), TPeerId>,
) -> (ActionItem<THandler, TPeerId>, NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>)
where
    TTrans: Transport<Output = (TConnInfo, TMuxer)> + Clone,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    TConnInfo: ConnectionInfo<PeerId = TPeerId> + Clone + Send + 'static,
    TPeerId: Eq + Hash + AsRef<[u8]> + Clone,
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
                if let ConnectedPoint::Listener { local_addr, send_back_addr } = opened_endpoint {
                    return (Default::default(), NetworkEvent::IncomingConnectionError {
                        local_addr,
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

        let (outcome, conn_info) = event.accept(());
        if let CollectionNodeAccept::ReplacedExisting(old_info, ()) = outcome {
            let closed_endpoint = closed_endpoint
                .expect("We insert into connected_points whenever a connection is opened and \
                         remove only when a connection is closed; the underlying API is \
                         guaranteed to always deliver a connection closed message after it has \
                         been opened, and no two closed messages; QED");
            return (action, NetworkEvent::Replaced {
                new_info: conn_info.0,
                old_info: old_info.0,
                endpoint: opened_endpoint,
                closed_endpoint,
            });
        } else {
            return (action, NetworkEvent::Connected {
                conn_info: conn_info.0,
                endpoint: opened_endpoint
            });
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

        let (outcome, conn_info) = event.accept(());
        if let CollectionNodeAccept::ReplacedExisting(old_info, ()) = outcome {
            let closed_endpoint = closed_endpoint
                .expect("We insert into connected_points whenever a connection is opened and \
                        remove only when a connection is closed; the underlying API is guaranteed \
                        to always deliver a connection closed message after it has been opened, \
                        and no two closed messages; QED");
            return (Default::default(), NetworkEvent::Replaced {
                new_info: conn_info.0,
                old_info: old_info.0,
                endpoint: opened_endpoint,
                closed_endpoint,
            });

        } else {
            return (Default::default(), NetworkEvent::Connected {
                conn_info: conn_info.0,
                endpoint: opened_endpoint
            });
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
fn handle_reach_error<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>(
    reach_attempts: &mut ReachAttempts<TPeerId>,
    reach_id: ReachAttemptId,
    error: InternalReachErr<TTrans::Error, TConnInfo>,
    handler: THandler,
) -> (ActionItem<THandler, TPeerId>, NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>)
where
    TTrans: Transport,
    TConnInfo: ConnectionInfo<PeerId = TPeerId> + Send + 'static,
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
            InternalReachErr::Transport(err) => NetworkReachError::Transport(err),
            InternalReachErr::PeerIdMismatch { obtained } => {
                NetworkReachError::PeerIdMismatch { obtained }
            },
            InternalReachErr::FoundLocalPeerId => {
                unreachable!("We only generate FoundLocalPeerId within dial() or accept(); neither \
                              of these methods add an entry to out_reach_attempts; QED")
            },
        };

        return (action, NetworkEvent::DialError {
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
                return (Default::default(), NetworkEvent::UnknownPeerDialError {
                    multiaddr: address,
                    error,
                    handler,
                });
            }
            ConnectedPoint::Listener { local_addr, send_back_addr } => {
                let error = match error {
                    InternalReachErr::Transport(err) => IncomingError::Transport(err),
                    InternalReachErr::FoundLocalPeerId => IncomingError::FoundLocalPeerId,
                    InternalReachErr::PeerIdMismatch { .. } => {
                        unreachable!("We only generate PeerIdMismatch within start_dial_out(),
                                      which doesn't add any entry in other_reach_attempts; QED")
                    },
                };
                return (Default::default(), NetworkEvent::IncomingConnectionError {
                    local_addr,
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
pub enum Peer<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport,
{
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>),

    /// We are not connected to this peer at all.
    ///
    /// > **Note**: It is however possible that a pending incoming connection is being negotiated
    /// > and will connect to this peer, but we don't know it yet.
    NotConnected(PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>),

    /// The requested peer is the local node.
    LocalNode,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId> fmt::Debug for
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId>,
    TPeerId: fmt::Debug + Eq + Hash,
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
impl<'a, TTrans, TMuxer, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport<Output = (TConnInfo, TMuxer)> + Clone,
    TTrans::Error: Send + 'static,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    THandler: IntoNodeHandler<(TConnInfo, ConnectedPoint)> + Send + 'static,
    THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
    <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    THandlerErr: error::Error + Send + 'static,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone + Send + 'static,
{
    /// If we are connected, returns the `PeerConnected`.
    pub fn into_connected(self) -> Option<PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>> {
        match self {
            Peer::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    pub fn into_pending_connect(self) -> Option<PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>> {
        match self {
            Peer::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }

    /// If we are not connected, returns the `PeerNotConnected`.
    pub fn into_not_connected(self) -> Option<PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>> {
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
    pub fn or_connect(self, addr: Multiaddr, handler: THandler)
        -> Result<PeerPotentialConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>, Self>
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
    pub fn or_connect_with<TFn>(self, addr: TFn, handler: THandler)
        -> Result<PeerPotentialConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>, Self>
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
pub enum PeerPotentialConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport
{
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>),
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
    PeerPotentialConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport,
    TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    TPeerId: Eq + Hash,
{
    /// Closes the connection or the connection attempt.
    // TODO: consider returning a `PeerNotConnected`
    pub fn close(self) {
        match self {
            PeerPotentialConnect::Connected(peer) => peer.close(),
            PeerPotentialConnect::PendingConnect(peer) => peer.interrupt(),
        }
    }

    /// If we are connected, returns the `PeerConnected`.
    pub fn into_connected(self) -> Option<PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>> {
        match self {
            PeerPotentialConnect::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    pub fn into_pending_connect(self) -> Option<PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>> {
        match self {
            PeerPotentialConnect::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }
}

/// Access to a peer we are connected to.
pub struct PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where TTrans: Transport,
{
    /// Reference to the `active_nodes` of the parent.
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TConnInfo>, THandlerErr, (), (TConnInfo, ConnectedPoint), TPeerId>,
    /// Reference to the `connected_points` field of the parent.
    connected_points: &'a mut FnvHashMap<TPeerId, ConnectedPoint>,
    /// Reference to the `out_reach_attempts` field of the parent.
    out_reach_attempts: &'a mut FnvHashMap<TPeerId, OutReachAttempt>,
    peer_id: TPeerId,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId> PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport,
    TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    TPeerId: Eq + Hash,
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

    /// Returns the connection info for this node.
    // TODO: we would love to return a `&'a TConnInfo`, but this isn't possible because of lifetime
    //       issues; see the corresponding method in collection.rs module
    // TODO: should take a `&self`, but the API in collection.rs requires &mut
    pub fn connection_info(&mut self) -> TConnInfo
    where
        TConnInfo: Clone,
    {
        self.active_nodes.peer_mut(&self.peer_id)
            .expect("A PeerConnected is always created with a PeerId in active_nodes; QED")
            .info().0.clone()
    }

    /// Returns the endpoint we're connected to.
    pub fn endpoint(&self) -> &ConnectedPoint {
        self.connected_points.get(&self.peer_id)
            .expect("We insert into connected_points whenever a connection is opened and remove \
                     only when a connection is closed; the underlying API is guaranteed to always \
                     deliver a connection closed message after it has been opened, and no two \
                     closed messages; QED")
    }

    /// Start sending an event to the node.
    pub fn start_send_event(&mut self, event: TInEvent) -> StartSend<TInEvent, ()> {
        self.active_nodes.peer_mut(&self.peer_id)
            .expect("A PeerConnected is always created with a PeerId in active_nodes; QED")
            .start_send_event(event)
    }

    /// Complete sending an event message, initiated by `start_send_event`.
    pub fn complete_send_event(&mut self) -> Poll<(), ()> {
        self.active_nodes.peer_mut(&self.peer_id)
            .expect("A PeerConnected is always created with a PeerId in active_nodes; QED")
            .complete_send_event()
    }
}

/// Access to a peer we are attempting to connect to.
#[derive(Debug)]
pub struct PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport
{
    attempt: OccupiedEntry<'a, TPeerId, OutReachAttempt>,
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler, InternalReachErr<TTrans::Error, TConnInfo>, THandlerErr, (), (TConnInfo, ConnectedPoint), TPeerId>,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
    PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport,
    TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    TPeerId: Eq + Hash,
{
    /// Interrupt this connection attempt.
    // TODO: consider returning a PeerNotConnected; however that is really pain in terms of
    // borrows
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
    pub fn attempted_multiaddr(&self) -> &Multiaddr {
        &self.attempt.get().cur_attempted
    }

    /// Returns a list of the multiaddresses we're going to try if the current dialing fails.
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
pub struct PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport,
{
    peer_id: TPeerId,
    nodes: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId> fmt::Debug for
    PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
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

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler, THandlerErr, TConnInfo, TPeerId>
    PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
where
    TTrans: Transport<Output = (TConnInfo, TMuxer)> + Clone,
    TTrans::Error: Send + 'static,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    THandler: IntoNodeHandler<(TConnInfo, ConnectedPoint)> + Send + 'static,
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
    pub fn connect(self, addr: Multiaddr, handler: THandler)
        -> PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
    where
        TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
        TPeerId: Eq + Hash + Clone + Send + 'static,
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
    pub fn connect_iter<TIter>(self, addrs: TIter, handler: THandler)
        -> Result<PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>, Self>
    where
        TIter: IntoIterator<Item = Multiaddr>,
        TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
        TPeerId: Eq + Hash + Clone + Send + 'static,
    {
        let mut addrs = addrs.into_iter();
        let first = match addrs.next() {
            Some(f) => f,
            None => return Err(self)
        };
        let rest = addrs.collect();
        Ok(self.connect_inner(handler, first, rest))
    }

    /// Moves the given node to a connected state using the given connection info and muxer.
    ///
    /// No `Connected` event is generated for this action.
    ///
    /// # Panic
    ///
    /// Panics if `conn_info.peer_id()` is not the current peer.
    ///
    pub fn inject_connection(self, conn_info: TConnInfo, connected_point: ConnectedPoint, muxer: TMuxer, handler: THandler::Handler)
        -> PeerConnected<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
    where
        TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Clone + Send + 'static,
        TPeerId: Eq + Hash + Clone,
    {
        if conn_info.peer_id() != &self.peer_id {
            panic!("Mismatch between conn_info PeerId and request PeerId");
        }

        match self.nodes.active_nodes.add_connection((conn_info, connected_point), (), muxer, handler) {
            CollectionNodeAccept::NewEntry => {},
            CollectionNodeAccept::ReplacedExisting { .. } =>
                unreachable!("We can only build a PeerNotConnected if we don't have this peer in \
                              the collection yet"),
        }

        PeerConnected {
            active_nodes: &mut self.nodes.active_nodes,
            connected_points: &mut self.nodes.reach_attempts.connected_points,
            out_reach_attempts: &mut self.nodes.reach_attempts.out_reach_attempts,
            peer_id: self.peer_id,
        }
    }

    /// Inner implementation of `connect`.
    fn connect_inner(self, handler: THandler, first: Multiaddr, rest: Vec<Multiaddr>)
        -> PeerPendingConnect<'a, TTrans, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo, TPeerId>
    where
        TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
        TPeerId: Eq + Hash + Clone + Send + 'static,
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
