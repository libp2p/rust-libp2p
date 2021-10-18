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

pub use crate::connection::{ConnectionCounters, ConnectionLimits};
pub use event::{IncomingConnection, NetworkEvent};
pub use peer::Peer;

use crate::{
    connection::{
        handler::{THandlerInEvent, THandlerOutEvent},
        pool::{Pool, PoolConfig, PoolEvent},
        ConnectionHandler, ConnectionId, ConnectionLimit, IncomingInfo, IntoConnectionHandler,
        ListenerId, ListenersEvent, ListenersStream, PendingPoint, Substream,
    },
    muxing::StreamMuxer,
    transport::{Transport, TransportError},
    Executor, Multiaddr, PeerId,
};
use std::{
    convert::TryFrom as _,
    error, fmt,
    num::{NonZeroU8, NonZeroUsize},
    pin::Pin,
    task::{Context, Poll},
};

/// Implementation of `Stream` that handles the nodes.
pub struct Network<TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    /// The local peer ID.
    local_peer_id: PeerId,

    /// Listeners for incoming connections.
    listeners: ListenersStream<TTrans>,

    /// The nodes currently active.
    pool: Pool<THandler, TTrans>,
}

impl<TTrans, THandler> fmt::Debug for Network<TTrans, THandler>
where
    TTrans: fmt::Debug + Transport,
    THandler: fmt::Debug + ConnectionHandler,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ReachAttempts")
            .field("local_peer_id", &self.local_peer_id)
            .field("listeners", &self.listeners)
            .field("peers", &self.pool)
            .finish()
    }
}

impl<TTrans, THandler> Unpin for Network<TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
}

impl<TTrans, THandler> Network<TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    /// Checks whether the network has an established connection to a peer.
    pub fn is_connected(&self, peer: &PeerId) -> bool {
        self.pool.is_connected(peer)
    }

    fn dialing_attempts(&self, peer: PeerId) -> impl Iterator<Item = &ConnectionId> {
        self.pool
            .iter_pending_info()
            .filter(move |(_, endpoint, peer_id)| {
                matches!(endpoint, PendingPoint::Dialer) && peer_id.as_ref() == Some(&peer)
            })
            .map(|(connection_id, _, _)| connection_id)
    }

    /// Checks whether the network has an ongoing dialing attempt to a peer.
    pub fn is_dialing(&self, peer: &PeerId) -> bool {
        self.dialing_attempts(*peer).next().is_some()
    }

    fn disconnect(&mut self, peer: &PeerId) {
        self.pool.disconnect(peer);
    }
}

impl<TTrans, THandler> Network<TTrans, THandler>
where
    TTrans: Transport + Clone + 'static,
    <TTrans as Transport>::Error: Send + 'static,
    THandler: IntoConnectionHandler + Send + 'static,
{
    /// Creates a new node events stream.
    pub fn new(transport: TTrans, local_peer_id: PeerId, config: NetworkConfig) -> Self {
        Network {
            local_peer_id,
            listeners: ListenersStream::new(transport),
            pool: Pool::new(local_peer_id, config.pool_config, config.limits),
        }
    }

    /// Returns the transport passed when building this object.
    pub fn transport(&self) -> &TTrans {
        self.listeners.transport()
    }

    /// Start listening on the given multiaddress.
    pub fn listen_on(
        &mut self,
        addr: Multiaddr,
    ) -> Result<ListenerId, TransportError<TTrans::Error>> {
        self.listeners.listen_on(addr)
    }

    /// Remove a previously added listener.
    ///
    /// Returns `true` if there was a listener with this ID, `false`
    /// otherwise.
    pub fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.listeners.remove_listener(id)
    }

    /// Returns an iterator that produces the list of addresses we are listening on.
    pub fn listen_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.listen_addrs()
    }

    /// Maps the given `observed_addr`, representing an address of the local
    /// node observed by a remote peer, onto the locally known listen addresses
    /// to yield one or more addresses of the local node that may be publicly
    /// reachable.
    ///
    /// I.e. this method incorporates the view of other peers into the listen
    /// addresses seen by the local node to account for possible IP and port
    /// mappings performed by intermediate network devices in an effort to
    /// obtain addresses for the local peer that are also reachable for peers
    /// other than the peer who reported the `observed_addr`.
    ///
    /// The translation is transport-specific. See [`Transport::address_translation`].
    pub fn address_translation<'a>(&'a self, observed_addr: &'a Multiaddr) -> Vec<Multiaddr>
    where
        THandler: 'a,
    {
        let transport = self.listeners.transport();
        let mut addrs: Vec<_> = self
            .listen_addrs()
            .filter_map(move |server| transport.address_translation(server, observed_addr))
            .collect();

        // remove duplicates
        addrs.sort_unstable();
        addrs.dedup();

        addrs
    }

    /// Returns the peer id of the local node.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Dials a [`Multiaddr`] that may or may not encapsulate a
    /// specific expected remote peer ID.
    ///
    /// The given `handler` will be used to create the
    /// [`Connection`](crate::connection::Connection) upon success and the
    /// connection ID is returned.
    pub fn dial(
        &mut self,
        address: &Multiaddr,
        handler: THandler,
    ) -> Result<ConnectionId, DialError<THandler>>
    where
        TTrans: Transport + Send,
        TTrans::Output: Send + 'static,
        TTrans::Dial: Send + 'static,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
    {
        // If the address ultimately encapsulates an expected peer ID, dial that peer
        // such that any mismatch is detected. We do not "pop off" the `P2p` protocol
        // from the address, because it may be used by the `Transport`, i.e. `P2p`
        // is a protocol component that can influence any transport, like `libp2p-dns`.
        if let Some(multiaddr::Protocol::P2p(ma)) = address.iter().last() {
            if let Ok(peer) = PeerId::try_from(ma) {
                return self.dial_peer(DialingOpts {
                    peer,
                    addresses: std::iter::once(address.clone()),
                    handler,
                });
            }
        }

        self.pool.add_outgoing(
            self.transport().clone(),
            std::iter::once(address.clone()),
            None,
            handler,
        )
    }

    /// Initiates a connection attempt to a known peer.
    fn dial_peer<I>(
        &mut self,
        opts: DialingOpts<THandler, I>,
    ) -> Result<ConnectionId, DialError<THandler>>
    where
        I: Iterator<Item = Multiaddr> + Send + 'static,
        TTrans: Transport + Send,
        TTrans::Output: Send + 'static,
        TTrans::Dial: Send + 'static,
        TTrans::Error: Send + 'static,
    {
        let id = self.pool.add_outgoing(
            self.transport().clone(),
            opts.addresses,
            Some(opts.peer),
            opts.handler,
        )?;

        Ok(id)
    }

    /// Returns information about the state of the `Network`.
    pub fn info(&self) -> NetworkInfo {
        let num_peers = self.pool.num_peers();
        let connection_counters = self.pool.counters().clone();
        NetworkInfo {
            num_peers,
            connection_counters,
        }
    }

    /// Returns an iterator for information on all pending incoming connections.
    pub fn incoming_info(&self) -> impl Iterator<Item = IncomingInfo<'_>> {
        self.pool.iter_pending_incoming()
    }

    /// Returns a list of all connected peers, i.e. peers to whom the `Network`
    /// has at least one established connection.
    pub fn connected_peers(&self) -> impl Iterator<Item = &PeerId> {
        self.pool.iter_connected()
    }

    /// Checks whether the network has neither an ongoing dialing attempt,
    /// nor an established connection to a peer.
    pub fn is_disconnected(&self, peer: &PeerId) -> bool {
        !self.is_connected(peer) && !self.is_dialing(peer)
    }

    /// Returns a list of all the peers to whom a new outgoing connection
    /// is currently being established.
    pub fn dialing_peers(&self) -> impl Iterator<Item = &PeerId> {
        self.pool
            .iter_pending_info()
            .filter(|(_, endpoint, _)| matches!(endpoint, PendingPoint::Dialer))
            .filter_map(|(_, _, peer)| peer.as_ref())
    }

    /// Obtains a view of a [`Peer`] with the given ID in the network.
    pub fn peer(&mut self, peer_id: PeerId) -> Peer<'_, TTrans, THandler> {
        Peer::new(self, peer_id)
    }

    /// Accepts a pending incoming connection obtained via [`NetworkEvent::IncomingConnection`],
    /// adding it to the `Network`s connection pool subject to the configured limits.
    ///
    /// Once the connection is established and all transport protocol upgrades
    /// completed, the connection is associated with the provided `handler`.
    pub fn accept(
        &mut self,
        IncomingConnection {
            upgrade,
            local_addr,
            send_back_addr,
        }: IncomingConnection<TTrans::ListenerUpgrade>,
        handler: THandler,
    ) -> Result<ConnectionId, (ConnectionLimit, THandler)>
    where
        TTrans: Transport,
        TTrans::Output: Send + 'static,
        TTrans::Error: Send + 'static,
        TTrans::ListenerUpgrade: Send + 'static,
    {
        self.pool.add_incoming(
            upgrade,
            handler,
            IncomingInfo {
                local_addr: &local_addr,
                send_back_addr: &send_back_addr,
            },
        )
    }

    /// Provides an API similar to `Stream`, except that it does not terminate.
    pub fn poll<'a, TMuxer>(
        &'a mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        NetworkEvent<'a, TTrans, THandlerInEvent<THandler>, THandlerOutEvent<THandler>, THandler>,
    >
    where
        TTrans: Transport<Output = (PeerId, TMuxer)>,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
        TTrans::ListenerUpgrade: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::Error: std::fmt::Debug,
        TMuxer::OutboundSubstream: Send,
        THandler: IntoConnectionHandler + Send + 'static,
        <THandler::Handler as ConnectionHandler>::Error: error::Error + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send,
        <THandler::Handler as ConnectionHandler>::Error: error::Error + Send,
        THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>> + Send,
    {
        // Poll the listener(s) for new connections.
        match ListenersStream::poll(Pin::new(&mut self.listeners), cx) {
            Poll::Pending => (),
            Poll::Ready(ListenersEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr,
            }) => {
                return Poll::Ready(NetworkEvent::IncomingConnection {
                    listener_id,
                    connection: IncomingConnection {
                        upgrade,
                        local_addr,
                        send_back_addr,
                    },
                })
            }
            Poll::Ready(ListenersEvent::NewAddress {
                listener_id,
                listen_addr,
            }) => {
                return Poll::Ready(NetworkEvent::NewListenerAddress {
                    listener_id,
                    listen_addr,
                })
            }
            Poll::Ready(ListenersEvent::AddressExpired {
                listener_id,
                listen_addr,
            }) => {
                return Poll::Ready(NetworkEvent::ExpiredListenerAddress {
                    listener_id,
                    listen_addr,
                })
            }
            Poll::Ready(ListenersEvent::Closed {
                listener_id,
                addresses,
                reason,
            }) => {
                return Poll::Ready(NetworkEvent::ListenerClosed {
                    listener_id,
                    addresses,
                    reason,
                })
            }
            Poll::Ready(ListenersEvent::Error { listener_id, error }) => {
                return Poll::Ready(NetworkEvent::ListenerError { listener_id, error })
            }
        }

        // Poll the known peers.
        let event = match self.pool.poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(PoolEvent::ConnectionEstablished {
                connection,
                num_established,
                concurrent_dial_errors,
            }) => NetworkEvent::ConnectionEstablished {
                connection,
                num_established,
                concurrent_dial_errors,
            },
            Poll::Ready(PoolEvent::PendingOutboundConnectionError {
                id: _,
                error,
                handler,
                peer,
            }) => {
                if let Some(peer) = peer {
                    NetworkEvent::DialError {
                        handler,
                        peer_id: peer,
                        error,
                    }
                } else {
                    NetworkEvent::UnknownPeerDialError { error, handler }
                }
            }
            Poll::Ready(PoolEvent::PendingInboundConnectionError {
                id: _,
                send_back_addr,
                local_addr,
                error,
                handler,
            }) => NetworkEvent::IncomingConnectionError {
                error,
                handler,
                send_back_addr,
                local_addr,
            },
            Poll::Ready(PoolEvent::ConnectionClosed {
                id,
                connected,
                error,
                num_established,
                handler,
                ..
            }) => NetworkEvent::ConnectionClosed {
                id,
                connected,
                num_established,
                error,
                handler,
            },
            Poll::Ready(PoolEvent::ConnectionEvent { connection, event }) => {
                NetworkEvent::ConnectionEvent { connection, event }
            }
            Poll::Ready(PoolEvent::AddressChange {
                connection,
                new_endpoint,
                old_endpoint,
            }) => NetworkEvent::AddressChange {
                connection,
                new_endpoint,
                old_endpoint,
            },
        };

        Poll::Ready(event)
    }
}

/// Options for a dialing attempt (i.e. repeated connection attempt
/// via a list of address) to a peer.
struct DialingOpts<THandler, I> {
    peer: PeerId,
    handler: THandler,
    addresses: I,
}

/// Information about the network obtained by [`Network::info()`].
#[derive(Clone, Debug)]
pub struct NetworkInfo {
    /// The total number of connected peers.
    num_peers: usize,
    /// Counters of ongoing network connections.
    connection_counters: ConnectionCounters,
}

impl NetworkInfo {
    /// The number of connected peers, i.e. peers with whom at least
    /// one established connection exists.
    pub fn num_peers(&self) -> usize {
        self.num_peers
    }

    /// Gets counters for ongoing network connections.
    pub fn connection_counters(&self) -> &ConnectionCounters {
        &self.connection_counters
    }
}

/// The (optional) configuration for a [`Network`].
///
/// The default configuration specifies no dedicated task executor, no
/// connection limits, a connection event buffer size of 32, and a
/// `notify_handler` buffer size of 8.
#[derive(Default)]
pub struct NetworkConfig {
    /// Connection [`Pool`] configuration.
    pool_config: PoolConfig,
    /// The effective connection limits.
    limits: ConnectionLimits,
}

impl NetworkConfig {
    /// Configures the executor to use for spawning connection background tasks.
    pub fn with_executor(mut self, e: Box<dyn Executor + Send>) -> Self {
        self.pool_config.executor = Some(e);
        self
    }

    /// Configures the executor to use for spawning connection background tasks,
    /// only if no executor has already been configured.
    pub fn or_else_with_executor<F>(mut self, f: F) -> Self
    where
        F: FnOnce() -> Option<Box<dyn Executor + Send>>,
    {
        self.pool_config.executor = self.pool_config.executor.or_else(f);
        self
    }

    /// Sets the maximum number of events sent to a connection's background task
    /// that may be buffered, if the task cannot keep up with their consumption and
    /// delivery to the connection handler.
    ///
    /// When the buffer for a particular connection is full, `notify_handler` will no
    /// longer be able to deliver events to the associated `ConnectionHandler`,
    /// thus exerting back-pressure on the connection and peer API.
    pub fn with_notify_handler_buffer_size(mut self, n: NonZeroUsize) -> Self {
        self.pool_config.task_command_buffer_size = n.get() - 1;
        self
    }

    /// Sets the maximum number of buffered connection events (beyond a guaranteed
    /// buffer of 1 event per connection).
    ///
    /// When the buffer is full, the background tasks of all connections will stall.
    /// In this way, the consumers of network events exert back-pressure on
    /// the network connection I/O.
    pub fn with_connection_event_buffer_size(mut self, n: usize) -> Self {
        self.pool_config.task_event_buffer_size = n;
        self
    }

    /// Number of addresses concurrently dialed for a single outbound connection attempt.
    pub fn with_dial_concurrency_factor(mut self, factor: NonZeroU8) -> Self {
        self.pool_config.dial_concurrency_factor = factor;
        self
    }

    /// Sets the connection limits to enforce.
    pub fn with_connection_limits(mut self, limits: ConnectionLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// Possible (synchronous) errors when dialing a peer.
#[derive(Clone)]
pub enum DialError<THandler> {
    /// The dialing attempt is rejected because of a connection limit.
    ConnectionLimit {
        limit: ConnectionLimit,
        handler: THandler,
    },
    /// The dialing attempt is rejected because the peer being dialed is the local peer.
    LocalPeerId { handler: THandler },
}

impl<THandler> fmt::Debug for DialError<THandler> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            DialError::ConnectionLimit { limit, handler: _ } => f
                .debug_struct("DialError::ConnectionLimit")
                .field("limit", limit)
                .finish(),
            DialError::LocalPeerId { handler: _ } => {
                f.debug_struct("DialError::LocalPeerId").finish()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::Future;

    struct Dummy;

    impl Executor for Dummy {
        fn exec(&self, _: Pin<Box<dyn Future<Output = ()> + Send>>) {}
    }

    #[test]
    fn set_executor() {
        NetworkConfig::default()
            .with_executor(Box::new(Dummy))
            .with_executor(Box::new(|f| {
                async_std::task::spawn(f);
            }));
    }
}
