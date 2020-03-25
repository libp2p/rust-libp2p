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

mod event;
pub mod peer;

pub use event::{NetworkEvent, IncomingConnectionEvent};
pub use peer::Peer;

use crate::{
    ConnectedPoint,
    Executor,
    Multiaddr,
    PeerId,
    address_translation,
    connection::{
        ConnectionId,
        ConnectionLimit,
        ConnectionHandler,
        ConnectionInfo,
        IntoConnectionHandler,
        IncomingInfo,
        OutgoingInfo,
        ListenersEvent,
        ListenerId,
        ListenersStream,
        PendingConnectionError,
        Substream,
        pool::{Pool, PoolEvent, PoolLimits},
    },
    muxing::StreamMuxer,
    transport::{Transport, TransportError},
};
use fnv::{FnvHashMap};
use futures::{prelude::*, future};
use std::{
    collections::hash_map,
    convert::TryFrom as _,
    error,
    fmt,
    hash::Hash,
    pin::Pin,
    task::{Context, Poll},
};

/// Implementation of `Stream` that handles the nodes.
pub struct Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo = PeerId, TPeerId = PeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
    /// The local peer ID.
    local_peer_id: TPeerId,

    /// Listeners for incoming connections.
    listeners: ListenersStream<TTrans>,

    /// The nodes currently active.
    pool: Pool<TInEvent, TOutEvent, THandler, TTrans::Error,
        <THandler::Handler as ConnectionHandler>::Error, TConnInfo, TPeerId>,

    /// The ongoing dialing attempts.
    ///
    /// The `Network` enforces a single ongoing dialing attempt per peer,
    /// even if multiple (established) connections per peer are allowed.
    /// However, a single dialing attempt operates on a list of addresses
    /// to connect to, which can be extended with new addresses while
    /// the connection attempt is still in progress. Thereby each
    /// dialing attempt is associated with a new connection and hence a new
    /// connection ID.
    ///
    /// > **Note**: `dialing` must be consistent with the pending outgoing
    /// > connections in `pool`. That is, for every entry in `dialing`
    /// > there must exist a pending outgoing connection in `pool` with
    /// > the same connection ID. This is ensured by the implementation of
    /// > `Network` (see `dial_peer_impl` and `on_connection_failed`)
    /// > together with the implementation of `DialingConnection::abort`.
    dialing: FnvHashMap<TPeerId, peer::DialingAttempt>,
}

impl<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> fmt::Debug for
    Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: fmt::Debug + Transport,
    THandler: fmt::Debug + ConnectionHandler,
    TConnInfo: fmt::Debug,
    TPeerId: fmt::Debug + Eq + Hash,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ReachAttempts")
            .field("local_peer_id", &self.local_peer_id)
            .field("listeners", &self.listeners)
            .field("peers", &self.pool)
            .field("dialing", &self.dialing)
            .finish()
    }
}

impl<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> Unpin for
    Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
}

impl<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    TPeerId: Eq + Hash + Clone,
{
    fn disconnect(&mut self, peer: &TPeerId) {
        self.pool.disconnect(peer);
        self.dialing.remove(peer);
    }
}

impl<TTrans, TInEvent, TOutEvent, TMuxer, THandler, TConnInfo, TPeerId>
    Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport + Clone,
    TMuxer: StreamMuxer,
    THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
    THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
    <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    <THandler::Handler as ConnectionHandler>::Error: error::Error + Send + 'static,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone,
{
    /// Creates a new node events stream.
    pub fn new(
        transport: TTrans,
        local_peer_id: TPeerId,
        config: NetworkConfig,
    ) -> Self {
        let pool_local_id = local_peer_id.clone();
        Network {
            local_peer_id,
            listeners: ListenersStream::new(transport),
            pool: Pool::new(pool_local_id, config.executor, config.pool_limits),
            dialing: Default::default(),
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
    ///
    /// Returns `Ok(())` if a listener with this ID was in the list.
    pub fn remove_listener(&mut self, id: ListenerId) -> Result<(), ()> {
        self.listeners.remove_listener(id)
    }

    /// Returns an iterator that produces the list of addresses we are listening on.
    pub fn listen_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.listen_addrs()
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
    pub fn local_peer_id(&self) -> &TPeerId {
        &self.local_peer_id
    }

    /// Dials a multiaddress without expecting a particular remote peer ID.
    ///
    /// The given `handler` will be used to create the
    /// [`Connection`](crate::connection::Connection) upon success and the
    /// connection ID is returned.
    pub fn dial(&mut self, address: &Multiaddr, handler: THandler)
        -> Result<ConnectionId, DialError<TTrans::Error>>
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
        let future = self.transport().clone().dial(address.clone())?
            .map_err(|err| PendingConnectionError::Transport(TransportError::Other(err)));
        let info = OutgoingInfo { address, peer_id: None };
        self.pool.add_outgoing(future, handler, info).map_err(DialError::MaxPending)
    }

    /// Returns information about the state of the `Network`.
    pub fn info(&self) -> NetworkInfo {
        let num_connections_established = self.pool.num_established();
        let num_connections_pending = self.pool.num_pending();
        let num_connections = num_connections_established + num_connections_pending;
        let num_peers = self.pool.num_connected();
        NetworkInfo {
            num_peers,
            num_connections,
            num_connections_established,
            num_connections_pending,
        }
    }

    /// Returns an iterator for information on all pending incoming connections.
    pub fn incoming_info(&self) -> impl Iterator<Item = IncomingInfo<'_>> {
        self.pool.iter_pending_incoming()
    }

    /// Returns the list of addresses we're currently dialing without knowing the `PeerId` of.
    pub fn unknown_dials(&self) -> impl Iterator<Item = &Multiaddr> {
        self.pool.iter_pending_outgoing()
            .filter_map(|info| {
                if info.peer_id.is_none() {
                    Some(info.address)
                } else {
                    None
                }
            })
    }

    /// Notifies the connection handler of _every_ connection of _every_ peer of an event.
    ///
    /// This function is "atomic", in the sense that if `Poll::Pending` is returned then no event
    /// has been sent to any node yet.
    #[must_use]
    pub fn poll_broadcast(&mut self, event: &TInEvent, cx: &mut Context) -> Poll<()>
    where
        TInEvent: Clone
    {
        self.pool.poll_broadcast(event, cx)
    }

    /// Returns a list of all connected peers, i.e. peers to whom the `Network`
    /// has at least one established connection.
    pub fn connected_peers(&self) -> impl Iterator<Item = &TPeerId> {
        self.pool.iter_connected()
    }

    /// Returns a list of all the peers to whom a new outgoing connection
    /// is currently being established.
    pub fn dialing_peers(&self) -> impl Iterator<Item = &TPeerId> {
        self.dialing.keys()
    }

    /// Gets the configured limit on pending incoming connections,
    /// i.e. concurrent incoming connection attempts.
    pub fn incoming_limit(&self) -> Option<usize> {
        self.pool.limits().max_pending_incoming
    }

    /// The total number of established connections in the `Network`.
    pub fn num_connections_established(&self) -> usize {
        self.pool.num_established()
    }

    /// The total number of pending connections in the `Network`.
    pub fn num_connections_pending(&self) -> usize {
        self.pool.num_pending()
    }

    /// Obtains a view of a [`Peer`] with the given ID in the network.
    pub fn peer(&mut self, peer_id: TPeerId)
        -> Peer<'_, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    {
        Peer::new(self, peer_id)
    }

    /// Provides an API similar to `Stream`, except that it cannot error.
    pub fn poll<'a>(&'a mut self, cx: &mut Context) -> Poll<NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>>
    where
        TTrans: Transport<Output = (TConnInfo, TMuxer)>,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
        TTrans::ListenerUpgrade: Send + 'static,
        TMuxer: Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
        THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
        <THandler::Handler as ConnectionHandler>::Error: error::Error + Send + 'static,
        TConnInfo: Clone,
        TPeerId: Send + 'static,
    {
        // Poll the listener(s) for new connections.
        match ListenersStream::poll(Pin::new(&mut self.listeners), cx) {
            Poll::Pending => (),
            Poll::Ready(ListenersEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr
            }) => {
                return Poll::Ready(NetworkEvent::IncomingConnection(
                    IncomingConnectionEvent {
                        listener_id,
                        upgrade,
                        local_addr,
                        send_back_addr,
                        pool: &mut self.pool,
                    }))
            }
            Poll::Ready(ListenersEvent::NewAddress { listener_id, listen_addr }) => {
                return Poll::Ready(NetworkEvent::NewListenerAddress { listener_id, listen_addr })
            }
            Poll::Ready(ListenersEvent::AddressExpired { listener_id, listen_addr }) => {
                return Poll::Ready(NetworkEvent::ExpiredListenerAddress { listener_id, listen_addr })
            }
            Poll::Ready(ListenersEvent::Closed { listener_id, addresses, reason }) => {
                return Poll::Ready(NetworkEvent::ListenerClosed { listener_id, addresses, reason })
            }
            Poll::Ready(ListenersEvent::Error { listener_id, error }) => {
                return Poll::Ready(NetworkEvent::ListenerError { listener_id, error })
            }
        }

        // Poll the known peers.
        let event = match self.pool.poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(PoolEvent::ConnectionEstablished { connection, num_established }) => {
                match self.dialing.entry(connection.peer_id().clone()) {
                    hash_map::Entry::Occupied(e) if e.get().id == connection.id() => {
                        e.remove();
                    },
                    _ => {}
                }

                NetworkEvent::ConnectionEstablished {
                    connection,
                    num_established,
                }
            }
            Poll::Ready(PoolEvent::PendingConnectionError { id, endpoint, error, handler, pool, .. }) => {
                let dialing = &mut self.dialing;
                let (next, event) = on_connection_failed(dialing, id, endpoint, error, handler);
                if let Some(dial) = next {
                    let transport = self.listeners.transport().clone();
                    if let Err(e) = dial_peer_impl(transport, pool, dialing, dial) {
                        log::warn!("Dialing aborted: {:?}", e);
                    }
                }
                event
            }
            Poll::Ready(PoolEvent::ConnectionError { connected, error, num_established, .. }) => {
                NetworkEvent::ConnectionError {
                    connected,
                    error,
                    num_established,
                }
            }
            Poll::Ready(PoolEvent::ConnectionEvent { connection, event }) => {
                NetworkEvent::ConnectionEvent {
                    connection,
                    event
                }
            }
        };

        Poll::Ready(event)
    }

    /// Initiates a connection attempt to a known peer.
    fn dial_peer(&mut self, opts: DialingOpts<TPeerId, THandler>)
        -> Result<ConnectionId, ConnectionLimit>
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
        dial_peer_impl(self.transport().clone(), &mut self.pool, &mut self.dialing, opts)
    }
}

/// Options for a dialing attempt (i.e. repeated connection attempt
/// via a list of address) to a peer.
struct DialingOpts<TPeerId, THandler> {
    peer: TPeerId,
    handler: THandler,
    address: Multiaddr,
    remaining: Vec<Multiaddr>,
}

/// Standalone implementation of `Network::dial_peer` for more granular borrowing.
fn dial_peer_impl<TMuxer, TInEvent, TOutEvent, THandler, TTrans, TConnInfo, TPeerId>(
    transport: TTrans,
    pool: &mut Pool<TInEvent, TOutEvent, THandler, TTrans::Error,
        <THandler::Handler as ConnectionHandler>::Error, TConnInfo, TPeerId>,
    dialing: &mut FnvHashMap<TPeerId, peer::DialingAttempt>,
    opts: DialingOpts<TPeerId, THandler>
) -> Result<ConnectionId, ConnectionLimit>
where
    THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
    <THandler::Handler as ConnectionHandler>::Error: error::Error + Send + 'static,
    <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
    THandler::Handler: ConnectionHandler<
        Substream = Substream<TMuxer>,
        InEvent = TInEvent,
        OutEvent = TOutEvent,
    > + Send + 'static,
    TTrans: Transport<Output = (TConnInfo, TMuxer)>,
    TTrans::Dial: Send + 'static,
    TTrans::Error: error::Error + Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send + 'static,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    TPeerId: Eq + Hash + Send + Clone + 'static,
    TConnInfo: ConnectionInfo<PeerId = TPeerId> + Send + 'static,
{
    let result = match transport.dial(opts.address.clone()) {
        Ok(fut) => {
            let fut = fut.map_err(|e| PendingConnectionError::Transport(TransportError::Other(e)));
            let info = OutgoingInfo { address: &opts.address, peer_id: Some(&opts.peer) };
            pool.add_outgoing(fut, opts.handler, info)
        },
        Err(err) => {
            let fut = future::err(PendingConnectionError::Transport(err));
            let info = OutgoingInfo { address: &opts.address, peer_id: Some(&opts.peer) };
            pool.add_outgoing(fut, opts.handler, info)
        },
    };

    if let Ok(id) = &result {
        let former = dialing.insert(opts.peer,
            peer::DialingAttempt {
                id: *id,
                current: opts.address,
                next: opts.remaining,
            },
        );
        debug_assert!(former.is_none());
    }

    result
}

/// Callback for handling a failed connection attempt, returning an
/// event to emit from the `Network`.
///
/// If the failed connection attempt was a dialing attempt and there
/// are more addresses to try, new `DialingOpts` are returned.
fn on_connection_failed<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>(
    dialing: &mut FnvHashMap<TPeerId, peer::DialingAttempt>,
    id: ConnectionId,
    endpoint: ConnectedPoint,
    error: PendingConnectionError<TTrans::Error>,
    handler: THandler,
) -> (Option<DialingOpts<TPeerId, THandler>>, NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>)
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TConnInfo: ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone,
{
    // Check if the failed connection is associated with a dialing attempt.
    // TODO: could be more optimal than iterating over everything
    let dialing_peer = dialing.iter() // (1)
        .find(|(_, a)| a.id == id)
        .map(|(p, _)| p.clone());

    if let Some(peer_id) = dialing_peer {
        // A pending outgoing connection to a known peer failed.
        let mut attempt = dialing.remove(&peer_id).expect("by (1)");

        let num_remain = u32::try_from(attempt.next.len()).unwrap();
        let failed_addr = attempt.current.clone();

        let opts =
            if num_remain > 0 {
                let next_attempt = attempt.next.remove(0);
                let opts = DialingOpts {
                    peer: peer_id.clone(),
                    handler,
                    address: next_attempt,
                    remaining: attempt.next
                };
                Some(opts)
            } else {
                None
            };

        (opts, NetworkEvent::DialError {
            attempts_remaining: num_remain,
            peer_id,
            multiaddr: failed_addr,
            error,
        })
    } else {
        // A pending incoming connection or outgoing connection to an unknown peer failed.
        match endpoint {
            ConnectedPoint::Dialer { address } =>
                (None, NetworkEvent::UnknownPeerDialError {
                    multiaddr: address,
                    error,
                    handler,
                }),
            ConnectedPoint::Listener { local_addr, send_back_addr } =>
                (None, NetworkEvent::IncomingConnectionError {
                    local_addr,
                    send_back_addr,
                    error
                })
        }
    }
}

/// Information about the network obtained by [`Network::info()`].
#[derive(Clone, Debug)]
pub struct NetworkInfo {
    pub num_peers: usize,
    pub num_connections: usize,
    pub num_connections_pending: usize,
    pub num_connections_established: usize,
}

/// The possible errors of [`Network::dial`].
#[derive(Debug)]
pub enum DialError<T> {
    /// The configured limit of pending outgoing connections has been reached.
    MaxPending(ConnectionLimit),
    /// A transport error occurred when creating the connection.
    Transport(TransportError<T>),
}

impl<T> fmt::Display for DialError<T>
where T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DialError::MaxPending(limit) => write!(f, "Dial error (pending limit): {}", limit.current),
            DialError::Transport(err) => write!(f, "Dial error (transport): {}", err),
        }
    }
}

impl<T> std::error::Error for DialError<T>
where T: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DialError::MaxPending(_) => None,
            DialError::Transport(e) => Some(e),
        }
    }
}

impl<T> From<TransportError<T>> for DialError<T> {
    fn from(e: TransportError<T>) -> DialError<T> {
        DialError::Transport(e)
    }
}

/// The (optional) configuration for a [`Network`].
///
/// The default configuration specifies no dedicated task executor
/// and no connection limits.
#[derive(Default)]
pub struct NetworkConfig {
    executor: Option<Box<dyn Executor + Send>>,
    pool_limits: PoolLimits,
}

impl NetworkConfig {
    pub fn set_executor(&mut self, e: Box<dyn Executor + Send>) -> &mut Self {
        self.executor = Some(e);
        self
    }

    pub fn executor(&self) -> Option<&Box<dyn Executor + Send>> {
        self.executor.as_ref()
    }

    pub fn set_pending_incoming_limit(&mut self, n: usize) -> &mut Self {
        self.pool_limits.max_pending_incoming = Some(n);
        self
    }

    pub fn set_pending_outgoing_limit(&mut self, n: usize) -> &mut Self {
        self.pool_limits.max_pending_outgoing = Some(n);
        self
    }

    pub fn set_established_per_peer_limit(&mut self, n: usize) -> &mut Self {
        self.pool_limits.max_established_per_peer = Some(n);
        self
    }
}

