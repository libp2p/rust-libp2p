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
        manager::ManagerConfig,
        pool::{Pool, PoolEvent, PoolLimits},
    },
    muxing::StreamMuxer,
    transport::{Transport, TransportError},
};
use fnv::{FnvHashMap};
use futures::{prelude::*, future};
use smallvec::SmallVec;
use std::{
    collections::hash_map,
    convert::TryFrom as _,
    error,
    fmt,
    hash::Hash,
    num::NonZeroUsize,
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
    /// There may be multiple ongoing dialing attempts to the same peer.
    /// Each dialing attempt is associated with a new connection and hence
    /// a new connection ID.
    ///
    /// > **Note**: `dialing` must be consistent with the pending outgoing
    /// > connections in `pool`. That is, for every entry in `dialing`
    /// > there must exist a pending outgoing connection in `pool` with
    /// > the same connection ID. This is ensured by the implementation of
    /// > `Network` (see `dial_peer_impl` and `on_connection_failed`)
    /// > together with the implementation of `DialingAttempt::abort`.
    dialing: FnvHashMap<TPeerId, SmallVec<[peer::DialingState; 10]>>,
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
            pool: Pool::new(pool_local_id, config.manager_config, config.pool_limits),
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
        -> Result<ConnectionId, ConnectionLimit>
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
        let info = OutgoingInfo { address, peer_id: None };
        match self.transport().clone().dial(address.clone()) {
            Ok(f) => {
                let f = f.map_err(|err| PendingConnectionError::Transport(TransportError::Other(err)));
                self.pool.add_outgoing(f, handler, info)
            }
            Err(err) => {
                let f = future::err(PendingConnectionError::Transport(err));
                self.pool.add_outgoing(f, handler, info)
            }
        }
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

    /// Returns a list of all connected peers, i.e. peers to whom the `Network`
    /// has at least one established connection.
    pub fn connected_peers(&self) -> impl Iterator<Item = &TPeerId> {
        self.pool.iter_connected()
    }

    /// Checks whether the network has an established connection to a peer.
    pub fn is_connected(&self, peer: &TPeerId) -> bool {
        self.pool.is_connected(peer)
    }

    /// Checks whether the network has an ongoing dialing attempt to a peer.
    pub fn is_dialing(&self, peer: &TPeerId) -> bool {
        self.dialing.contains_key(peer)
    }

    /// Checks whether the network has neither an ongoing dialing attempt,
    /// nor an established connection to a peer.
    pub fn is_disconnected(&self, peer: &TPeerId) -> bool {
        !self.is_connected(peer) && !self.is_dialing(peer)
    }

    /// Returns a list of all the peers to whom a new outgoing connection
    /// is currently being established.
    pub fn dialing_peers(&self) -> impl Iterator<Item = &TPeerId> {
        self.dialing.keys()
    }

    /// Gets the configured limit on pending incoming connections,
    /// i.e. concurrent incoming connection attempts.
    pub fn incoming_limit(&self) -> Option<usize> {
        self.pool.limits().max_incoming
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
                    hash_map::Entry::Occupied(mut e) => {
                        e.get_mut().retain(|s| s.current.0 != connection.id());
                        if e.get().is_empty() {
                            e.remove();
                        }
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
            Poll::Ready(PoolEvent::ConnectionError { id, connected, error, num_established, .. }) => {
                NetworkEvent::ConnectionError {
                    id,
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
    dialing: &mut FnvHashMap<TPeerId, SmallVec<[peer::DialingState; 10]>>,
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
        dialing.entry(opts.peer).or_default().push(
            peer::DialingState {
                current: (*id, opts.address),
                remaining: opts.remaining,
            },
        );
    }

    result
}

/// Callback for handling a failed connection attempt, returning an
/// event to emit from the `Network`.
///
/// If the failed connection attempt was a dialing attempt and there
/// are more addresses to try, new `DialingOpts` are returned.
fn on_connection_failed<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>(
    dialing: &mut FnvHashMap<TPeerId, SmallVec<[peer::DialingState; 10]>>,
    id: ConnectionId,
    endpoint: ConnectedPoint,
    error: PendingConnectionError<TTrans::Error>,
    handler: Option<THandler>,
) -> (Option<DialingOpts<TPeerId, THandler>>, NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>)
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TConnInfo: ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone,
{
    // Check if the failed connection is associated with a dialing attempt.
    let dialing_failed = dialing.iter_mut()
        .find_map(|(peer, attempts)| {
            if let Some(pos) = attempts.iter().position(|s| s.current.0 == id) {
                let attempt = attempts.remove(pos);
                let last = attempts.is_empty();
                Some((peer.clone(), attempt, last))
            } else {
                None
            }
        });

    if let Some((peer_id, mut attempt, last)) = dialing_failed {
        if last {
            dialing.remove(&peer_id);
        }

        let num_remain = u32::try_from(attempt.remaining.len()).unwrap();
        let failed_addr = attempt.current.1.clone();

        let (opts, attempts_remaining) =
            if num_remain > 0 {
                if let Some(handler) = handler {
                    let next_attempt = attempt.remaining.remove(0);
                    let opts = DialingOpts {
                        peer: peer_id.clone(),
                        handler,
                        address: next_attempt,
                        remaining: attempt.remaining
                    };
                    (Some(opts), num_remain)
                } else {
                    // The error is "fatal" for the dialing attempt, since
                    // the handler was already consumed. All potential
                    // remaining connection attempts are thus void.
                    (None, 0)
                }
            } else {
                (None, 0)
            };

        (opts, NetworkEvent::DialError {
            attempts_remaining,
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
    /// The total number of connected peers.
    pub num_peers: usize,
    /// The total number of connections, both established and pending.
    pub num_connections: usize,
    /// The total number of pending connections, both incoming and outgoing.
    pub num_connections_pending: usize,
    /// The total number of established connections.
    pub num_connections_established: usize,
}

/// The (optional) configuration for a [`Network`].
///
/// The default configuration specifies no dedicated task executor, no
/// connection limits, a connection event buffer size of 32, and a
/// `notify_handler` buffer size of 8.
#[derive(Default)]
pub struct NetworkConfig {
    /// Note that the `ManagerConfig`s task command buffer always provides
    /// one "free" slot per task. Thus the given total `notify_handler_buffer_size`
    /// exposed for configuration on the `Network` is reduced by one.
    manager_config: ManagerConfig,
    pool_limits: PoolLimits,
}

impl NetworkConfig {
    pub fn set_executor(&mut self, e: Box<dyn Executor + Send>) -> &mut Self {
        self.manager_config.executor = Some(e);
        self
    }

    /// Shortcut for calling `executor` with an object that calls the given closure.
    pub fn set_executor_fn(mut self, f: impl Fn(Pin<Box<dyn Future<Output = ()> + Send>>) + Send + 'static) -> Self {
        struct SpawnImpl<F>(F);
        impl<F: Fn(Pin<Box<dyn Future<Output = ()> + Send>>)> Executor for SpawnImpl<F> {
            fn exec(&self, f: Pin<Box<dyn Future<Output = ()> + Send>>) {
                (self.0)(f)
            }
        }
        self.set_executor(Box::new(SpawnImpl(f)));
        self
    }

    pub fn executor(&self) -> Option<&Box<dyn Executor + Send>> {
        self.manager_config.executor.as_ref()
    }

    /// Sets the maximum number of events sent to a connection's background task
    /// that may be buffered, if the task cannot keep up with their consumption and
    /// delivery to the connection handler.
    ///
    /// When the buffer for a particular connection is full, `notify_handler` will no
    /// longer be able to deliver events to the associated `ConnectionHandler`,
    /// thus exerting back-pressure on the connection and peer API.
    pub fn set_notify_handler_buffer_size(&mut self, n: NonZeroUsize) -> &mut Self {
        self.manager_config.task_command_buffer_size = n.get() - 1;
        self
    }

    /// Sets the maximum number of buffered connection events (beyond a guaranteed
    /// buffer of 1 event per connection).
    ///
    /// When the buffer is full, the background tasks of all connections will stall.
    /// In this way, the consumers of network events exert back-pressure on
    /// the network connection I/O.
    pub fn set_connection_event_buffer_size(&mut self, n: usize) -> &mut Self {
        self.manager_config.task_event_buffer_size = n;
        self
    }

    pub fn set_incoming_limit(&mut self, n: usize) -> &mut Self {
        self.pool_limits.max_incoming = Some(n);
        self
    }

    pub fn set_outgoing_limit(&mut self, n: usize) -> &mut Self {
        self.pool_limits.max_outgoing = Some(n);
        self
    }

    pub fn set_established_per_peer_limit(&mut self, n: usize) -> &mut Self {
        self.pool_limits.max_established_per_peer = Some(n);
        self
    }

    pub fn set_outgoing_per_peer_limit(&mut self, n: usize) -> &mut Self {
        self.pool_limits.max_outgoing_per_peer = Some(n);
        self
    }
}
