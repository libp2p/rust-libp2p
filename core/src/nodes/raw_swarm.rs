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
            PeerMut as CollecPeerMut,
            ReachAttemptId
        },
        handled_node::NodeHandler,
        node::Substream
    },
    nodes::listeners::{ListenersEvent, ListenersStream},
    transport::Transport
};
use fnv::FnvHashMap;
use futures::{prelude::*, future};
use std::{
    fmt,
    collections::hash_map::{Entry, OccupiedEntry},
    io::{Error as IoError, ErrorKind as IoErrorKind}
};

/// Implementation of `Stream` that handles the nodes.
#[derive(Debug)]
pub struct RawSwarm<TTrans, TInEvent, TOutEvent, THandler>
where
    TTrans: Transport,
{
    /// Listeners for incoming connections.
    listeners: ListenersStream<TTrans>,

    /// The nodes currently active.
    active_nodes: CollectionStream<TInEvent, TOutEvent, THandler>,

    /// The reach attempts of the swarm.
    /// This needs to be a separate struct in order to handle multiple mutable borrows issues.
    reach_attempts: ReachAttempts,
}

#[derive(Debug)]
struct ReachAttempts {
    /// Attempts to reach a peer.
    out_reach_attempts: FnvHashMap<PeerId, OutReachAttempt>,

    /// Reach attempts for incoming connections, and outgoing connections for which we don't know
    /// the peer ID.
    other_reach_attempts: Vec<(ReachAttemptId, ConnectedPoint)>,

    /// For each peer ID we're connected to, contains the endpoint we're connected to.
    connected_points: FnvHashMap<PeerId, ConnectedPoint>,
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
pub enum RawSwarmEvent<'a, TTrans: 'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a>
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
    IncomingConnection(IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler>),

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
        error: IoError,
    },

    /// A new connection to a peer has been opened.
    Connected {
        /// Id of the peer.
        peer_id: PeerId,
        /// If `Listener`, then we received the connection. If `Dial`, then it's a connection that
        /// we opened.
        endpoint: ConnectedPoint,
    },

    /// A connection to a peer has been replaced with a new one.
    Replaced {
        /// Id of the peer.
        peer_id: PeerId,
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
        peer_id: PeerId,
        /// Endpoint we were connected to.
        endpoint: ConnectedPoint,
    },

    /// The muxer of a node has produced an error.
    NodeError {
        /// Identifier of the node.
        peer_id: PeerId,
        /// Endpoint we were connected to.
        endpoint: ConnectedPoint,
        /// The error that happened.
        error: IoError,
    },

    /// Failed to reach a peer that we were trying to dial.
    DialError {
        /// Returns the number of multiaddresses that still need to be attempted. If this is
        /// non-zero, then there's still a chance we can connect to this node. If this is zero,
        /// then we have definitely failed.
        remain_addrs_attempt: usize,

        /// Id of the peer we were trying to dial.
        peer_id: PeerId,

        /// The multiaddr we failed to reach.
        multiaddr: Multiaddr,

        /// The error that happened.
        error: IoError,
    },

    /// Failed to reach a peer that we were trying to dial.
    UnknownPeerDialError {
        /// The multiaddr we failed to reach.
        multiaddr: Multiaddr,

        /// The error that happened.
        error: IoError,

        /// The handler that was passed to `dial()`.
        handler: THandler,
    },

    /// A node produced a custom event.
    NodeEvent {
        /// Id of the node that produced the event.
        peer_id: PeerId,
        /// Event that was produced by the node.
        event: TOutEvent,
    },
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler> fmt::Debug for RawSwarmEvent<'a, TTrans, TInEvent, TOutEvent, THandler>
where
    TOutEvent: fmt::Debug,
    TTrans: Transport,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            RawSwarmEvent::ListenerClosed { ref listen_addr, listener: _, ref result } => {
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
            RawSwarmEvent::DialError { ref remain_addrs_attempt, ref peer_id, ref multiaddr, ref error } => {
                f.debug_struct("DialError")
                    .field("remain_addrs_attempt", remain_addrs_attempt)
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


/// A new connection arrived on a listener.
pub struct IncomingConnectionEvent<'a, TTrans: 'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a>
where TTrans: Transport
{
    /// The produced upgrade.
    upgrade: TTrans::ListenerUpgrade,
    /// Address of the listener which received the connection.
    listen_addr: Multiaddr,
    /// Address used to send back data to the remote.
    send_back_addr: Multiaddr,
    /// Reference to the `active_nodes` field of the swarm.
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler>,
    /// Reference to the `other_reach_attempts` field of the swarm.
    other_reach_attempts: &'a mut Vec<(ReachAttemptId, ConnectedPoint)>,
}

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler> IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler>
where
    TTrans: Transport<Output = (PeerId, TMuxer)>,
    TTrans::ListenerUpgrade: Send + 'static,
    THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
    THandler::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
{
    /// Starts processing the incoming connection and sets the handler to use for it.
    #[inline]
    pub fn accept(self, handler: THandler) {
        self.accept_with_builder(|_| handler)
    }

    /// Same as `accept`, but accepts a closure that turns a `ConnectedPoint` into a handler.
    pub fn accept_with_builder<TBuilder>(self, builder: TBuilder)
    where TBuilder: FnOnce(&ConnectedPoint) -> THandler
    {
        let connected_point = self.to_connected_point();
        let handler = builder(&connected_point);
        let id = self.active_nodes.add_reach_attempt(self.upgrade, handler);
        self.other_reach_attempts.push((
            id,
            connected_point,
        ));
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler> IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler>
where TTrans: Transport
{
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
        ConnectedPoint::Listener {
            listen_addr: self.listen_addr.clone(),
            send_back_addr: self.send_back_addr.clone(),
        }
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

impl<TTrans, TInEvent, TOutEvent, TMuxer, THandler>
    RawSwarm<TTrans, TInEvent, TOutEvent, THandler>
where
    TTrans: Transport + Clone,
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
    THandler::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
{
    /// Creates a new node events stream.
    #[inline]
    pub fn new(transport: TTrans) -> Self {
        // TODO: with_capacity?
        RawSwarm {
            listeners: ListenersStream::new(transport),
            active_nodes: CollectionStream::new(),
            reach_attempts: ReachAttempts {
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
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<Multiaddr, Multiaddr> {
        self.listeners.listen_on(addr)
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    #[inline]
    pub fn listeners(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.listeners()
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

    /// Dials a multiaddress without knowing the peer ID we're going to obtain.
    ///
    /// The second parameter is the handler to use if we manage to reach a node.
    pub fn dial(&mut self, addr: Multiaddr, handler: THandler) -> Result<(), Multiaddr>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)>,
        TTrans::Dial: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
    {
        let future = match self.transport().clone().dial(addr.clone()) {
            Ok(fut) => fut,
            Err((_, addr)) => return Err(addr),
        };

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
    // TODO: thats's not true as we should be able to know their multiaddress, but that requires
    // a lot of API changes
    #[inline]
    pub fn num_incoming_negotiated(&self) -> usize {
        self.reach_attempts.other_reach_attempts
            .iter()
            .filter(|&(_, endpoint)| endpoint.is_listener())
            .count()
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
    pub fn peer(&mut self, peer_id: PeerId) -> Peer<TTrans, TInEvent, TOutEvent, THandler> {
        // TODO: we do `peer_mut(...).is_some()` followed with `peer_mut(...).unwrap()`, otherwise
        // the borrow checker yells at us.

        if self.active_nodes.peer_mut(&peer_id).is_some() {
            debug_assert!(!self.reach_attempts.out_reach_attempts.contains_key(&peer_id));
            return Peer::Connected(PeerConnected {
                peer: self
                    .active_nodes
                    .peer_mut(&peer_id)
                    .expect("we checked for Some just above"),
                peer_id,
                connected_points: &mut self.reach_attempts.connected_points,
            });
        }

        if self.reach_attempts.out_reach_attempts.get_mut(&peer_id).is_some() {
            debug_assert!(!self.reach_attempts.connected_points.contains_key(&peer_id));
            return Peer::PendingConnect(PeerPendingConnect {
                attempt: match self.reach_attempts.out_reach_attempts.entry(peer_id.clone()) {
                    Entry::Occupied(e) => e,
                    Entry::Vacant(_) => panic!("we checked for Some just above"),
                },
                active_nodes: &mut self.active_nodes,
            });
        }

        debug_assert!(!self.reach_attempts.connected_points.contains_key(&peer_id));
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
    fn start_dial_out(&mut self, peer_id: PeerId, handler: THandler, first: Multiaddr, rest: Vec<Multiaddr>)
    where
        TTrans: Transport<Output = (PeerId, TMuxer)>,
        TTrans::Dial: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
    {
        let reach_id = match self.transport().clone().dial(first.clone()) {
            Ok(fut) => {
                let expected_peer_id = peer_id.clone();
                let fut = fut.and_then(move |(actual_peer_id, muxer)| {
                    if actual_peer_id == expected_peer_id {
                        Ok((actual_peer_id, muxer))
                    } else {
                        let msg = format!("public key mismatch; expected = {:?}; obtained = {:?}",
                                          expected_peer_id, actual_peer_id);
                        Err(IoError::new(IoErrorKind::Other, msg))
                    }
                });
                self.active_nodes.add_reach_attempt(fut, handler)
            },
            Err((_, addr)) => {
                let msg = format!("unsupported multiaddr {}", addr);
                let fut = future::err(IoError::new(IoErrorKind::Other, msg));
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
    pub fn poll(&mut self) -> Async<RawSwarmEvent<TTrans, TInEvent, TOutEvent, THandler>>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)>,
        TTrans::Dial: Send + 'static,
        TTrans::ListenerUpgrade: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
        THandler::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    {
        // Start by polling the listeners for events.
        match self.listeners.poll() {
            Async::NotReady => (),
            Async::Ready(ListenersEvent::Incoming { upgrade, listen_addr, send_back_addr }) => {
                let event = IncomingConnectionEvent {
                    upgrade,
                    listen_addr,
                    send_back_addr,
                    active_nodes: &mut self.active_nodes,
                    other_reach_attempts: &mut self.reach_attempts.other_reach_attempts,
                };
                return Async::Ready(RawSwarmEvent::IncomingConnection(event));
            }
            Async::Ready(ListenersEvent::Closed { listen_addr, listener, result }) => {
                return Async::Ready(RawSwarmEvent::ListenerClosed {
                    listen_addr,
                    listener,
                    result,
                });
            }
        }

        // Poll the existing nodes.
        loop {
            let (action, out_event);
            match self.active_nodes.poll() {
                Async::NotReady => break,
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
                }) => {
                    let endpoint = self.reach_attempts.connected_points.remove(&peer_id)
                        .expect("We insert into connected_points whenever a connection is \
                                 opened and remove only when a connection is closed; the \
                                 underlying API is guaranteed to always deliver a connection \
                                 closed message after it has been opened, and no two closed \
                                 messages; qed");
                    debug_assert!(!self.reach_attempts.out_reach_attempts.contains_key(&peer_id));
                    action = Default::default();
                    out_event = RawSwarmEvent::NodeError {
                        peer_id,
                        endpoint,
                        error,
                    };
                }
                Async::Ready(CollectionEvent::NodeClosed { peer_id }) => {
                    let endpoint = self.reach_attempts.connected_points.remove(&peer_id)
                        .expect("We insert into connected_points whenever a connection is \
                                 opened and remove only when a connection is closed; the \
                                 underlying API is guaranteed to always deliver a connection \
                                 closed message after it has been opened, and no two closed \
                                 messages; qed");
                    debug_assert!(!self.reach_attempts.out_reach_attempts.contains_key(&peer_id));
                    action = Default::default();
                    out_event = RawSwarmEvent::NodeClosed { peer_id, endpoint };
                }
                Async::Ready(CollectionEvent::NodeEvent { peer_id, event }) => {
                    action = Default::default();
                    out_event = RawSwarmEvent::NodeEvent { peer_id, event };
                }
            };

            if let Some((peer_id, handler, first, rest)) = action.start_dial_out {
                self.start_dial_out(peer_id, handler, first, rest);
            }

            if let Some(interrupt) = action.interrupt {
                // TODO: improve proof or remove; this is too complicated right now
                self.active_nodes
                    .interrupt(interrupt)
                    .expect("interrupt is guaranteed to be gathered from `out_reach_attempts`;
                             we insert in out_reach_attempts only when we call \
                             active_nodes.add_reach_attempt, and we remove only when we call \
                             interrupt or when a reach attempt succeeds or errors; therefore the \
                             out_reach_attempts should always be in sync with the actual \
                             attempts; qed");
            }

            return Async::Ready(out_event);
        }

        Async::NotReady
    }
}

/// Internal struct indicating an action to perform of the swarm.
#[derive(Debug)]
#[must_use]
struct ActionItem<THandler> {
    start_dial_out: Option<(PeerId, THandler, Multiaddr, Vec<Multiaddr>)>,
    interrupt: Option<ReachAttemptId>,
}

impl<THandler> Default for ActionItem<THandler> {
    fn default() -> Self {
        ActionItem {
            start_dial_out: None,
            interrupt: None,
        }
    }
}

/// Handles a node reached event from the collection.
///
/// Returns an event to return from the stream.
///
/// > **Note**: The event **must** have been produced by the collection of nodes, otherwise
/// >           panics will likely happen.
fn handle_node_reached<'a, TTrans, TMuxer, TInEvent, TOutEvent, THandler>(
    reach_attempts: &mut ReachAttempts,
    event: CollectionReachEvent<TInEvent, TOutEvent, THandler>
) -> (ActionItem<THandler>, RawSwarmEvent<'a, TTrans, TInEvent, TOutEvent, THandler>)
where
    TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
{
    // We first start looking in the incoming attempts. While this makes the code less optimal,
    // it also makes the logic easier.
    if let Some(in_pos) = reach_attempts
        .other_reach_attempts
        .iter()
        .position(|i| i.0 == event.reach_attempt_id())
    {
        let (_, opened_endpoint) = reach_attempts.other_reach_attempts.swap_remove(in_pos);

        // Set the endpoint for this peer.
        let closed_endpoint = reach_attempts.connected_points.insert(event.peer_id().clone(), opened_endpoint.clone());

        // Cancel any outgoing attempt to this peer.
        let action = if let Some(attempt) = reach_attempts.out_reach_attempts.remove(&event.peer_id()) {
            debug_assert_ne!(attempt.id, event.reach_attempt_id());
            ActionItem {
                interrupt: Some(attempt.id),
                .. Default::default()
            }
        } else {
            ActionItem::default()
        };

        let (outcome, peer_id) = event.accept();
        if outcome == CollectionNodeAccept::ReplacedExisting {
            let closed_endpoint = closed_endpoint
                .expect("We insert into connected_points whenever a connection is opened and \
                         remove only when a connection is closed; the underlying API is \
                         guaranteed to always deliver a connection closed message after it has \
                         been opened, and no two closed messages; qed");
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

        let (outcome, peer_id) = event.accept();
        if outcome == CollectionNodeAccept::ReplacedExisting {
            let closed_endpoint = closed_endpoint
                .expect("We insert into connected_points whenever a connection is opened and \
                        remove only when a connection is closed; the underlying API is guaranteed \
                        to always deliver a connection closed message after it has been opened, \
                        and no two closed messages; qed");
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

/// Handles a reach error event from the collection.
///
/// Optionally returns an event to return from the stream.
///
/// > **Note**: The event **must** have been produced by the collection of nodes, otherwise
/// >           panics will likely happen.
fn handle_reach_error<'a, TTrans, TInEvent, TOutEvent, THandler>(
    reach_attempts: &mut ReachAttempts,
    reach_id: ReachAttemptId,
    error: IoError,
    handler: THandler,
) -> (ActionItem<THandler>, RawSwarmEvent<'a, TTrans, TInEvent, TOutEvent, THandler>)
where TTrans: Transport
{
    // Search for the attempt in `out_reach_attempts`.
    // TODO: could be more optimal than iterating over everything
    let out_reach_peer_id = reach_attempts
        .out_reach_attempts
        .iter()
        .find(|(_, a)| a.id == reach_id)
        .map(|(p, _)| p.clone());
    if let Some(peer_id) = out_reach_peer_id {
        let mut attempt = reach_attempts.out_reach_attempts.remove(&peer_id)
            .expect("out_reach_peer_id is a key that is grabbed from out_reach_attempts");

        let num_remain = attempt.next_attempts.len();
        let failed_addr = attempt.cur_attempted.clone();

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

        return (action, RawSwarmEvent::DialError {
            remain_addrs_attempt: num_remain,
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
                return (Default::default(), RawSwarmEvent::UnknownPeerDialError {
                    multiaddr: address,
                    error,
                    handler,
                });
            }
            ConnectedPoint::Listener { listen_addr, send_back_addr } => {
                return (Default::default(), RawSwarmEvent::IncomingConnectionError { listen_addr, send_back_addr, error });
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
pub enum Peer<'a, TTrans: 'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a>
where
    TTrans: Transport,
{
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TInEvent>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TInEvent, TOutEvent, THandler>),

    /// We are not connected to this peer at all.
    ///
    /// > **Note**: It is however possible that a pending incoming connection is being negotiated
    /// > and will connect to this peer, but we don't know it yet.
    NotConnected(PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler>),
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler> fmt::Debug for Peer<'a, TTrans, TInEvent, TOutEvent, THandler>
where
    TTrans: Transport,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            Peer::Connected( PeerConnected { peer: _, ref peer_id, ref connected_points }) => {
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
        }
    }
}

// TODO: add other similar methods that wrap to the ones of `PeerNotConnected`
impl<'a, TTrans, TMuxer, TInEvent, TOutEvent, THandler>
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler>
where
    TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
    THandler::OutboundOpenInfo: Send + 'static,
{
    /// If we are connected, returns the `PeerConnected`.
    #[inline]
    pub fn as_connected(self) -> Option<PeerConnected<'a, TInEvent>> {
        match self {
            Peer::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    #[inline]
    pub fn as_pending_connect(self) -> Option<PeerPendingConnect<'a, TInEvent, TOutEvent, THandler>> {
        match self {
            Peer::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }

    /// If we are not connected, returns the `PeerNotConnected`.
    #[inline]
    pub fn as_not_connected(self) -> Option<PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler>> {
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
    /// > **Note**: It is possible that the attempt reaches a node that doesn't have the peer id
    /// >           that we are expecting, in which case the handler will be used for this "wrong"
    /// >           node.
    #[inline]
    pub fn or_connect(self, addr: Multiaddr, handler: THandler)
        -> Result<PeerPotentialConnect<'a, TInEvent, TOutEvent, THandler>, Self>
    {
        self.or_connect_with(move |_| addr, handler)
    }

    /// If we're not connected, calls the function passed as parameter and opens a new connection
    /// using the returned address.
    ///
    /// If we reach a peer but the `PeerId` doesn't correspond to the one we're expecting, then
    /// the whole connection is immediately closed.
    ///
    /// > **Note**: It is possible that the attempt reaches a node that doesn't have the peer id
    /// >           that we are expecting, in which case the handler will be used for this "wrong"
    /// >           node.
    #[inline]
    pub fn or_connect_with<TFn>(self, addr: TFn, handler: THandler)
        -> Result<PeerPotentialConnect<'a, TInEvent, TOutEvent, THandler>, Self>
    where
        TFn: FnOnce(&PeerId) -> Multiaddr,
    {
        match self {
            Peer::Connected(peer) => Ok(PeerPotentialConnect::Connected(peer)),
            Peer::PendingConnect(peer) => Ok(PeerPotentialConnect::PendingConnect(peer)),
            Peer::NotConnected(peer) => {
                let addr = addr(&peer.peer_id);
                match peer.connect(addr, handler) {
                    Ok(peer) => Ok(PeerPotentialConnect::PendingConnect(peer)),
                    Err(peer) => Err(Peer::NotConnected(peer)),
                }
            }
        }
    }
}

/// Peer we are potentially going to connect to.
pub enum PeerPotentialConnect<'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a> {
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TInEvent>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TInEvent, TOutEvent, THandler>),
}

impl<'a, TInEvent, TOutEvent, THandler> PeerPotentialConnect<'a, TInEvent, TOutEvent, THandler> {
    /// Closes the connection or the connection attempt.
    ///
    /// If the connection was active, returns the list of outbound substream openings that were
    /// closed in the process.
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
    pub fn as_connected(self) -> Option<PeerConnected<'a, TInEvent>> {
        match self {
            PeerPotentialConnect::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    #[inline]
    pub fn as_pending_connect(self) -> Option<PeerPendingConnect<'a, TInEvent, TOutEvent, THandler>> {
        match self {
            PeerPotentialConnect::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }
}

/// Access to a peer we are connected to.
pub struct PeerConnected<'a, TInEvent: 'a> {
    peer: CollecPeerMut<'a, TInEvent>,
    /// Reference to the `connected_points` field of the parent.
    connected_points: &'a mut FnvHashMap<PeerId, ConnectedPoint>,
    peer_id: PeerId,
}

impl<'a, TInEvent> PeerConnected<'a, TInEvent> {
    /// Closes the connection to this node.
    ///
    /// No `NodeClosed` message will be generated for this node.
    // TODO: consider returning a `PeerNotConnected`; however this makes all the borrows things
    // much more annoying to deal with
    pub fn close(self) {
        self.connected_points.remove(&self.peer_id);
        self.peer.close()
    }

    /// Returns the endpoint we're connected to.
    #[inline]
    pub fn endpoint(&self) -> &ConnectedPoint {
        self.connected_points.get(&self.peer_id)
            .expect("We insert into connected_points whenever a connection is opened and remove \
                     only when a connection is closed; the underlying API is guaranteed to always \
                     deliver a connection closed message after it has been opened, and no two \
                     closed messages; qed")
    }

    /// Sends an event to the node.
    #[inline]
    pub fn send_event(&mut self, event: TInEvent) {
        self.peer.send_event(event)
    }
}

/// Access to a peer we are attempting to connect to.
#[derive(Debug)]
pub struct PeerPendingConnect<'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a> {
    attempt: OccupiedEntry<'a, PeerId, OutReachAttempt>,
    active_nodes: &'a mut CollectionStream<TInEvent, TOutEvent, THandler>,
}

impl<'a, TInEvent, TOutEvent, THandler> PeerPendingConnect<'a, TInEvent, TOutEvent, THandler> {
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
#[derive(Debug)]
pub struct PeerNotConnected<'a, TTrans: 'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a>
where
    TTrans: Transport,
{
    peer_id: PeerId,
    nodes: &'a mut RawSwarm<TTrans, TInEvent, TOutEvent, THandler>,
}

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler>
    PeerNotConnected<'a, TTrans, TInEvent, TOutEvent, THandler>
where
    TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
    THandler::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
{
    /// Attempts a new connection to this node using the given multiaddress.
    ///
    /// If we reach a peer but the `PeerId` doesn't correspond to the one we're expecting, then
    /// the whole connection is immediately closed.
    #[inline]
    pub fn connect(self, addr: Multiaddr, handler: THandler) -> Result<PeerPendingConnect<'a, TInEvent, TOutEvent, THandler>, Self> {
        self.connect_inner(handler, addr, Vec::new())
    }

    /// Attempts a new connection to this node using the given multiaddresses.
    ///
    /// The multiaddresses passed as parameter will be tried one by one.
    ///
    /// If the iterator is empty, TODO: what to do? at the moment we unwrap
    ///
    /// If we reach a peer but the `PeerId` doesn't correspond to the one we're expecting, then
    /// the whole connection is immediately closed.
    #[inline]
    pub fn connect_iter<TIter>(self, addrs: TIter, handler: THandler)
        -> Result<PeerPendingConnect<'a, TInEvent, TOutEvent, THandler>, Self>
    where
        TIter: IntoIterator<Item = Multiaddr>,
    {
        let mut addrs = addrs.into_iter();
        let first = match addrs.next() {
            Some(f) => f,
            None => return Err(self)
        };
        let rest = addrs.collect();
        self.connect_inner(handler, first, rest)
    }

    /// Inner implementation of `connect`.
    fn connect_inner(self, handler: THandler, first: Multiaddr, rest: Vec<Multiaddr>)
        -> Result<PeerPendingConnect<'a, TInEvent, TOutEvent, THandler>, Self>
    {
        self.nodes.start_dial_out(self.peer_id.clone(), handler, first, rest);
        Ok(PeerPendingConnect {
            attempt: match self.nodes.reach_attempts.out_reach_attempts.entry(self.peer_id) {
                Entry::Occupied(e) => e,
                Entry::Vacant(_) => {
                    panic!("We called out_reach_attempts.insert with this peer id just above")
                },
            },
            active_nodes: &mut self.nodes.active_nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use parking_lot::Mutex;
    use tokio::runtime::{Builder, Runtime};
    use tests::dummy_transport::DummyTransport;
    use tests::dummy_handler::{Handler, HandlerState, InEvent, OutEvent};
    use tests::dummy_transport::ListenerState;
    use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
    use nodes::NodeHandlerEvent;

    #[test]
    fn query_transport() {
        let transport = DummyTransport::new();
        let transport2 = transport.clone();
        let raw_swarm = RawSwarm::<_, _, _, Handler>::new(transport);
        assert_eq!(raw_swarm.transport(), &transport2);
    }

    #[test]
    fn starts_listening() {
        let mut raw_swarm = RawSwarm::<_, _, _, Handler>::new(DummyTransport::new());
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let addr2 = addr.clone();
        assert!(raw_swarm.listen_on(addr).is_ok());
        let listeners = raw_swarm.listeners().collect::<Vec<&Multiaddr>>();
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0], &addr2);
    }

    #[test]
    fn nat_traversal_transforms_the_observed_address_according_to_the_transport_used() {
        // the DummyTransport nat_traversal increments the port number by one for Ip4 addresses
        let transport = DummyTransport::new();
        let mut raw_swarm = RawSwarm::<_, _, _, Handler>::new(transport);
        let addr1 = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        // An unrelated outside address is returned as-is, no transform
        let outside_addr1 = "/memory".parse::<Multiaddr>().expect("bad multiaddr");

        let addr2 = "/ip4/127.0.0.2/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let outside_addr2 = "/ip4/127.0.0.2/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");

        raw_swarm.listen_on(addr1).unwrap();
        raw_swarm.listen_on(addr2).unwrap();

        let natted = raw_swarm
            .nat_traversal(&outside_addr1)
            .map(|a| a.to_string())
            .collect::<Vec<_>>();

        assert!(natted.is_empty());

        let natted = raw_swarm
            .nat_traversal(&outside_addr2)
            .map(|a| a.to_string())
            .collect::<Vec<_>>();

        assert_eq!(natted, vec!["/ip4/127.0.0.2/tcp/1234"])
    }

    #[test]
    fn successful_dial_reaches_a_node() {
        let mut swarm = RawSwarm::<_, _, _, Handler>::new(DummyTransport::new());
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let dial_res = swarm.dial(addr, Handler::default());
        assert!(dial_res.is_ok());

        // Poll the swarm until we get a `NodeReached` then assert on the peer:
        // it's there and it's connected.
        let swarm = Arc::new(Mutex::new(swarm));

        let mut rt = Runtime::new().unwrap();
        let mut peer_id : Option<PeerId> = None;
        // Drive forward until we're Connected
        while peer_id.is_none() {
            let swarm_fut = swarm.clone();
            peer_id = rt.block_on(future::poll_fn(move || -> Poll<Option<PeerId>, ()> {
                let mut swarm = swarm_fut.lock();
                let poll_res = swarm.poll();
                match poll_res {
                    Async::Ready(RawSwarmEvent::Connected { peer_id, .. }) => Ok(Async::Ready(Some(peer_id))),
                    _ => Ok(Async::Ready(None))
                }
            })).expect("tokio works");
        }

        let mut swarm = swarm.lock();
        let peer = swarm.peer(peer_id.unwrap());
        assert_matches!(peer, Peer::Connected(PeerConnected{..}));
    }

    #[test]
    fn num_incoming_negotiated() {
        let mut transport = DummyTransport::new();
        let peer_id = PeerId::random();
        let muxer = DummyMuxer::new();

        // Set up listener to see an incoming connection
        transport.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some((peer_id, muxer)))));

        let mut swarm = RawSwarm::<_, _, _, Handler>::new(transport);
        swarm.listen_on("/memory".parse().unwrap()).unwrap();

        // no incoming yet
        assert_eq!(swarm.num_incoming_negotiated(), 0);

        let mut rt = Runtime::new().unwrap();
        let swarm = Arc::new(Mutex::new(swarm));
        let swarm_fut = swarm.clone();
        let fut = future::poll_fn(move || -> Poll<_, ()> {
            let mut swarm_fut = swarm_fut.lock();
            assert_matches!(swarm_fut.poll(), Async::Ready(RawSwarmEvent::IncomingConnection(incoming)) => {
                incoming.accept(Handler::default());
            });

            Ok(Async::Ready(()))
        });
        rt.block_on(fut).expect("tokio works");
        let swarm = swarm.lock();
        // Now there's an incoming connection
        assert_eq!(swarm.num_incoming_negotiated(), 1);
    }

    #[test]
    fn broadcasted_events_reach_active_nodes() {
        let mut swarm = RawSwarm::<_, _, _, Handler>::new(DummyTransport::new());
        let mut muxer = DummyMuxer::new();
        muxer.set_inbound_connection_state(DummyConnectionState::Pending);
        muxer.set_outbound_connection_state(DummyConnectionState::Opened);
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let mut handler = Handler::default();
        handler.next_states = vec![HandlerState::Ready(Some(NodeHandlerEvent::Custom(OutEvent::Custom("from handler 1") ))),];
        let dial_result = swarm.dial(addr, handler);
        assert!(dial_result.is_ok());

        swarm.broadcast_event(&InEvent::NextState);
        let swarm = Arc::new(Mutex::new(swarm));
        let mut rt = Runtime::new().unwrap();
        let mut peer_id : Option<PeerId> = None;
        while peer_id.is_none() {
            let swarm_fut = swarm.clone();
            peer_id = rt.block_on(future::poll_fn(move || -> Poll<Option<PeerId>, ()> {
                let mut swarm = swarm_fut.lock();
                let poll_res = swarm.poll();
                match poll_res {
                    Async::Ready(RawSwarmEvent::Connected { peer_id, .. }) => Ok(Async::Ready(Some(peer_id))),
                    _ => Ok(Async::Ready(None))
                }
            })).expect("tokio works");
        }

        let mut keep_polling = true;
        while keep_polling {
            let swarm_fut = swarm.clone();
            keep_polling = rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
                let mut swarm = swarm_fut.lock();
                match swarm.poll() {
                    Async::Ready(event) => {
                        assert_matches!(event, RawSwarmEvent::NodeEvent { peer_id: _, event: inner_event } => {
                            // The event we sent reached the node and triggered sending the out event we told it to return
                            assert_matches!(inner_event, OutEvent::Custom("from handler 1"));
                        });
                        Ok(Async::Ready(false))
                    },
                    _ => Ok(Async::Ready(true))
                }
            })).expect("tokio works");
        }
    }

    #[test]
    fn querying_for_pending_peer() {
        let mut swarm = RawSwarm::<_, _, _, Handler>::new(DummyTransport::new());
        let peer_id = PeerId::random();
        let peer = swarm.peer(peer_id.clone());
        assert_matches!(peer, Peer::NotConnected(PeerNotConnected{ .. }));
        let addr = "/memory".parse().expect("bad multiaddr");
        let pending_peer = peer.as_not_connected().unwrap().connect(addr, Handler::default());
        assert!(pending_peer.is_ok());
        assert_matches!(pending_peer, Ok(PeerPendingConnect { .. } ));
    }

    #[test]
    fn querying_for_unknown_peer() {
        let mut swarm = RawSwarm::<_, _, _, Handler>::new(DummyTransport::new());
        let peer_id = PeerId::random();
        let peer = swarm.peer(peer_id.clone());
        assert_matches!(peer, Peer::NotConnected( PeerNotConnected { nodes: _, peer_id: node_peer_id }) => {
            assert_eq!(node_peer_id, peer_id);
        });
    }

    #[test]
    fn querying_for_connected_peer() {
        let mut swarm = RawSwarm::<_, _, _, Handler>::new(DummyTransport::new());

        // Dial a node
        let addr = "/ip4/127.0.0.1/tcp/1234".parse().expect("bad multiaddr");
        swarm.dial(addr, Handler::default()).expect("dialing works");

        let swarm = Arc::new(Mutex::new(swarm));
        let mut rt = Runtime::new().unwrap();
        // Drive it forward until we connect; extract the new PeerId.
        let mut peer_id : Option<PeerId> = None;
        while peer_id.is_none() {
            let swarm_fut = swarm.clone();
            peer_id = rt.block_on(future::poll_fn(move || -> Poll<Option<PeerId>, ()> {
                let mut swarm = swarm_fut.lock();
                let poll_res = swarm.poll();
                match poll_res {
                    Async::Ready(RawSwarmEvent::Connected { peer_id, .. }) => Ok(Async::Ready(Some(peer_id))),
                    _ => Ok(Async::Ready(None))
                }
            })).expect("tokio works");
        }

        // We're connected.
        let mut swarm = swarm.lock();
        let peer = swarm.peer(peer_id.unwrap());
        assert_matches!(peer, Peer::Connected( PeerConnected { .. } ));
    }

    #[test]
    fn poll_with_closed_listener() {
        let mut transport = DummyTransport::new();
        // Set up listener to be closed
        transport.set_initial_listener_state(ListenerState::Ok(Async::Ready(None)));

        let mut swarm = RawSwarm::<_, _, _, Handler>::new(transport);
        swarm.listen_on("/memory".parse().unwrap()).unwrap();

        let mut rt = Runtime::new().unwrap();
        let swarm = Arc::new(Mutex::new(swarm));

        let swarm_fut = swarm.clone();
        let fut = future::poll_fn(move || -> Poll<_, ()> {
            let mut swarm = swarm_fut.lock();
            assert_matches!(swarm.poll(), Async::Ready(RawSwarmEvent::ListenerClosed { .. } ));
            Ok(Async::Ready(()))
        });
        rt.block_on(fut).expect("tokio works");
    }

    #[test]
    fn unknown_peer_that_is_unreachable_yields_unknown_peer_dial_error() {
        let mut transport = DummyTransport::new();
        transport.make_dial_fail();
        let mut swarm = RawSwarm::<_, _, _, Handler>::new(transport);
        let addr = "/memory".parse::<Multiaddr>().expect("bad multiaddr");
        let handler = Handler::default();
        let dial_result = swarm.dial(addr, handler);
        assert!(dial_result.is_ok());

        let swarm = Arc::new(Mutex::new(swarm));
        let mut rt = Runtime::new().unwrap();
        // Drive it forward until we hear back from the node.
        let mut keep_polling = true;
        while keep_polling {
            let swarm_fut = swarm.clone();
            keep_polling = rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
                let mut swarm = swarm_fut.lock();
                match swarm.poll() {
                    Async::NotReady => Ok(Async::Ready(true)),
                    Async::Ready(event) => {
                        assert_matches!(event, RawSwarmEvent::UnknownPeerDialError { .. } );
                        Ok(Async::Ready(false))
                    },
                }
            })).expect("tokio works");
        }
    }

    #[test]
    fn known_peer_that_is_unreachable_yields_dial_error() {
        let mut transport = DummyTransport::new();
        let peer_id = PeerId::random();
        transport.set_next_peer_id(&peer_id);
        transport.make_dial_fail();
        let swarm = Arc::new(Mutex::new(RawSwarm::<_, _, _, Handler>::new(transport)));

        {
            let swarm1 = swarm.clone();
            let mut swarm1 = swarm1.lock();
            let peer = swarm1.peer(peer_id.clone());
            assert_matches!(peer, Peer::NotConnected(PeerNotConnected{ .. }));
            let addr = "/memory".parse::<Multiaddr>().expect("bad multiaddr");
            let pending_peer = peer.as_not_connected().unwrap().connect(addr, Handler::default());
            assert!(pending_peer.is_ok());
            assert_matches!(pending_peer, Ok(PeerPendingConnect { .. } ));
        }
        let mut rt = Runtime::new().unwrap();
        // Drive it forward until we hear back from the node.
        let mut keep_polling = true;
        while keep_polling {
            let swarm_fut = swarm.clone();
            let peer_id = peer_id.clone();
            keep_polling = rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
                let mut swarm = swarm_fut.lock();
                match swarm.poll() {
                    Async::NotReady => Ok(Async::Ready(true)),
                    Async::Ready(event) => {
                        let failed_peer_id = assert_matches!(
                            event,
                            RawSwarmEvent::DialError { remain_addrs_attempt: _, peer_id: failed_peer_id, .. } => failed_peer_id
                        );
                        assert_eq!(peer_id, failed_peer_id);
                        Ok(Async::Ready(false))
                    },
                }
            })).expect("tokio works");
        }
    }

    #[test]
    fn yields_node_error_when_there_is_an_error_after_successful_connect() {
        let mut transport = DummyTransport::new();
        let peer_id = PeerId::random();
        transport.set_next_peer_id(&peer_id);
        let swarm = Arc::new(Mutex::new(RawSwarm::<_, _, _, Handler>::new(transport)));

        {
            // Set up an outgoing connection with a PeerId we know
            let swarm1 = swarm.clone();
            let mut swarm1 = swarm1.lock();
            let peer = swarm1.peer(peer_id.clone());
            let addr = "/unix/reachable".parse().expect("bad multiaddr");
            let mut handler = Handler::default();
            // Force an error
            handler.next_states = vec![ HandlerState::Err ];
            peer.as_not_connected().unwrap().connect(addr, handler).expect("can connect unconnected peer");
        }

        // Ensure we run on a single thread
        let mut rt = Builder::new().core_threads(1).build().unwrap();

        // Drive it forward until we connect to the node.
        let mut keep_polling = true;
        while keep_polling {
            let swarm_fut = swarm.clone();
            keep_polling = rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
                let mut swarm = swarm_fut.lock();
                // Push the Handler into an error state on the next poll
                swarm.broadcast_event(&InEvent::NextState);
                match swarm.poll() {
                    Async::NotReady => Ok(Async::Ready(true)),
                    Async::Ready(event) => {
                        assert_matches!(event, RawSwarmEvent::Connected { .. });
                        // We're connected, we can move on
                        Ok(Async::Ready(false))
                    },
                }
            })).expect("tokio works");
        }

        // Poll again. It is going to be a NodeError because of how the
        // handler's next state was set up.
        let swarm_fut = swarm.clone();
        let expected_peer_id = peer_id.clone();
        rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
            let mut swarm = swarm_fut.lock();
            assert_matches!(swarm.poll(), Async::Ready(RawSwarmEvent::NodeError { peer_id, .. }) => {
                assert_eq!(peer_id, expected_peer_id);
            });
            Ok(Async::Ready(()))
        })).expect("tokio works");
    }

    #[test]
    fn yields_node_closed_when_the_node_closes_after_successful_connect() {
        let mut transport = DummyTransport::new();
        let peer_id = PeerId::random();
        transport.set_next_peer_id(&peer_id);
        let swarm = Arc::new(Mutex::new(RawSwarm::<_, _, _, Handler>::new(transport)));

        {
            // Set up an outgoing connection with a PeerId we know
            let swarm1 = swarm.clone();
            let mut swarm1 = swarm1.lock();
            let peer = swarm1.peer(peer_id.clone());
            let addr = "/unix/reachable".parse().expect("bad multiaddr");
            let mut handler = Handler::default();
            // Force handler to close
            handler.next_states = vec![ HandlerState::Ready(None) ];
            peer.as_not_connected().unwrap().connect(addr, handler).expect("can connect unconnected peer");
        }

        // Ensure we run on a single thread
        let mut rt = Builder::new().core_threads(1).build().unwrap();

        // Drive it forward until we connect to the node.
        let mut keep_polling = true;
        while keep_polling {
            let swarm_fut = swarm.clone();
            keep_polling = rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
                let mut swarm = swarm_fut.lock();
                // Push the Handler into the closed state on the next poll
                swarm.broadcast_event(&InEvent::NextState);
                match swarm.poll() {
                    Async::NotReady => Ok(Async::Ready(true)),
                    Async::Ready(event) => {
                        assert_matches!(event, RawSwarmEvent::Connected { .. });
                        // We're connected, we can move on
                        Ok(Async::Ready(false))
                    },
                }
            })).expect("tokio works");
        }

        // Poll again. It is going to be a NodeClosed because of how the
        // handler's next state was set up.
        let swarm_fut = swarm.clone();
        let expected_peer_id = peer_id.clone();
        rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
            let mut swarm = swarm_fut.lock();
            assert_matches!(swarm.poll(), Async::Ready(RawSwarmEvent::NodeClosed { peer_id, .. }) => {
                assert_eq!(peer_id, expected_peer_id);
            });
            Ok(Async::Ready(()))
        })).expect("tokio works");
    }
}
