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

use fnv::FnvHashMap;
use futures::{prelude::*, future};
use muxing;
use nodes::collection::{
    CollectionEvent, CollectionStream, PeerMut as CollecPeerMut, ReachAttemptId,
};
use nodes::listeners::{ListenersEvent, ListenersStream};
use nodes::node::Substream;
use std::collections::hash_map::{Entry, OccupiedEntry};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use void::Void;
use {Endpoint, Multiaddr, PeerId, Transport};

/// Implementation of `Stream` that handles the nodes.
pub struct Swarm<TTrans, TMuxer, TUserData>
where
    TTrans: Transport,
    TMuxer: muxing::StreamMuxer,
{
    /// Listeners for incoming connections.
    listeners: ListenersStream<TTrans>,

    /// The nodes currently active.
    active_nodes: CollectionStream<TMuxer, TUserData>,

    /// Attempts to reach a peer.
    out_reach_attempts: FnvHashMap<PeerId, OutReachAttempt>,

    /// Reach attempts for incoming connections, and outgoing connections for which we don't know
    /// the peer ID.
    other_reach_attempts: Vec<(ReachAttemptId, ConnectedPoint)>,

    /// For each peer ID we're connected to, contains the multiaddress we're connected to.
    connected_multiaddresses: FnvHashMap<PeerId, Multiaddr>,
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

/// Event that can happen on the `Swarm`.
pub enum SwarmEvent<TTrans, TMuxer, TUserData>
where
    TTrans: Transport,
    TMuxer: muxing::StreamMuxer,
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
    IncomingConnection {
        /// Address of the listener which received the connection.
        listen_addr: Multiaddr,
    },

    /// An error happened when negotiating a new connection.
    IncomingConnectionError {
        /// Address of the listener which received the connection.
        listen_addr: Multiaddr,
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
        /// Outbound substream attempts that have been closed in the process.
        closed_outbound_substreams: Vec<TUserData>,
        /// Multiaddr we were connected to, or `None` if it was unknown.
        closed_multiaddr: Option<Multiaddr>,
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
        /// Address we were connected to. `None` if not known.
        address: Option<Multiaddr>,
    },

    /// The muxer of a node has produced an error.
    NodeError {
        /// Identifier of the node.
        peer_id: PeerId,
        /// Address we were connected to. `None` if not known.
        address: Option<Multiaddr>,
        /// The error that happened.
        error: IoError,
        /// Pending outbound substreams that were cancelled.
        closed_outbound_substreams: Vec<TUserData>,
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
    },

    /// When dialing a peer, we successfully connected to a remote whose peer id doesn't match
    /// what we expected.
    PublicKeyMismatch {
        /// Id of the peer we were expecting.
        expected_peer_id: PeerId,

        /// Id of the peer we actually obtained.
        actual_peer_id: PeerId,

        /// The multiaddr we failed to reach.
        multiaddr: Multiaddr,

        /// Returns the number of multiaddresses that still need to be attempted in order to reach
        /// `expected_peer_id`. If this is non-zero, then there's still a chance we can connect to
        /// this node. If this is zero, then we have definitely failed.
        remain_addrs_attempt: usize,
    },

    /// A new inbound substream arrived.
    InboundSubstream {
        /// Id of the peer we received a substream from.
        peer_id: PeerId,
        /// The newly-opened substream.
        substream: Substream<TMuxer>,
    },

    /// An outbound substream has successfully been opened.
    OutboundSubstream {
        /// Id of the peer we received a substream from.
        peer_id: PeerId,
        /// User data that has been passed to the `open_substream` method.
        user_data: TUserData,
        /// The newly-opened substream.
        substream: Substream<TMuxer>,
    },

    /// The inbound side of a muxer has been gracefully closed. No more inbound substreams will
    /// be produced.
    InboundClosed {
        /// Id of the peer.
        peer_id: PeerId,
    },

    /// An outbound substream couldn't be opened because the muxer is no longer capable of opening
    /// more substreams.
    OutboundClosed {
        /// Id of the peer we were trying to open a substream with.
        peer_id: PeerId,
        /// User data that has been passed to the `open_substream` method.
        user_data: TUserData,
    },

    /// The multiaddress of the node has been resolved.
    NodeMultiaddr {
        /// Identifier of the node.
        peer_id: PeerId,
        /// Address that has been resolved.
        address: Result<Multiaddr, IoError>,
    },
}

/// How we connected to a node.
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
    },
}

impl From<ConnectedPoint> for Endpoint {
    #[inline]
    fn from(endpoint: ConnectedPoint) -> Endpoint {
        match endpoint {
            ConnectedPoint::Dialer { .. } => Endpoint::Dialer,
            ConnectedPoint::Listener { .. } => Endpoint::Listener,
        }
    }
}

impl ConnectedPoint {
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

impl<TTrans, TMuxer, TUserData> Swarm<TTrans, TMuxer, TUserData>
where
    TTrans: Transport + Clone,
    TMuxer: muxing::StreamMuxer,
{
    /// Creates a new node events stream.
    #[inline]
    pub fn new(transport: TTrans) -> Self {
        // TODO: with_capacity?
        Swarm {
            listeners: ListenersStream::new(transport),
            active_nodes: CollectionStream::new(),
            out_reach_attempts: Default::default(),
            other_reach_attempts: Vec::new(),
            connected_multiaddresses: Default::default(),
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
    ) -> impl Iterator<Item = Multiaddr> + 'a {
        self.listeners()
            .flat_map(move |server| self.transport().nat_traversal(server, observed_addr))
    }

    /// Dials a multiaddress without knowing the peer ID we're going to obtain.
    pub fn dial(&mut self, addr: Multiaddr) -> Result<(), Multiaddr>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TUserData: Send + 'static,
    {
        let future = match self.transport().clone().dial(addr.clone()) {
            Ok(fut) => fut,
            Err((_, addr)) => return Err(addr),
        };

        let reach_id = self.active_nodes.add_reach_attempt(future);
        self.other_reach_attempts
            .push((reach_id, ConnectedPoint::Dialer { address: addr }));
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
        self.other_reach_attempts
            .iter()
            .filter(|&(_, endpoint)| endpoint.is_listener())
            .count()
    }

    /// Grants access to a struct that represents a peer.
    #[inline]
    pub fn peer(&mut self, peer_id: PeerId) -> Peer<TTrans, TMuxer, TUserData>
    where
        TUserData: Send + 'static,
    {
        // TODO: we do `peer_mut(...).is_some()` followed with `peer_mut(...).unwrap()`, otherwise
        // the borrow checker yells at us.

        if self.active_nodes.peer_mut(&peer_id).is_some() {
            debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
            return Peer::Connected(PeerConnected {
                peer: self
                    .active_nodes
                    .peer_mut(&peer_id)
                    .expect("we checked for Some"),
                peer_id,
                connected_multiaddresses: &mut self.connected_multiaddresses,
            });
        }

        if self.out_reach_attempts.get_mut(&peer_id).is_some() {
            debug_assert!(!self.connected_multiaddresses.contains_key(&peer_id));
            return Peer::PendingConnect(PeerPendingConnect {
                attempt: match self.out_reach_attempts.entry(peer_id.clone()) {
                    Entry::Occupied(e) => e,
                    Entry::Vacant(_) => panic!("we checked for Some just above"),
                },
                active_nodes: &mut self.active_nodes,
            });
        }

        debug_assert!(!self.connected_multiaddresses.contains_key(&peer_id));
        Peer::NotConnected(PeerNotConnected {
            nodes: self,
            peer_id,
        })
    }

    /// Handles a node reached event from the collection.
    ///
    /// Returns an event to return from the stream.
    ///
    /// > **Note**: The event **must** have been produced by the collection of nodes, otherwise
    /// >           panics will likely happen.
    fn handle_node_reached(
        &mut self,
        peer_id: PeerId,
        reach_id: ReachAttemptId,
        closed_outbound_substreams: Option<Vec<TUserData>>,
    ) -> SwarmEvent<TTrans, TMuxer, TUserData>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TUserData: Send + 'static,
    {
        // We first start looking in the incoming attempts. While this makes the code less optimal,
        // it also makes the logic easier.
        if let Some(in_pos) = self
            .other_reach_attempts
            .iter()
            .position(|i| i.0 == reach_id)
        {
            let (_, endpoint) = self.other_reach_attempts.swap_remove(in_pos);

            // Clear the known multiaddress for this peer.
            let closed_multiaddr = self.connected_multiaddresses.remove(&peer_id);
            // Cancel any outgoing attempt to this peer.
            if let Some(attempt) = self.out_reach_attempts.remove(&peer_id) {
                debug_assert_ne!(attempt.id, reach_id);
                self.active_nodes
                    .interrupt(attempt.id)
                    .expect("State inconsistency: invalid reach attempt cancel");
            }

            if let Some(closed_outbound_substreams) = closed_outbound_substreams {
                return SwarmEvent::Replaced {
                    peer_id,
                    endpoint,
                    closed_multiaddr,
                    closed_outbound_substreams,
                };
            } else {
                return SwarmEvent::Connected { peer_id, endpoint };
            }
        }

        // Otherwise, try for outgoing attempts.
        let is_outgoing_and_ok = if let Some(attempt) = self.out_reach_attempts.get(&peer_id) {
            attempt.id == reach_id
        } else {
            false
        };

        // We only remove the attempt from `out_reach_attempts` if it both matches the reach id
        // and the expected peer id.
        if is_outgoing_and_ok {
            let attempt = self.out_reach_attempts.remove(&peer_id).unwrap();

            let closed_multiaddr = self.connected_multiaddresses
                .insert(peer_id.clone(), attempt.cur_attempted.clone());
            let endpoint = ConnectedPoint::Dialer {
                address: attempt.cur_attempted,
            };

            if let Some(closed_outbound_substreams) = closed_outbound_substreams {
                return SwarmEvent::Replaced {
                    peer_id,
                    endpoint,
                    closed_multiaddr,
                    closed_outbound_substreams,
                };
            } else {
                return SwarmEvent::Connected { peer_id, endpoint };
            }
        }

        // If in neither, check outgoing reach attempts again as we may have a public
        // key mismatch.
        let wrong_peer_id = self
            .out_reach_attempts
            .iter()
            .find(|(_, a)| a.id == reach_id)
            .map(|(p, _)| p.clone());
        if let Some(wrong_peer_id) = wrong_peer_id {
            let attempt = self.out_reach_attempts.remove(&wrong_peer_id).unwrap();

            let num_remain = attempt.next_attempts.len();
            let failed_addr = attempt.cur_attempted.clone();

            let opened_attempts = self.active_nodes.peer_mut(&peer_id)
                .expect("Inconsistent state ; received NodeReached event for invalid node")
                .close();
            debug_assert!(opened_attempts.is_empty());

            if !attempt.next_attempts.is_empty() {
                let mut attempt = attempt;
                attempt.cur_attempted = attempt.next_attempts.remove(0);
                attempt.id = match self.transport().clone().dial(attempt.cur_attempted.clone()) {
                    Ok(fut) => self.active_nodes.add_reach_attempt(fut),
                    Err((_, addr)) => {
                        let msg = format!("unsupported multiaddr {}", addr);
                        let fut = future::err(IoError::new(IoErrorKind::Other, msg));
                        self.active_nodes.add_reach_attempt::<_, future::FutureResult<Multiaddr, IoError>>(fut)
                    },
                };

                self.out_reach_attempts.insert(peer_id.clone(), attempt);
            }

            return SwarmEvent::PublicKeyMismatch {
                remain_addrs_attempt: num_remain,
                expected_peer_id: peer_id,
                actual_peer_id: wrong_peer_id,
                multiaddr: failed_addr,
            };
        }

        // We didn't find any entry in neither the outgoing connections not ingoing connections.
        panic!("State inconsistency ; received unknown ReachAttemptId in NodeReached")
    }

    /// Handles a reach error event from the collection.
    ///
    /// Optionally returns an event to return from the stream.
    ///
    /// > **Note**: The event **must** have been produced by the collection of nodes, otherwise
    /// >           panics will likely happen.
    fn handle_reach_error(
        &mut self,
        reach_id: ReachAttemptId,
        error: IoError,
    ) -> Option<SwarmEvent<TTrans, TMuxer, TUserData>>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TUserData: Send + 'static,
    {
        // Search for the attempt in `out_reach_attempts`.
        // TODO: could be more optimal than iterating over everything
        let out_reach_peer_id = self
            .out_reach_attempts
            .iter()
            .find(|(_, a)| a.id == reach_id)
            .map(|(p, _)| p.clone());
        if let Some(peer_id) = out_reach_peer_id {
            let mut attempt = self.out_reach_attempts.remove(&peer_id).unwrap();

            let num_remain = attempt.next_attempts.len();
            let failed_addr = attempt.cur_attempted.clone();

            if !attempt.next_attempts.is_empty() {
                let mut attempt = attempt;
                attempt.cur_attempted = attempt.next_attempts.remove(0);
                attempt.id = match self.transport().clone().dial(attempt.cur_attempted.clone()) {
                    Ok(fut) => self.active_nodes.add_reach_attempt(fut),
                    Err((_, addr)) => {
                        let msg = format!("unsupported multiaddr {}", addr);
                        let fut = future::err(IoError::new(IoErrorKind::Other, msg));
                        self.active_nodes.add_reach_attempt::<_, future::FutureResult<Multiaddr, IoError>>(fut)
                    },
                };

                self.out_reach_attempts.insert(peer_id.clone(), attempt);
            }

            return Some(SwarmEvent::DialError {
                remain_addrs_attempt: num_remain,
                peer_id,
                multiaddr: failed_addr,
                error,
            });
        }

        // If this is not an outgoing reach attempt, check the incoming reach attempts.
        if let Some(in_pos) = self
            .other_reach_attempts
            .iter()
            .position(|i| i.0 == reach_id)
        {
            let (_, endpoint) = self.other_reach_attempts.swap_remove(in_pos);
            match endpoint {
                ConnectedPoint::Dialer { address } => {
                    return Some(SwarmEvent::UnknownPeerDialError {
                        multiaddr: address,
                        error,
                    });
                }
                ConnectedPoint::Listener { listen_addr } => {
                    return Some(SwarmEvent::IncomingConnectionError { listen_addr, error });
                }
            }
        }

        // The id was neither in the outbound list nor the inbound list.
        panic!("State inconsistency: received unknown ReachAttemptId")
    }
}

/// State of a peer in the system.
pub enum Peer<'a, TTrans, TMuxer, TUserData>
where
    TTrans: Transport + 'a,
    TMuxer: muxing::StreamMuxer + 'a,
    TUserData: Send + 'static,
{
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TUserData>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TMuxer, TUserData>),

    /// We are not connected to this peer at all.
    ///
    /// > **Note**: It is however possible that a pending incoming connection is being negotiated
    /// > and will connect to this peer, but we don't know it yet.
    NotConnected(PeerNotConnected<'a, TTrans, TMuxer, TUserData>),
}

// TODO: add other similar methods that wrap to the ones of `PeerNotConnected`
impl<'a, TTrans, TMuxer, TUserData> Peer<'a, TTrans, TMuxer, TUserData>
where
    TTrans: Transport,
    TMuxer: muxing::StreamMuxer,
    TUserData: Send + 'static,
{
    /// If we are connected, returns the `PeerConnected`.
    #[inline]
    pub fn as_connected(self) -> Option<PeerConnected<'a, TUserData>> {
        match self {
            Peer::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    #[inline]
    pub fn as_pending_connect(self) -> Option<PeerPendingConnect<'a, TMuxer, TUserData>> {
        match self {
            Peer::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }

    /// If we are not connected, returns the `PeerNotConnected`.
    #[inline]
    pub fn as_not_connected(self) -> Option<PeerNotConnected<'a, TTrans, TMuxer, TUserData>> {
        match self {
            Peer::NotConnected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If we're not connected, opens a new connection to this peer using the given multiaddr.
    #[inline]
    pub fn or_connect(
        self,
        addr: Multiaddr,
    ) -> Result<PeerPotentialConnect<'a, TMuxer, TUserData>, Self>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
    {
        self.or_connect_with(move |_| addr)
    }

    /// If we're not connected, calls the function passed as parameter and opens a new connection
    /// using the returned address.
    #[inline]
    pub fn or_connect_with<TFn>(
        self,
        addr: TFn,
    ) -> Result<PeerPotentialConnect<'a, TMuxer, TUserData>, Self>
    where
        TFn: FnOnce(&PeerId) -> Multiaddr,
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
    {
        match self {
            Peer::Connected(peer) => Ok(PeerPotentialConnect::Connected(peer)),
            Peer::PendingConnect(peer) => Ok(PeerPotentialConnect::PendingConnect(peer)),
            Peer::NotConnected(peer) => {
                let addr = addr(&peer.peer_id);
                match peer.connect(addr) {
                    Ok(peer) => Ok(PeerPotentialConnect::PendingConnect(peer)),
                    Err(peer) => Err(Peer::NotConnected(peer)),
                }
            }
        }
    }
}

/// Peer we are potentially going to connect to.
pub enum PeerPotentialConnect<'a, TMuxer, TUserData>
where
    TUserData: Send + 'static,
    TMuxer: muxing::StreamMuxer + 'a,
{
    /// We are connected to this peer.
    Connected(PeerConnected<'a, TUserData>),

    /// We are currently attempting to connect to this peer.
    PendingConnect(PeerPendingConnect<'a, TMuxer, TUserData>),
}

impl<'a, TMuxer, TUserData> PeerPotentialConnect<'a, TMuxer, TUserData>
where
    TUserData: Send + 'static,
    TMuxer: muxing::StreamMuxer,
{
    /// Closes the connection or the connection attempt.
    ///
    /// If the connection was active, returns the list of outbound substream openings that were
    /// closed in the process.
    // TODO: consider returning a `PeerNotConnected`
    #[inline]
    pub fn close(self) -> Vec<TUserData> {
        match self {
            PeerPotentialConnect::Connected(peer) => peer.close(),
            PeerPotentialConnect::PendingConnect(peer) => {
                peer.interrupt();
                Vec::new()
            }
        }
    }

    /// If we are connected, returns the `PeerConnected`.
    #[inline]
    pub fn as_connected(self) -> Option<PeerConnected<'a, TUserData>> {
        match self {
            PeerPotentialConnect::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `PeerPendingConnect`.
    #[inline]
    pub fn as_pending_connect(self) -> Option<PeerPendingConnect<'a, TMuxer, TUserData>> {
        match self {
            PeerPotentialConnect::PendingConnect(peer) => Some(peer),
            _ => None,
        }
    }
}

/// Access to a peer we are connected to.
pub struct PeerConnected<'a, TUserData>
where
    TUserData: Send + 'static,
{
    peer: CollecPeerMut<'a, TUserData>,
    /// Reference to the `connected_multiaddresses` field of the parent.
    connected_multiaddresses: &'a mut FnvHashMap<PeerId, Multiaddr>,
    peer_id: PeerId,
}

impl<'a, TUserData> PeerConnected<'a, TUserData>
where
    TUserData: Send + 'static,
{
    /// Closes the connection to this node.
    ///
    /// This interrupts all the current substream opening attempts and returns them.
    /// No `NodeClosed` message will be generated for this node.
    // TODO: consider returning a `PeerNotConnected` ; however this makes all the borrows things
    // much more annoying to deal with
    pub fn close(self) -> Vec<TUserData> {
        self.connected_multiaddresses.remove(&self.peer_id);
        self.peer.close()
    }

    /// Returns the outcome of the future that resolves the multiaddress of the peer.
    #[inline]
    pub fn multiaddr(&self) -> Option<&Multiaddr> {
        self.connected_multiaddresses.get(&self.peer_id)
    }

    /// Starts the process of opening a new outbound substream towards the peer.
    #[inline]
    pub fn open_substream(&mut self, user_data: TUserData) {
        self.peer.open_substream(user_data)
    }
}

/// Access to a peer we are attempting to connect to.
pub struct PeerPendingConnect<'a, TMuxer, TUserData>
where
    TUserData: Send + 'static,
    TMuxer: muxing::StreamMuxer + 'a,
{
    attempt: OccupiedEntry<'a, PeerId, OutReachAttempt>,
    active_nodes: &'a mut CollectionStream<TMuxer, TUserData>,
}

impl<'a, TMuxer, TUserData> PeerPendingConnect<'a, TMuxer, TUserData>
where
    TUserData: Send + 'static,
    TMuxer: muxing::StreamMuxer,
{
    /// Interrupt this connection attempt.
    // TODO: consider returning a PeerNotConnected ; however that is really pain in terms of
    // borrows
    #[inline]
    pub fn interrupt(self) {
        let attempt = self.attempt.remove();
        if let Err(_) = self.active_nodes.interrupt(attempt.id) {
            panic!("State inconsistency ; interrupted invalid ReachAttemptId");
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
pub struct PeerNotConnected<'a, TTrans, TMuxer, TUserData>
where
    TTrans: Transport + 'a,
    TMuxer: muxing::StreamMuxer + 'a,
    TUserData: Send + 'a,
{
    peer_id: PeerId,
    nodes: &'a mut Swarm<TTrans, TMuxer, TUserData>,
}

impl<'a, TTrans, TMuxer, TUserData> PeerNotConnected<'a, TTrans, TMuxer, TUserData>
where
    TTrans: Transport,
    TMuxer: muxing::StreamMuxer,
    TUserData: Send,
{
    /// Attempts a new connection to this node using the given multiaddress.
    #[inline]
    pub fn connect(self, addr: Multiaddr) -> Result<PeerPendingConnect<'a, TMuxer, TUserData>, Self>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TUserData: 'static,
    {
        self.connect_inner(addr, Vec::new())
    }

    /// Attempts a new connection to this node using the given multiaddresses.
    ///
    /// The multiaddresses passes as parameter will be tried one by one.
    ///
    /// If the iterator is empty, TODO: what to do? at the moment we unwrap
    #[inline]
    pub fn connect_iter<TIter>(
        self,
        addrs: TIter,
    ) -> Result<PeerPendingConnect<'a, TMuxer, TUserData>, Self>
    where
        TIter: IntoIterator<Item = Multiaddr>,
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TUserData: 'static,
    {
        let mut addrs = addrs.into_iter();
        let first = addrs.next().unwrap(); // TODO: bad
        let rest = addrs.collect();
        self.connect_inner(first, rest)
    }

    /// Inner implementation of `connect`.
    fn connect_inner(
        self,
        first: Multiaddr,
        rest: Vec<Multiaddr>,
    ) -> Result<PeerPendingConnect<'a, TMuxer, TUserData>, Self>
    where
        TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
        TTrans::Dial: Send + 'static,
        TTrans::MultiaddrFuture: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TUserData: 'static,
    {
        let future = match self.nodes.transport().clone().dial(first.clone()) {
            Ok(fut) => fut,
            Err(_) => return Err(self),
        };

        let reach_id = self.nodes.active_nodes.add_reach_attempt(future);

        let former = self.nodes.out_reach_attempts.insert(
            self.peer_id.clone(),
            OutReachAttempt {
                id: reach_id,
                cur_attempted: first,
                next_attempts: rest,
            },
        );
        debug_assert!(former.is_none());

        Ok(PeerPendingConnect {
            attempt: match self.nodes.out_reach_attempts.entry(self.peer_id) {
                Entry::Occupied(e) => e,
                Entry::Vacant(_) => panic!("We inserted earlier"),
            },
            active_nodes: &mut self.nodes.active_nodes,
        })
    }
}

impl<TTrans, TMuxer, TUserData> Stream for Swarm<TTrans, TMuxer, TUserData>
where
    TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
    TTrans::Dial: Send + 'static,
    TTrans::MultiaddrFuture: Future<Item = Multiaddr, Error = IoError> + Send + 'static,
    TTrans::ListenerUpgrade: Send + 'static,
    TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TUserData: Send + 'static,
{
    type Item = SwarmEvent<TTrans, TMuxer, TUserData>;
    type Error = Void; // TODO: use `!` once stable

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Start by polling the listeners for events.
        match self.listeners.poll() {
            Ok(Async::NotReady) => (),
            Ok(Async::Ready(Some(ListenersEvent::Incoming {
                upgrade,
                listen_addr,
            }))) => {
                let id = self.active_nodes.add_reach_attempt(upgrade);
                self.other_reach_attempts.push((
                    id,
                    ConnectedPoint::Listener {
                        listen_addr: listen_addr.clone(),
                    },
                ));
                return Ok(Async::Ready(Some(SwarmEvent::IncomingConnection {
                    listen_addr,
                })));
            }
            Ok(Async::Ready(Some(ListenersEvent::Closed {
                listen_addr,
                listener,
                result,
            }))) => {
                return Ok(Async::Ready(Some(SwarmEvent::ListenerClosed {
                    listen_addr,
                    listener,
                    result,
                })));
            }
            Ok(Async::Ready(None)) => unreachable!("The listeners stream never finishes"),
            Err(_) => unreachable!("The listeners stream never errors"), // TODO: remove variant
        }

        // Poll the existing nodes.
        loop {
            match self.active_nodes.poll() {
                Ok(Async::NotReady) => break,
                Ok(Async::Ready(Some(CollectionEvent::NodeReached { peer_id, id }))) => {
                    let event = self.handle_node_reached(peer_id, id, None);
                    return Ok(Async::Ready(Some(event)));
                }
                Ok(Async::Ready(Some(CollectionEvent::NodeReplaced {
                    peer_id,
                    id,
                    closed_outbound_substreams,
                }))) => {
                    let event = self.handle_node_reached(peer_id, id, Some(closed_outbound_substreams));
                    return Ok(Async::Ready(Some(event)));
                }
                Ok(Async::Ready(Some(CollectionEvent::ReachError { id, error }))) => {
                    if let Some(event) = self.handle_reach_error(id, error) {
                        return Ok(Async::Ready(Some(event)));
                    }
                }
                Ok(Async::Ready(Some(CollectionEvent::NodeError {
                    peer_id,
                    error,
                    closed_outbound_substreams,
                }))) => {
                    let address = self.connected_multiaddresses.remove(&peer_id);
                    debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
                    return Ok(Async::Ready(Some(SwarmEvent::NodeError {
                        peer_id,
                        address,
                        error,
                        closed_outbound_substreams,
                    })));
                }
                Ok(Async::Ready(Some(CollectionEvent::NodeClosed { peer_id }))) => {
                    let address = self.connected_multiaddresses.remove(&peer_id);
                    debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
                    return Ok(Async::Ready(Some(SwarmEvent::NodeClosed { peer_id, address })));
                }
                Ok(Async::Ready(Some(CollectionEvent::NodeMultiaddr { peer_id, address }))) => {
                    debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
                    if let Ok(ref addr) = address {
                        self.connected_multiaddresses
                            .insert(peer_id.clone(), addr.clone());
                    }
                    return Ok(Async::Ready(Some(SwarmEvent::NodeMultiaddr {
                        peer_id,
                        address,
                    })));
                }
                Ok(Async::Ready(Some(CollectionEvent::InboundSubstream {
                    peer_id,
                    substream,
                }))) => {
                    debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
                    return Ok(Async::Ready(Some(SwarmEvent::InboundSubstream {
                        peer_id,
                        substream,
                    })));
                }
                Ok(Async::Ready(Some(CollectionEvent::OutboundSubstream {
                    peer_id,
                    user_data,
                    substream,
                }))) => {
                    debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
                    return Ok(Async::Ready(Some(SwarmEvent::OutboundSubstream {
                        peer_id,
                        substream,
                        user_data,
                    })));
                }
                Ok(Async::Ready(Some(CollectionEvent::InboundClosed { peer_id }))) => {
                    debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
                    return Ok(Async::Ready(Some(SwarmEvent::InboundClosed { peer_id })));
                }
                Ok(Async::Ready(Some(CollectionEvent::OutboundClosed { peer_id, user_data }))) => {
                    debug_assert!(!self.out_reach_attempts.contains_key(&peer_id));
                    return Ok(Async::Ready(Some(SwarmEvent::OutboundClosed {
                        peer_id,
                        user_data,
                    })));
                }
                Ok(Async::Ready(None)) => unreachable!("CollectionStream never ends"),
                Err(_) => unreachable!("CollectionStream never errors"),
            }
        }

        Ok(Async::NotReady)
    }
}
