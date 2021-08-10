//! Basic building block for easier building of libp2p protocols. The streaming protocol provides
//! the functionality to establish a bi-directional stream to a remote peer including dialing. In
//! essence, it provides a generic way to write a custom protocol using an opinionated
//! [`NetworkBehaviour`].  The [`Streaming`] behaviour can be constructed with a custom
//! [`StreamingCodec`], which provides a custom protocol definition.
//!
//! ```
//! use libp2p_streaming::{IdentityCodec, Streaming};
//!
//! let behaviour = Streaming::<IdentityCodec>::default();
//! ```
pub use handler::StreamId;
use handler::{RefCount, StreamingProtocolsHandler, StreamingProtocolsHandlerEvent};
use libp2p_core::{connection::ConnectionId, ConnectedPoint, Multiaddr, PeerId, ProtocolName};
use libp2p_swarm::{
    DialPeerCondition, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler,
};
use smallvec::SmallVec;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::{atomic::AtomicU64, Arc},
    task::Poll,
    time::Duration,
};

mod handler;

pub struct Streaming<T: StreamingCodec> {
    /// Pending events to return from `poll`
    pending_events: VecDeque<NetworkBehaviourAction<OutboundStreamId, StreamingEvent<T>>>,
    /// The next (inbound) stream ID
    next_inbound_id: Arc<AtomicU64>,
    /// The next outbound stream ID
    next_stream_id: OutboundStreamId,
    /// The currently connected peers, their pending outbound and
    /// inbound requests and their known, reachable addresses, if any.
    connected: HashMap<PeerId, SmallVec<[Connection; 2]>>,
    /// Externally managed addresses via `add_address` and
    /// `remove_address`.
    addresses: HashMap<PeerId, SmallVec<[Multiaddr; 6]>>,
    /// Requests that have not yet been sent and are waiting for a
    /// connection to be established.
    pending_outbound_requests: HashMap<PeerId, SmallVec<[OutboundStreamId; 10]>>,
    config: StreamingConfig,
    _codec: PhantomData<T>,
}
impl<T: StreamingCodec> Streaming<T> {
    pub fn new(config: StreamingConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }
}
#[derive(Clone, Debug)]
pub struct StreamingConfig {
    /// The keep-alive timeout of idle connections. A connection is considered idle if there are no
    /// substreams.
    keep_alive_timeout: Duration,
    /// The timeout for inbound and outbound substreams .
    substream_timeout: Duration,
}
impl StreamingConfig {
    pub fn new(keep_alive_timeout: Duration, substream_timeout: Duration) -> Self {
        Self {
            keep_alive_timeout,
            substream_timeout,
        }
    }
}
impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            keep_alive_timeout: Duration::from_millis(10_000),
            substream_timeout: Duration::from_millis(5_000),
        }
    }
}
impl<T: StreamingCodec> Default for Streaming<T> {
    fn default() -> Self {
        Self {
            pending_events: Default::default(),
            next_inbound_id: Default::default(),
            next_stream_id: Default::default(),
            connected: Default::default(),
            addresses: Default::default(),
            pending_outbound_requests: Default::default(),
            config: Default::default(),
            _codec: Default::default(),
        }
    }
}

struct Connection {
    id: ConnectionId,
    address: Option<Multiaddr>,
    pending_outbound: HashSet<OutboundStreamId>,
    pending_inbound: HashSet<InboundStreamId>,
}

impl Connection {
    fn new(id: ConnectionId, address: Option<Multiaddr>) -> Self {
        Self {
            id,
            address,
            pending_outbound: Default::default(),
            pending_inbound: Default::default(),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct InboundStreamId(u64);

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct OutboundStreamId(u64);
#[derive(Debug)]
/// Handle associated with an upgraded connection to a remote peer.  The handle can be dereferenced
/// yielding the wrapped &mut T.
pub struct StreamHandle<T> {
    /// The wrapped value.
    inner: T,
    /// Reference counter to indicate whether a connection should be kept alive or not.
    marker: RefCount,
}
impl<T> StreamHandle<T> {
    fn new(inner: T, marker: RefCount) -> Self {
        Self { inner, marker }
    }
}
impl<T> Deref for StreamHandle<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<T> DerefMut for StreamHandle<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Implement this trait to provide a custom StreamingCodec. If you're just interested in the raw
/// bytes (yielding a handle which implements both [`AsyncRead`](futures::io::AsyncRead) and
/// [`AsyncWrite`](futures::io::AsyncWrite)), use [`IdentityCodec`].
pub trait StreamingCodec {
    type Protocol: ProtocolName + Send + Clone;
    type Upgrade: Send;

    fn upgrade(io: NegotiatedSubstream) -> Self::Upgrade;
    fn protocol_name() -> Self::Protocol;
}

#[derive(Debug)]
/// Codec providing the raw bytes. The resulting [`StreamHandle`] can be derefenced yielding an
/// [`AsyncRead`](futures::io::AsyncRead) and [`AsyncWrite`](futures::io::AsyncWrite)
/// implementation.
pub struct IdentityCodec;
impl StreamingCodec for IdentityCodec {
    type Protocol = &'static [u8];

    type Upgrade = NegotiatedSubstream;

    fn upgrade(io: NegotiatedSubstream) -> Self::Upgrade {
        io
    }

    fn protocol_name() -> Self::Protocol {
        b"/streaming/bytes/1.0.0"
    }
}

// Exposed swarm events
#[derive(Debug)]
pub enum StreamingEvent<T: StreamingCodec> {
    // New connection from remote
    NewIncoming {
        peer_id: PeerId,
        id: InboundStreamId,
        stream: StreamHandle<T::Upgrade>,
    },
    StreamOpened {
        peer_id: PeerId,
        id: OutboundStreamId,
        stream: StreamHandle<T::Upgrade>,
    },
    StreamClosed {
        peer_id: PeerId,
        id: StreamId,
    },
    OutboundFailure {
        peer_id: PeerId,
        id: OutboundStreamId,
        error: OutboundFailure,
    },
    InboundFailure {
        peer_id: PeerId,
        id: InboundStreamId,
        error: InboundFailure,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum OutboundFailure {
    #[error("Unable to dial remote.")]
    DialFailure,
    #[error("Hit timeout when trying to reach remote.")]
    Timeout,
    #[error("Connection closed.")]
    ConnectionClosed,
    #[error("Outbound upgrade failed due to the remote not supporting the protocol.")]
    UnsupportedProtocols,
}
#[derive(thiserror::Error, Debug)]
pub enum InboundFailure {
    #[error("Connection closed.")]
    ConnectionClosed,
    #[error("Timeout.")]
    Timeout,
    #[error("Inbound upgrade failed due to the remote not supporting the protocol.")]
    UnsupportedProtocols,
}

impl<T: StreamingCodec> Streaming<T> {
    /// Establish an outbound stream. If the given [`PeerId`] is not yet connected, a dialing
    /// attempt will be initiated. The remote's address needs to be added first through
    /// [`add_address`].
    pub fn open_stream(&mut self, peer_id: PeerId) -> OutboundStreamId {
        let stream_id = self.next_stream_id();
        tracing::trace!(%peer_id, ?stream_id, "Opening stream");
        if !self.try_open(peer_id, stream_id) {
            self.pending_events
                .push_back(NetworkBehaviourAction::DialPeer {
                    peer_id,
                    condition: DialPeerCondition::Disconnected,
                });
            self.pending_outbound_requests
                .entry(peer_id)
                .or_default()
                .push(stream_id);
        }
        stream_id
    }
    fn try_open(&mut self, peer_id: PeerId, id: OutboundStreamId) -> bool {
        if let Some(connections) = self.connected.get_mut(&peer_id) {
            if connections.is_empty() {
                self.connected.remove(&peer_id);
                return false;
            }

            // Use a random connection
            let ix = (id.0 as usize) % connections.len();
            let conn = &mut connections[ix];
            conn.pending_outbound.insert(id);
            self.pending_events
                .push_back(NetworkBehaviourAction::NotifyHandler {
                    peer_id,
                    handler: NotifyHandler::One(conn.id),
                    event: id,
                });
            true
        } else {
            false
        }
    }

    fn next_stream_id(&mut self) -> OutboundStreamId {
        let stream_id = self.next_stream_id;
        self.next_stream_id.0 += 1;
        stream_id
    }

    /// Adds a known address for a peer that can be used for
    /// dialing attempts by the `Swarm`, i.e. is returned
    /// by [`NetworkBehaviour::addresses_of_peer`].
    ///
    /// Addresses added in this way are only removed by `remove_address`.
    pub fn add_address(&mut self, peer: PeerId, address: Multiaddr) {
        self.addresses.entry(peer).or_default().push(address);
    }

    /// Removes an address of a peer previously added via [`add_address`].
    pub fn remove_address(&mut self, peer: PeerId, address: &Multiaddr) {
        let mut last = false;
        if let Some(addresses) = self.addresses.get_mut(&peer) {
            addresses.retain(|a| a != address);
            last = addresses.is_empty();
        }
        if last {
            self.addresses.remove(&peer);
        }
    }

    fn get_connection_mut(
        &mut self,
        peer: &PeerId,
        connection: ConnectionId,
    ) -> Option<&mut Connection> {
        self.connected
            .get_mut(peer)
            .and_then(|connections| connections.iter_mut().find(|c| c.id == connection))
    }

    fn remove_pending_outbound(
        &mut self,
        peer: &PeerId,
        connection: ConnectionId,
        id: OutboundStreamId,
    ) -> bool {
        self.get_connection_mut(peer, connection)
            .map(|c| c.pending_outbound.remove(&id))
            .unwrap_or(false)
    }

    fn remove_pending_inbound(
        &mut self,
        peer: &PeerId,
        connection: ConnectionId,
        id: InboundStreamId,
    ) -> bool {
        self.get_connection_mut(peer, connection)
            .map(|c| c.pending_inbound.remove(&id))
            .unwrap_or(false)
    }
}

impl<T: StreamingCodec + Send + 'static> NetworkBehaviour for Streaming<T> {
    type ProtocolsHandler = StreamingProtocolsHandler<T>;

    type OutEvent = StreamingEvent<T>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        tracing::trace!("new_handler");
        StreamingProtocolsHandler::new(self.next_inbound_id.clone(), self.config.clone())
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        let mut addresses = Vec::new();
        if let Some(connections) = self.connected.get(peer) {
            addresses.extend(connections.iter().filter_map(|c| c.address.clone()))
        }
        if let Some(more) = self.addresses.get(peer) {
            addresses.extend(more.into_iter().cloned());
        }
        addresses
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        if let Some(pending) = self.pending_outbound_requests.remove(peer) {
            for request in pending {
                let sent_request = self.try_open(*peer, request);
                assert!(sent_request);
            }
        }
    }

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        let address = match endpoint {
            ConnectedPoint::Dialer { address } => Some(address.clone()),
            ConnectedPoint::Listener { .. } => None,
        };
        self.connected
            .entry(*peer)
            .or_default()
            .push(Connection::new(*conn, address));
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        conn: &ConnectionId,
        _: &ConnectedPoint,
    ) {
        let connections = self
            .connected
            .get_mut(peer_id)
            .expect("Expected some established connection to peer before closing.");

        let connection = connections
            .iter()
            .position(|c| &c.id == conn)
            .map(|p: usize| connections.remove(p))
            .expect("Expected connection to be established before closing.");

        if connections.is_empty() {
            self.connected.remove(peer_id);
        }

        for id in connection.pending_inbound {
            self.pending_events
                .push_back(NetworkBehaviourAction::GenerateEvent(
                    StreamingEvent::InboundFailure {
                        peer_id: *peer_id,
                        id,
                        error: InboundFailure::ConnectionClosed,
                    },
                ));
        }

        for id in connection.pending_outbound {
            self.pending_events
                .push_back(NetworkBehaviourAction::GenerateEvent(
                    StreamingEvent::OutboundFailure {
                        peer_id: *peer_id,
                        id,
                        error: OutboundFailure::ConnectionClosed,
                    },
                ));
        }
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        self.connected.remove(peer);
    }

    fn inject_dial_failure(&mut self, peer: &PeerId) {
        // Consider any pending outgoing requests to that peer failed.
        if let Some(pending) = self.pending_outbound_requests.remove(peer) {
            for id in pending {
                self.pending_events
                    .push_back(NetworkBehaviourAction::GenerateEvent(
                        StreamingEvent::OutboundFailure {
                            peer_id: *peer,
                            id,
                            error: OutboundFailure::DialFailure,
                        },
                    ));
            }
        }
    }

    fn inject_event(
        &mut self,
        peer_id: libp2p_core::PeerId,
        connection_id: libp2p_core::connection::ConnectionId,
        event: StreamingProtocolsHandlerEvent<T>,
    ) {
        let ev = match event {
            StreamingProtocolsHandlerEvent::NewIncoming { id, stream } => {
                match self.get_connection_mut(&peer_id, connection_id) {
                    Some(connection) => {
                        let inserted = connection.pending_inbound.insert(id);
                        debug_assert!(inserted, "Expect id of new request to be unknown.");
                        StreamingEvent::NewIncoming {
                            peer_id,
                            id,
                            stream,
                        }
                    }
                    // Connection was immediately closed.
                    None => StreamingEvent::InboundFailure {
                        peer_id,
                        id,
                        error: InboundFailure::ConnectionClosed,
                    },
                }
            }
            StreamingProtocolsHandlerEvent::StreamOpened { id, stream } => {
                StreamingEvent::StreamOpened {
                    id,
                    peer_id,
                    stream,
                }
            }
            StreamingProtocolsHandlerEvent::OutboundTimeout(id) => {
                let removed = self.remove_pending_outbound(&peer_id, connection_id, id);
                debug_assert!(removed, "Expect id to be pending before request times out.");

                StreamingEvent::OutboundFailure {
                    peer_id,
                    id,
                    error: OutboundFailure::Timeout,
                }
            }
            StreamingProtocolsHandlerEvent::OutboundUnsupportedProtocols(id) => {
                let removed = self.remove_pending_outbound(&peer_id, connection_id, id);
                debug_assert!(
                    removed,
                    "Expect id to be pending before request is rejected."
                );

                StreamingEvent::OutboundFailure {
                    peer_id,
                    id,
                    error: OutboundFailure::UnsupportedProtocols,
                }
            }
            StreamingProtocolsHandlerEvent::InboundTimeout(id) => StreamingEvent::InboundFailure {
                peer_id,
                id,
                error: InboundFailure::Timeout,
            },
            StreamingProtocolsHandlerEvent::InboundUnsupportedProtocols(id) => {
                StreamingEvent::InboundFailure {
                    peer_id,
                    id,
                    error: InboundFailure::UnsupportedProtocols,
                }
            }
            StreamingProtocolsHandlerEvent::InboundStreamClosed { id } => {
                self.remove_pending_inbound(&peer_id, connection_id, id);
                StreamingEvent::StreamClosed {
                    peer_id,
                    id: StreamId::Inbound(id),
                }
            }
            StreamingProtocolsHandlerEvent::OutboundStreamClosed { id } => {
                self.remove_pending_outbound(&peer_id, connection_id, id);
                StreamingEvent::StreamClosed {
                    peer_id,
                    id: StreamId::Outbound(id),
                }
            }
        };

        self.pending_events
            .push_back(NetworkBehaviourAction::GenerateEvent(ev));
    }

    fn poll(
        &mut self,
        _: &mut std::task::Context<'_>,
        _: &mut impl libp2p_swarm::PollParameters,
    ) -> std::task::Poll<libp2p_swarm::NetworkBehaviourAction<OutboundStreamId, Self::OutEvent>>
    {
        if let Some(ev) = self.pending_events.pop_front() {
            return Poll::Ready(ev);
        } else if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }

        Poll::Pending
    }
}

/// Internal threshold for when to shrink the capacity of empty
/// queues. If the capacity of an empty queue exceeds this threshold,
/// the associated memory is released.
const EMPTY_QUEUE_SHRINK_THRESHOLD: usize = 100;
