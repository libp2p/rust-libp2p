// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! High-level network manager.
//!
//! A [`Swarm`] contains the state of the network as a whole. The entire
//! behaviour of a libp2p network can be controlled through the `Swarm`.
//! The `Swarm` struct contains all active and pending connections to
//! remotes and manages the state of all the substreams that have been
//! opened, and all the upgrades that were built upon these substreams.
//!
//! # Initializing a Swarm
//!
//! Creating a `Swarm` requires three things:
//!
//!  1. A network identity of the local node in form of a [`PeerId`].
//!  2. An implementation of the [`Transport`] trait. This is the type that
//!     will be used in order to reach nodes on the network based on their
//!     address. See the `transport` module for more information.
//!  3. An implementation of the [`NetworkBehaviour`] trait. This is a state
//!     machine that defines how the swarm should behave once it is connected
//!     to a node.
//!
//! # Network Behaviour
//!
//! The [`NetworkBehaviour`] trait is implemented on types that indicate to
//! the swarm how it should behave. This includes which protocols are supported
//! and which nodes to try to connect to. It is the `NetworkBehaviour` that
//! controls what happens on the network. Multiple types that implement
//! `NetworkBehaviour` can be composed into a single behaviour.
//!
//! # Protocols Handler
//!
//! The [`ConnectionHandler`] trait defines how each active connection to a
//! remote should behave: how to handle incoming substreams, which protocols
//! are supported, when to open a new outbound substream, etc.
//!

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod connection;
mod registry;
#[cfg(test)]
mod test;
mod upgrade;

pub mod behaviour;
pub mod dial_opts;
pub mod dummy;
mod executor;
pub mod handler;
pub mod keep_alive;

/// Bundles all symbols required for the [`libp2p_swarm_derive::NetworkBehaviour`] macro.
#[doc(hidden)]
pub mod derive_prelude {
    pub use crate::behaviour::AddressChange;
    pub use crate::behaviour::ConnectionClosed;
    pub use crate::behaviour::ConnectionEstablished;
    pub use crate::behaviour::DialFailure;
    pub use crate::behaviour::ExpiredExternalAddr;
    pub use crate::behaviour::ExpiredListenAddr;
    pub use crate::behaviour::FromSwarm;
    pub use crate::behaviour::ListenFailure;
    pub use crate::behaviour::ListenerClosed;
    pub use crate::behaviour::ListenerError;
    pub use crate::behaviour::NewExternalAddr;
    pub use crate::behaviour::NewListenAddr;
    pub use crate::behaviour::NewListener;
    pub use crate::connection::ConnectionId;
    pub use crate::ConnectionHandler;
    pub use crate::DialError;
    pub use crate::IntoConnectionHandler;
    pub use crate::IntoConnectionHandlerSelect;
    pub use crate::NetworkBehaviour;
    pub use crate::NetworkBehaviourAction;
    pub use crate::PollParameters;
    pub use futures::prelude as futures;
    pub use libp2p_core::either::EitherOutput;
    pub use libp2p_core::transport::ListenerId;
    pub use libp2p_core::ConnectedPoint;
    pub use libp2p_core::Multiaddr;
    pub use libp2p_core::PeerId;
}

pub use behaviour::{
    AddressChange, CloseConnection, ConnectionClosed, DialFailure, ExpiredExternalAddr,
    ExpiredListenAddr, ExternalAddresses, FromSwarm, ListenAddresses, ListenFailure,
    ListenerClosed, ListenerError, NetworkBehaviour, NetworkBehaviourAction, NewExternalAddr,
    NewListenAddr, NotifyHandler, PollParameters,
};
pub use connection::pool::{ConnectionCounters, ConnectionLimits};
pub use connection::{
    ConnectionError, ConnectionId, ConnectionLimit, PendingConnectionError,
    PendingInboundConnectionError, PendingOutboundConnectionError,
};
pub use executor::Executor;
pub use handler::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerSelect, ConnectionHandlerUpgrErr,
    IntoConnectionHandler, IntoConnectionHandlerSelect, KeepAlive, OneShotHandler,
    OneShotHandlerConfig, SubstreamProtocol,
};
#[cfg(feature = "macros")]
pub use libp2p_swarm_derive::NetworkBehaviour;
pub use registry::{AddAddressResult, AddressRecord, AddressScore};

use connection::pool::{EstablishedConnection, Pool, PoolConfig, PoolEvent};
use connection::IncomingInfo;
use dial_opts::{DialOpts, PeerCondition};
use futures::{executor::ThreadPoolBuilder, prelude::*, stream::FusedStream};
use libp2p_core::muxing::SubstreamBox;
use libp2p_core::{
    connection::ConnectedPoint,
    multiaddr::Protocol,
    multihash::Multihash,
    muxing::StreamMuxerBox,
    transport::{self, ListenerId, TransportError, TransportEvent},
    upgrade::ProtocolName,
    Endpoint, Multiaddr, Negotiated, PeerId, Transport,
};
use registry::{AddressIntoIter, Addresses};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};
use std::num::{NonZeroU32, NonZeroU8, NonZeroUsize};
use std::{
    convert::TryFrom,
    error, fmt, io,
    pin::Pin,
    task::{Context, Poll},
};
use upgrade::UpgradeInfoSend as _;

/// Substream for which a protocol has been chosen.
///
/// Implements the [`AsyncRead`](futures::io::AsyncRead) and
/// [`AsyncWrite`](futures::io::AsyncWrite) traits.
pub type NegotiatedSubstream = Negotiated<SubstreamBox>;

/// Event generated by the [`NetworkBehaviour`] that the swarm will report back.
type TBehaviourOutEvent<TBehaviour> = <TBehaviour as NetworkBehaviour>::OutEvent;

/// [`ConnectionHandler`] of the [`NetworkBehaviour`] for all the protocols the [`NetworkBehaviour`]
/// supports.
type THandler<TBehaviour> = <TBehaviour as NetworkBehaviour>::ConnectionHandler;

/// Custom event that can be received by the [`ConnectionHandler`] of the
/// [`NetworkBehaviour`].
type THandlerInEvent<TBehaviour> =
    <<THandler<TBehaviour> as IntoConnectionHandler>::Handler as ConnectionHandler>::InEvent;

/// Custom event that can be produced by the [`ConnectionHandler`] of the [`NetworkBehaviour`].
type THandlerOutEvent<TBehaviour> =
    <<THandler<TBehaviour> as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent;

/// Custom error that can be produced by the [`ConnectionHandler`] of the [`NetworkBehaviour`].
type THandlerErr<TBehaviour> =
    <<THandler<TBehaviour> as IntoConnectionHandler>::Handler as ConnectionHandler>::Error;

/// Event generated by the `Swarm`.
#[derive(Debug)]
pub enum SwarmEvent<TBehaviourOutEvent, THandlerErr> {
    /// Event generated by the `NetworkBehaviour`.
    Behaviour(TBehaviourOutEvent),
    /// A connection to the given peer has been opened.
    ConnectionEstablished {
        /// Identity of the peer that we have connected to.
        peer_id: PeerId,
        /// Endpoint of the connection that has been opened.
        endpoint: ConnectedPoint,
        /// Number of established connections to this peer, including the one that has just been
        /// opened.
        num_established: NonZeroU32,
        /// [`Some`] when the new connection is an outgoing connection.
        /// Addresses are dialed concurrently. Contains the addresses and errors
        /// of dial attempts that failed before the one successful dial.
        concurrent_dial_errors: Option<Vec<(Multiaddr, TransportError<io::Error>)>>,
        /// How long it took to establish this connection
        established_in: std::time::Duration,
    },
    /// A connection with the given peer has been closed,
    /// possibly as a result of an error.
    ConnectionClosed {
        /// Identity of the peer that we have connected to.
        peer_id: PeerId,
        /// Endpoint of the connection that has been closed.
        endpoint: ConnectedPoint,
        /// Number of other remaining connections to this same peer.
        num_established: u32,
        /// Reason for the disconnection, if it was not a successful
        /// active close.
        cause: Option<ConnectionError<THandlerErr>>,
    },
    /// A new connection arrived on a listener and is in the process of protocol negotiation.
    ///
    /// A corresponding [`ConnectionEstablished`](SwarmEvent::ConnectionEstablished),
    /// [`BannedPeer`](SwarmEvent::BannedPeer), or
    /// [`IncomingConnectionError`](SwarmEvent::IncomingConnectionError) event will later be
    /// generated for this connection.
    IncomingConnection {
        /// Local connection address.
        /// This address has been earlier reported with a [`NewListenAddr`](SwarmEvent::NewListenAddr)
        /// event.
        local_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
    },
    /// An error happened on a connection during its initial handshake.
    ///
    /// This can include, for example, an error during the handshake of the encryption layer, or
    /// the connection unexpectedly closed.
    IncomingConnectionError {
        /// Local connection address.
        /// This address has been earlier reported with a [`NewListenAddr`](SwarmEvent::NewListenAddr)
        /// event.
        local_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
        /// The error that happened.
        error: PendingInboundConnectionError,
    },
    /// Outgoing connection attempt failed.
    OutgoingConnectionError {
        /// If known, [`PeerId`] of the peer we tried to reach.
        peer_id: Option<PeerId>,
        /// Error that has been encountered.
        error: DialError,
    },
    /// We connected to a peer, but we immediately closed the connection because that peer is banned.
    BannedPeer {
        /// Identity of the banned peer.
        peer_id: PeerId,
        /// Endpoint of the connection that has been closed.
        endpoint: ConnectedPoint,
    },
    /// One of our listeners has reported a new local listening address.
    NewListenAddr {
        /// The listener that is listening on the new address.
        listener_id: ListenerId,
        /// The new address that is being listened on.
        address: Multiaddr,
    },
    /// One of our listeners has reported the expiration of a listening address.
    ExpiredListenAddr {
        /// The listener that is no longer listening on the address.
        listener_id: ListenerId,
        /// The expired address.
        address: Multiaddr,
    },
    /// One of the listeners gracefully closed.
    ListenerClosed {
        /// The listener that closed.
        listener_id: ListenerId,
        /// The addresses that the listener was listening on. These addresses are now considered
        /// expired, similar to if a [`ExpiredListenAddr`](SwarmEvent::ExpiredListenAddr) event
        /// has been generated for each of them.
        addresses: Vec<Multiaddr>,
        /// Reason for the closure. Contains `Ok(())` if the stream produced `None`, or `Err`
        /// if the stream produced an error.
        reason: Result<(), io::Error>,
    },
    /// One of the listeners reported a non-fatal error.
    ListenerError {
        /// The listener that errored.
        listener_id: ListenerId,
        /// The listener error.
        error: io::Error,
    },
    /// A new dialing attempt has been initiated by the [`NetworkBehaviour`]
    /// implementation.
    ///
    /// A [`ConnectionEstablished`](SwarmEvent::ConnectionEstablished) event is
    /// reported if the dialing attempt succeeds, otherwise a
    /// [`OutgoingConnectionError`](SwarmEvent::OutgoingConnectionError) event
    /// is reported.
    Dialing(PeerId),
}

/// Contains the state of the network, plus the way it should behave.
///
/// Note: Needs to be polled via `<Swarm as Stream>` in order to make
/// progress.
pub struct Swarm<TBehaviour>
where
    TBehaviour: NetworkBehaviour,
{
    /// [`Transport`] for dialing remote peers and listening for incoming connection.
    transport: transport::Boxed<(PeerId, StreamMuxerBox)>,

    /// The nodes currently active.
    pool: Pool<THandler<TBehaviour>>,

    /// The local peer ID.
    local_peer_id: PeerId,

    /// Handles which nodes to connect to and how to handle the events sent back by the protocol
    /// handlers.
    behaviour: TBehaviour,

    /// List of protocols that the behaviour says it supports.
    supported_protocols: SmallVec<[Vec<u8>; 16]>,

    /// Multiaddresses that our listeners are listening on,
    listened_addrs: HashMap<ListenerId, SmallVec<[Multiaddr; 1]>>,

    /// List of multiaddresses we're listening on, after account for external IP addresses and
    /// similar mechanisms.
    external_addrs: Addresses,

    /// List of nodes for which we deny any incoming connection.
    banned_peers: HashSet<PeerId>,

    /// Connections for which we withhold any reporting. These belong to banned peers.
    ///
    /// Note: Connections to a peer that are established at the time of banning that peer
    /// are not added here. Instead they are simply closed.
    banned_peer_connections: HashSet<ConnectionId>,

    /// Pending event to be delivered to connection handlers
    /// (or dropped if the peer disconnected) before the `behaviour`
    /// can be polled again.
    pending_event: Option<(PeerId, PendingNotifyHandler, THandlerInEvent<TBehaviour>)>,
}

impl<TBehaviour> Unpin for Swarm<TBehaviour> where TBehaviour: NetworkBehaviour {}

impl<TBehaviour> Swarm<TBehaviour>
where
    TBehaviour: NetworkBehaviour,
{
    /// Builds a new `Swarm` with a provided executor.
    pub fn with_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
        executor: impl Executor + Send + 'static,
    ) -> Self {
        SwarmBuilder::with_executor(transport, behaviour, local_peer_id, executor).build()
    }

    /// Builds a new `Swarm` with a tokio executor.
    #[cfg(all(
        feature = "tokio",
        not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown"))
    ))]
    pub fn with_tokio_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        Self::with_executor(
            transport,
            behaviour,
            local_peer_id,
            crate::executor::TokioExecutor,
        )
    }

    /// Builds a new `Swarm` with an async-std executor.
    #[cfg(all(
        feature = "async-std",
        not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown"))
    ))]
    pub fn with_async_std_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        Self::with_executor(
            transport,
            behaviour,
            local_peer_id,
            crate::executor::AsyncStdExecutor,
        )
    }

    /// Builds a new `Swarm` with a threadpool executor.
    pub fn with_threadpool_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        let builder = match ThreadPoolBuilder::new()
            .name_prefix("libp2p-swarm-task-")
            .create()
        {
            Ok(tp) => SwarmBuilder::with_executor(transport, behaviour, local_peer_id, tp),
            Err(err) => {
                log::warn!("Failed to create executor thread pool: {:?}", err);
                SwarmBuilder::without_executor(transport, behaviour, local_peer_id)
            }
        };
        builder.build()
    }

    /// Builds a new `Swarm` with a wasm executor.
    /// Background tasks will be executed by the browser on the next micro-tick.
    ///
    /// Spawning a task is similar too:
    /// ```typescript
    /// function spawn(task: () => Promise<void>) {
    ///     task()
    /// }
    /// ```
    #[cfg(feature = "wasm-bindgen")]
    pub fn with_wasm_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        Self::with_executor(
            transport,
            behaviour,
            local_peer_id,
            crate::executor::WasmBindgenExecutor,
        )
    }

    /// Builds a new `Swarm` without an executor, instead using the current task.
    ///
    /// ## ⚠️  Performance warning
    /// All connections will be polled on the current task, thus quite bad performance
    /// characteristics should be expected. Whenever possible use an executor and
    /// [`Swarm::with_executor`].
    pub fn without_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        SwarmBuilder::without_executor(transport, behaviour, local_peer_id).build()
    }

    /// Returns information about the connections underlying the [`Swarm`].
    pub fn network_info(&self) -> NetworkInfo {
        let num_peers = self.pool.num_peers();
        let connection_counters = self.pool.counters().clone();
        NetworkInfo {
            num_peers,
            connection_counters,
        }
    }

    /// Starts listening on the given address.
    /// Returns an error if the address is not supported.
    ///
    /// Listeners report their new listening addresses as [`SwarmEvent::NewListenAddr`].
    /// Depending on the underlying transport, one listener may have multiple listening addresses.
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<io::Error>> {
        let id = self.transport.listen_on(addr)?;
        self.behaviour
            .on_swarm_event(FromSwarm::NewListener(behaviour::NewListener {
                listener_id: id,
            }));
        Ok(id)
    }

    /// Remove some listener.
    ///
    /// Returns `true` if there was a listener with this ID, `false`
    /// otherwise.
    pub fn remove_listener(&mut self, listener_id: ListenerId) -> bool {
        self.transport.remove_listener(listener_id)
    }

    /// Dial a known or unknown peer.
    ///
    /// See also [`DialOpts`].
    ///
    /// ```
    /// # use libp2p_swarm::Swarm;
    /// # use libp2p_swarm::dial_opts::{DialOpts, PeerCondition};
    /// # use libp2p_core::{Multiaddr, PeerId, Transport};
    /// # use libp2p_core::transport::dummy::DummyTransport;
    /// # use libp2p_swarm::dummy;
    /// #
    /// let mut swarm = Swarm::without_executor(
    ///     DummyTransport::new().boxed(),
    ///     dummy::Behaviour,
    ///     PeerId::random(),
    /// );
    ///
    /// // Dial a known peer.
    /// swarm.dial(PeerId::random());
    ///
    /// // Dial an unknown peer.
    /// swarm.dial("/ip6/::1/tcp/12345".parse::<Multiaddr>().unwrap());
    /// ```
    pub fn dial(&mut self, opts: impl Into<DialOpts>) -> Result<(), DialError> {
        let handler = self.behaviour.new_handler();
        self.dial_with_handler(opts.into(), handler)
    }

    fn dial_with_handler(
        &mut self,
        dial_opts: DialOpts,
        handler: <TBehaviour as NetworkBehaviour>::ConnectionHandler,
    ) -> Result<(), DialError> {
        let peer_id = dial_opts
            .get_or_parse_peer_id()
            .map_err(DialError::InvalidPeerId)?;
        let condition = dial_opts.peer_condition();

        let should_dial = match (condition, peer_id) {
            (PeerCondition::Always, _) => true,
            (PeerCondition::Disconnected, None) => true,
            (PeerCondition::NotDialing, None) => true,
            (PeerCondition::Disconnected, Some(peer_id)) => !self.pool.is_connected(peer_id),
            (PeerCondition::NotDialing, Some(peer_id)) => !self.pool.is_dialing(peer_id),
        };

        if !should_dial {
            let e = DialError::DialPeerConditionFalse(condition);

            self.behaviour
                .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                    peer_id,
                    handler,
                    error: &e,
                }));

            return Err(e);
        }

        if let Some(peer_id) = peer_id {
            // Check if peer is banned.
            if self.banned_peers.contains(&peer_id) {
                let error = DialError::Banned;
                self.behaviour
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id: Some(peer_id),
                        handler,
                        error: &error,
                    }));

                return Err(error);
            }
        }

        let addresses = {
            let mut addresses = dial_opts.get_addresses();

            if let Some(peer_id) = peer_id {
                if dial_opts.extend_addresses_through_behaviour() {
                    addresses.extend(self.behaviour.addresses_of_peer(&peer_id));
                }
            }

            let mut unique_addresses = HashSet::new();
            addresses.retain(|addr| {
                !self.listened_addrs.values().flatten().any(|a| a == addr)
                    && unique_addresses.insert(addr.clone())
            });

            if addresses.is_empty() {
                let error = DialError::NoAddresses;
                self.behaviour
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id,
                        handler,
                        error: &error,
                    }));
                return Err(error);
            };

            addresses
        };

        let dials = addresses
            .into_iter()
            .map(|a| match p2p_addr(peer_id, a) {
                Ok(address) => {
                    let dial = match dial_opts.role_override() {
                        Endpoint::Dialer => self.transport.dial(address.clone()),
                        Endpoint::Listener => self.transport.dial_as_listener(address.clone()),
                    };
                    match dial {
                        Ok(fut) => fut
                            .map(|r| (address, r.map_err(TransportError::Other)))
                            .boxed(),
                        Err(err) => futures::future::ready((address, Err(err))).boxed(),
                    }
                }
                Err(address) => futures::future::ready((
                    address.clone(),
                    Err(TransportError::MultiaddrNotSupported(address)),
                ))
                .boxed(),
            })
            .collect();

        match self.pool.add_outgoing(
            dials,
            peer_id,
            handler,
            dial_opts.role_override(),
            dial_opts.dial_concurrency_override(),
        ) {
            Ok(_connection_id) => Ok(()),
            Err((connection_limit, handler)) => {
                let error = DialError::ConnectionLimit(connection_limit);
                self.behaviour
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id,
                        handler,
                        error: &error,
                    }));

                Err(error)
            }
        }
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    pub fn listeners(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listened_addrs.values().flatten()
    }

    /// Returns the peer ID of the swarm passed as parameter.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Returns an iterator for [`AddressRecord`]s of external addresses
    /// of the local node, in decreasing order of their current
    /// [score](AddressScore).
    pub fn external_addresses(&self) -> impl Iterator<Item = &AddressRecord> {
        self.external_addrs.iter()
    }

    /// Adds an external address record for the local node.
    ///
    /// An external address is an address of the local node known to
    /// be (likely) reachable for other nodes, possibly taking into
    /// account NAT. The external addresses of the local node may be
    /// shared with other nodes by the `NetworkBehaviour`.
    ///
    /// The associated score determines both the position of the address
    /// in the list of external addresses (which can determine the
    /// order in which addresses are used to connect to) as well as
    /// how long the address is retained in the list, depending on
    /// how frequently it is reported by the `NetworkBehaviour` via
    /// [`NetworkBehaviourAction::ReportObservedAddr`] or explicitly
    /// through this method.
    pub fn add_external_address(&mut self, a: Multiaddr, s: AddressScore) -> AddAddressResult {
        let result = self.external_addrs.add(a.clone(), s);
        let expired = match &result {
            AddAddressResult::Inserted { expired } => {
                self.behaviour
                    .on_swarm_event(FromSwarm::NewExternalAddr(NewExternalAddr { addr: &a }));
                expired
            }
            AddAddressResult::Updated { expired } => expired,
        };
        for a in expired {
            self.behaviour
                .on_swarm_event(FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr {
                    addr: &a.addr,
                }));
        }
        result
    }

    /// Removes an external address of the local node, regardless of
    /// its current score. See [`Swarm::add_external_address`]
    /// for details.
    ///
    /// Returns `true` if the address existed and was removed, `false`
    /// otherwise.
    pub fn remove_external_address(&mut self, addr: &Multiaddr) -> bool {
        if self.external_addrs.remove(addr) {
            self.behaviour
                .on_swarm_event(FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr { addr }));
            true
        } else {
            false
        }
    }

    /// Bans a peer by its peer ID.
    ///
    /// Any incoming connection and any dialing attempt will immediately be rejected.
    /// This function has no effect if the peer is already banned.
    pub fn ban_peer_id(&mut self, peer_id: PeerId) {
        if self.banned_peers.insert(peer_id) {
            // Note that established connections to the now banned peer are closed but not
            // added to [`Swarm::banned_peer_connections`]. They have been previously reported
            // as open to the behaviour and need be reported as closed once closing the
            // connection finishes.
            self.pool.disconnect(peer_id);
        }
    }

    /// Unbans a peer.
    pub fn unban_peer_id(&mut self, peer_id: PeerId) {
        self.banned_peers.remove(&peer_id);
    }

    /// Disconnects a peer by its peer ID, closing all connections to said peer.
    ///
    /// Returns `Ok(())` if there was one or more established connections to the peer.
    ///
    /// Note: Closing a connection via [`Swarm::disconnect_peer_id`] does
    /// not inform the corresponding [`ConnectionHandler`].
    /// Closing a connection via a [`ConnectionHandler`] can be done either in a
    /// collaborative manner across [`ConnectionHandler`]s
    /// with [`ConnectionHandler::connection_keep_alive`] or directly with
    /// [`ConnectionHandlerEvent::Close`].
    #[allow(clippy::result_unit_err)]
    pub fn disconnect_peer_id(&mut self, peer_id: PeerId) -> Result<(), ()> {
        let was_connected = self.pool.is_connected(peer_id);
        self.pool.disconnect(peer_id);

        if was_connected {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Checks whether there is an established connection to a peer.
    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.pool.is_connected(*peer_id)
    }

    /// Returns the currently connected peers.
    pub fn connected_peers(&self) -> impl Iterator<Item = &PeerId> {
        self.pool.iter_connected()
    }

    /// Returns a reference to the provided [`NetworkBehaviour`].
    pub fn behaviour(&self) -> &TBehaviour {
        &self.behaviour
    }

    /// Returns a mutable reference to the provided [`NetworkBehaviour`].
    pub fn behaviour_mut(&mut self) -> &mut TBehaviour {
        &mut self.behaviour
    }

    fn handle_pool_event(
        &mut self,
        event: PoolEvent<THandler<TBehaviour>>,
    ) -> Option<SwarmEvent<TBehaviour::OutEvent, THandlerErr<TBehaviour>>> {
        match event {
            PoolEvent::ConnectionEstablished {
                peer_id,
                id,
                endpoint,
                other_established_connection_ids,
                concurrent_dial_errors,
                established_in,
            } => {
                if self.banned_peers.contains(&peer_id) {
                    // Mark the connection for the banned peer as banned, thus withholding any
                    // future events from the connection to the behaviour.
                    self.banned_peer_connections.insert(id);
                    self.pool.disconnect(peer_id);
                    return Some(SwarmEvent::BannedPeer { peer_id, endpoint });
                } else {
                    let num_established = NonZeroU32::new(
                        u32::try_from(other_established_connection_ids.len() + 1).unwrap(),
                    )
                    .expect("n + 1 is always non-zero; qed");
                    let non_banned_established = other_established_connection_ids
                        .into_iter()
                        .filter(|conn_id| !self.banned_peer_connections.contains(conn_id))
                        .count();

                    log::debug!(
                            "Connection established: {:?} {:?}; Total (peer): {}. Total non-banned (peer): {}",
                            peer_id,
                            endpoint,
                            num_established,
                            non_banned_established + 1,
                        );
                    let failed_addresses = concurrent_dial_errors
                        .as_ref()
                        .map(|es| {
                            es.iter()
                                .map(|(a, _)| a)
                                .cloned()
                                .collect::<Vec<Multiaddr>>()
                        })
                        .unwrap_or_default();
                    self.behaviour
                        .on_swarm_event(FromSwarm::ConnectionEstablished(
                            behaviour::ConnectionEstablished {
                                peer_id,
                                connection_id: id,
                                endpoint: &endpoint,
                                failed_addresses: &failed_addresses,
                                other_established: non_banned_established,
                            },
                        ));
                    return Some(SwarmEvent::ConnectionEstablished {
                        peer_id,
                        num_established,
                        endpoint,
                        concurrent_dial_errors,
                        established_in,
                    });
                }
            }
            PoolEvent::PendingOutboundConnectionError {
                id: _,
                error,
                handler,
                peer,
            } => {
                let error = error.into();

                self.behaviour
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id: peer,
                        handler,
                        error: &error,
                    }));

                if let Some(peer) = peer {
                    log::debug!("Connection attempt to {:?} failed with {:?}.", peer, error,);
                } else {
                    log::debug!("Connection attempt to unknown peer failed with {:?}", error);
                }

                return Some(SwarmEvent::OutgoingConnectionError {
                    peer_id: peer,
                    error,
                });
            }
            PoolEvent::PendingInboundConnectionError {
                id: _,
                send_back_addr,
                local_addr,
                error,
                handler,
            } => {
                log::debug!("Incoming connection failed: {:?}", error);
                self.behaviour
                    .on_swarm_event(FromSwarm::ListenFailure(ListenFailure {
                        local_addr: &local_addr,
                        send_back_addr: &send_back_addr,
                        handler,
                    }));
                return Some(SwarmEvent::IncomingConnectionError {
                    local_addr,
                    send_back_addr,
                    error,
                });
            }
            PoolEvent::ConnectionClosed {
                id,
                connected,
                error,
                remaining_established_connection_ids,
                handler,
                ..
            } => {
                if let Some(error) = error.as_ref() {
                    log::debug!(
                        "Connection closed with error {:?}: {:?}; Total (peer): {}.",
                        error,
                        connected,
                        remaining_established_connection_ids.len()
                    );
                } else {
                    log::debug!(
                        "Connection closed: {:?}; Total (peer): {}.",
                        connected,
                        remaining_established_connection_ids.len()
                    );
                }
                let peer_id = connected.peer_id;
                let endpoint = connected.endpoint;
                let num_established =
                    u32::try_from(remaining_established_connection_ids.len()).unwrap();
                let conn_was_reported = !self.banned_peer_connections.remove(&id);
                if conn_was_reported {
                    let remaining_non_banned = remaining_established_connection_ids
                        .into_iter()
                        .filter(|conn_id| !self.banned_peer_connections.contains(conn_id))
                        .count();
                    self.behaviour
                        .on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
                            peer_id,
                            connection_id: id,
                            endpoint: &endpoint,
                            handler,
                            remaining_established: remaining_non_banned,
                        }));
                }
                return Some(SwarmEvent::ConnectionClosed {
                    peer_id,
                    endpoint,
                    cause: error,
                    num_established,
                });
            }
            PoolEvent::ConnectionEvent { peer_id, id, event } => {
                if self.banned_peer_connections.contains(&id) {
                    log::debug!("Ignoring event from banned peer: {} {:?}.", peer_id, id);
                } else {
                    self.behaviour
                        .on_connection_handler_event(peer_id, id, event);
                }
            }
            PoolEvent::AddressChange {
                peer_id,
                id,
                new_endpoint,
                old_endpoint,
            } => {
                if !self.banned_peer_connections.contains(&id) {
                    self.behaviour
                        .on_swarm_event(FromSwarm::AddressChange(AddressChange {
                            peer_id,
                            connection_id: id,
                            old: &old_endpoint,
                            new: &new_endpoint,
                        }));
                }
            }
        }

        None
    }

    fn handle_transport_event(
        &mut self,
        event: TransportEvent<
            <transport::Boxed<(PeerId, StreamMuxerBox)> as Transport>::ListenerUpgrade,
            io::Error,
        >,
    ) -> Option<SwarmEvent<TBehaviour::OutEvent, THandlerErr<TBehaviour>>> {
        match event {
            TransportEvent::Incoming {
                listener_id: _,
                upgrade,
                local_addr,
                send_back_addr,
            } => {
                let handler = self.behaviour.new_handler();
                match self.pool.add_incoming(
                    upgrade,
                    handler,
                    IncomingInfo {
                        local_addr: &local_addr,
                        send_back_addr: &send_back_addr,
                    },
                ) {
                    Ok(_connection_id) => {
                        return Some(SwarmEvent::IncomingConnection {
                            local_addr,
                            send_back_addr,
                        });
                    }
                    Err((connection_limit, handler)) => {
                        self.behaviour
                            .on_swarm_event(FromSwarm::ListenFailure(ListenFailure {
                                local_addr: &local_addr,
                                send_back_addr: &send_back_addr,
                                handler,
                            }));
                        log::warn!("Incoming connection rejected: {:?}", connection_limit);
                    }
                };
            }
            TransportEvent::NewAddress {
                listener_id,
                listen_addr,
            } => {
                log::debug!("Listener {:?}; New address: {:?}", listener_id, listen_addr);
                let addrs = self.listened_addrs.entry(listener_id).or_default();
                if !addrs.contains(&listen_addr) {
                    addrs.push(listen_addr.clone())
                }
                self.behaviour
                    .on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
                        listener_id,
                        addr: &listen_addr,
                    }));
                return Some(SwarmEvent::NewListenAddr {
                    listener_id,
                    address: listen_addr,
                });
            }
            TransportEvent::AddressExpired {
                listener_id,
                listen_addr,
            } => {
                log::debug!(
                    "Listener {:?}; Expired address {:?}.",
                    listener_id,
                    listen_addr
                );
                if let Some(addrs) = self.listened_addrs.get_mut(&listener_id) {
                    addrs.retain(|a| a != &listen_addr);
                }
                self.behaviour
                    .on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
                        listener_id,
                        addr: &listen_addr,
                    }));
                return Some(SwarmEvent::ExpiredListenAddr {
                    listener_id,
                    address: listen_addr,
                });
            }
            TransportEvent::ListenerClosed {
                listener_id,
                reason,
            } => {
                log::debug!("Listener {:?}; Closed by {:?}.", listener_id, reason);
                let addrs = self.listened_addrs.remove(&listener_id).unwrap_or_default();
                for addr in addrs.iter() {
                    self.behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(
                        ExpiredListenAddr { listener_id, addr },
                    ));
                }
                self.behaviour
                    .on_swarm_event(FromSwarm::ListenerClosed(ListenerClosed {
                        listener_id,
                        reason: reason.as_ref().copied(),
                    }));
                return Some(SwarmEvent::ListenerClosed {
                    listener_id,
                    addresses: addrs.to_vec(),
                    reason,
                });
            }
            TransportEvent::ListenerError { listener_id, error } => {
                self.behaviour
                    .on_swarm_event(FromSwarm::ListenerError(ListenerError {
                        listener_id,
                        err: &error,
                    }));
                return Some(SwarmEvent::ListenerError { listener_id, error });
            }
        }
        None
    }

    fn handle_behaviour_event(
        &mut self,
        event: NetworkBehaviourAction<TBehaviour::OutEvent, TBehaviour::ConnectionHandler>,
    ) -> Option<SwarmEvent<TBehaviour::OutEvent, THandlerErr<TBehaviour>>> {
        match event {
            NetworkBehaviourAction::GenerateEvent(event) => {
                return Some(SwarmEvent::Behaviour(event))
            }
            NetworkBehaviourAction::Dial { opts, handler } => {
                let peer_id = opts.get_or_parse_peer_id();
                if let Ok(()) = self.dial_with_handler(opts, handler) {
                    if let Ok(Some(peer_id)) = peer_id {
                        return Some(SwarmEvent::Dialing(peer_id));
                    }
                }
            }
            NetworkBehaviourAction::NotifyHandler {
                peer_id,
                handler,
                event,
            } => {
                assert!(self.pending_event.is_none());
                let handler = match handler {
                    NotifyHandler::One(connection) => PendingNotifyHandler::One(connection),
                    NotifyHandler::Any => {
                        let ids = self
                            .pool
                            .iter_established_connections_of_peer(&peer_id)
                            .collect();
                        PendingNotifyHandler::Any(ids)
                    }
                };

                self.pending_event = Some((peer_id, handler, event));
            }
            NetworkBehaviourAction::ReportObservedAddr { address, score } => {
                // Maps the given `observed_addr`, representing an address of the local
                // node observed by a remote peer, onto the locally known listen addresses
                // to yield one or more addresses of the local node that may be publicly
                // reachable.
                //
                // I.e. self method incorporates the view of other peers into the listen
                // addresses seen by the local node to account for possible IP and port
                // mappings performed by intermediate network devices in an effort to
                // obtain addresses for the local peer that are also reachable for peers
                // other than the peer who reported the `observed_addr`.
                //
                // The translation is transport-specific. See [`Transport::address_translation`].
                let translated_addresses = {
                    let mut addrs: Vec<_> = self
                        .listened_addrs
                        .values()
                        .flatten()
                        .filter_map(|server| self.transport.address_translation(server, &address))
                        .collect();

                    // remove duplicates
                    addrs.sort_unstable();
                    addrs.dedup();
                    addrs
                };
                for addr in translated_addresses {
                    self.add_external_address(addr, score);
                }
            }
            NetworkBehaviourAction::CloseConnection {
                peer_id,
                connection,
            } => match connection {
                CloseConnection::One(connection_id) => {
                    if let Some(conn) = self.pool.get_established(connection_id) {
                        conn.start_close();
                    }
                }
                CloseConnection::All => {
                    self.pool.disconnect(peer_id);
                }
            },
        }

        None
    }

    /// Internal function used by everything event-related.
    ///
    /// Polls the `Swarm` for the next event.
    fn poll_next_event(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<SwarmEvent<TBehaviour::OutEvent, THandlerErr<TBehaviour>>> {
        // We use a `this` variable because the compiler can't mutably borrow multiple times
        // across a `Deref`.
        let this = &mut *self;

        // This loop polls the components below in a prioritized order.
        //
        // 1. [`NetworkBehaviour`]
        // 2. Connection [`Pool`]
        // 3. [`ListenersStream`]
        //
        // (1) is polled before (2) to prioritize local work over work coming from a remote.
        //
        // (2) is polled before (3) to prioritize existing connections over upgrading new incoming connections.
        loop {
            match this.pending_event.take() {
                // Try to deliver the pending event emitted by the [`NetworkBehaviour`] in the previous
                // iteration to the connection handler(s).
                Some((peer_id, handler, event)) => match handler {
                    PendingNotifyHandler::One(conn_id) => {
                        match this.pool.get_established(conn_id) {
                            Some(conn) => match notify_one(conn, event, cx) {
                                None => continue,
                                Some(event) => {
                                    this.pending_event = Some((peer_id, handler, event));
                                }
                            },
                            None => continue,
                        }
                    }
                    PendingNotifyHandler::Any(ids) => {
                        match notify_any::<_, TBehaviour>(ids, &mut this.pool, event, cx) {
                            None => continue,
                            Some((event, ids)) => {
                                let handler = PendingNotifyHandler::Any(ids);
                                this.pending_event = Some((peer_id, handler, event));
                            }
                        }
                    }
                },
                // No pending event. Allow the [`NetworkBehaviour`] to make progress.
                None => {
                    let behaviour_poll = {
                        let mut parameters = SwarmPollParameters {
                            local_peer_id: &this.local_peer_id,
                            supported_protocols: &this.supported_protocols,
                            listened_addrs: this.listened_addrs.values().flatten().collect(),
                            external_addrs: &this.external_addrs,
                        };
                        this.behaviour.poll(cx, &mut parameters)
                    };

                    match behaviour_poll {
                        Poll::Pending => {}
                        Poll::Ready(behaviour_event) => {
                            if let Some(swarm_event) = this.handle_behaviour_event(behaviour_event)
                            {
                                return Poll::Ready(swarm_event);
                            }

                            continue;
                        }
                    }
                }
            }

            // Poll the known peers.
            match this.pool.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(pool_event) => {
                    if let Some(swarm_event) = this.handle_pool_event(pool_event) {
                        return Poll::Ready(swarm_event);
                    }

                    continue;
                }
            };

            // Poll the listener(s) for new connections.
            match Pin::new(&mut this.transport).poll(cx) {
                Poll::Pending => {}
                Poll::Ready(transport_event) => {
                    if let Some(swarm_event) = this.handle_transport_event(transport_event) {
                        return Poll::Ready(swarm_event);
                    }

                    continue;
                }
            }

            return Poll::Pending;
        }
    }
}

/// Connection to notify of a pending event.
///
/// The connection IDs out of which to notify one of an event are captured at
/// the time the behaviour emits the event, in order not to forward the event to
/// a new connection which the behaviour may not have been aware of at the time
/// it issued the request for sending it.
enum PendingNotifyHandler {
    One(ConnectionId),
    Any(SmallVec<[ConnectionId; 10]>),
}

/// Notify a single connection of an event.
///
/// Returns `Some` with the given event if the connection is not currently
/// ready to receive another event, in which case the current task is
/// scheduled to be woken up.
///
/// Returns `None` if the connection is closing or the event has been
/// successfully sent, in either case the event is consumed.
fn notify_one<THandlerInEvent>(
    conn: &mut EstablishedConnection<THandlerInEvent>,
    event: THandlerInEvent,
    cx: &mut Context<'_>,
) -> Option<THandlerInEvent> {
    match conn.poll_ready_notify_handler(cx) {
        Poll::Pending => Some(event),
        Poll::Ready(Err(())) => None, // connection is closing
        Poll::Ready(Ok(())) => {
            // Can now only fail if connection is closing.
            let _ = conn.notify_handler(event);
            None
        }
    }
}

/// Notify any one of a given list of connections of a peer of an event.
///
/// Returns `Some` with the given event and a new list of connections if
/// none of the given connections was able to receive the event but at
/// least one of them is not closing, in which case the current task
/// is scheduled to be woken up. The returned connections are those which
/// may still become ready to receive another event.
///
/// Returns `None` if either all connections are closing or the event
/// was successfully sent to a handler, in either case the event is consumed.
fn notify_any<THandler, TBehaviour>(
    ids: SmallVec<[ConnectionId; 10]>,
    pool: &mut Pool<THandler>,
    event: THandlerInEvent<TBehaviour>,
    cx: &mut Context<'_>,
) -> Option<(THandlerInEvent<TBehaviour>, SmallVec<[ConnectionId; 10]>)>
where
    TBehaviour: NetworkBehaviour,
    THandler: IntoConnectionHandler,
    THandler::Handler: ConnectionHandler<
        InEvent = THandlerInEvent<TBehaviour>,
        OutEvent = THandlerOutEvent<TBehaviour>,
    >,
{
    let mut pending = SmallVec::new();
    let mut event = Some(event); // (1)
    for id in ids.into_iter() {
        if let Some(conn) = pool.get_established(id) {
            match conn.poll_ready_notify_handler(cx) {
                Poll::Pending => pending.push(id),
                Poll::Ready(Err(())) => {} // connection is closing
                Poll::Ready(Ok(())) => {
                    let e = event.take().expect("by (1),(2)");
                    if let Err(e) = conn.notify_handler(e) {
                        event = Some(e) // (2)
                    } else {
                        break;
                    }
                }
            }
        }
    }

    event.and_then(|e| {
        if !pending.is_empty() {
            Some((e, pending))
        } else {
            None
        }
    })
}

/// Stream of events returned by [`Swarm`].
///
/// Includes events from the [`NetworkBehaviour`] as well as events about
/// connection and listener status. See [`SwarmEvent`] for details.
///
/// Note: This stream is infinite and it is guaranteed that
/// [`Stream::poll_next`] will never return `Poll::Ready(None)`.
impl<TBehaviour> Stream for Swarm<TBehaviour>
where
    TBehaviour: NetworkBehaviour,
{
    type Item = SwarmEvent<TBehaviourOutEvent<TBehaviour>, THandlerErr<TBehaviour>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.as_mut().poll_next_event(cx).map(Some)
    }
}

/// The stream of swarm events never terminates, so we can implement fused for it.
impl<TBehaviour> FusedStream for Swarm<TBehaviour>
where
    TBehaviour: NetworkBehaviour,
{
    fn is_terminated(&self) -> bool {
        false
    }
}

/// Parameters passed to `poll()`, that the `NetworkBehaviour` has access to.
// TODO: #[derive(Debug)]
pub struct SwarmPollParameters<'a> {
    local_peer_id: &'a PeerId,
    supported_protocols: &'a [Vec<u8>],
    listened_addrs: Vec<&'a Multiaddr>,
    external_addrs: &'a Addresses,
}

impl<'a> PollParameters for SwarmPollParameters<'a> {
    type SupportedProtocolsIter = std::iter::Cloned<std::slice::Iter<'a, std::vec::Vec<u8>>>;
    type ListenedAddressesIter = std::iter::Cloned<std::vec::IntoIter<&'a Multiaddr>>;
    type ExternalAddressesIter = AddressIntoIter;

    fn supported_protocols(&self) -> Self::SupportedProtocolsIter {
        self.supported_protocols.iter().cloned()
    }

    fn listened_addresses(&self) -> Self::ListenedAddressesIter {
        self.listened_addrs.clone().into_iter().cloned()
    }

    fn external_addresses(&self) -> Self::ExternalAddressesIter {
        self.external_addrs.clone().into_iter()
    }

    fn local_peer_id(&self) -> &PeerId {
        self.local_peer_id
    }
}

/// A [`SwarmBuilder`] provides an API for configuring and constructing a [`Swarm`].
pub struct SwarmBuilder<TBehaviour> {
    local_peer_id: PeerId,
    transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
    behaviour: TBehaviour,
    pool_config: PoolConfig,
    connection_limits: ConnectionLimits,
}

impl<TBehaviour> SwarmBuilder<TBehaviour>
where
    TBehaviour: NetworkBehaviour,
{
    /// Creates a new [`SwarmBuilder`] from the given transport, behaviour, local peer ID and
    /// executor. The `Swarm` with its underlying `Network` is obtained via
    /// [`SwarmBuilder::build`].
    pub fn with_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
        executor: impl Executor + Send + 'static,
    ) -> Self {
        Self {
            local_peer_id,
            transport,
            behaviour,
            pool_config: PoolConfig::new(Some(Box::new(executor))),
            connection_limits: Default::default(),
        }
    }

    /// Builds a new [`SwarmBuilder`] from the given transport, behaviour, local peer ID and a
    /// `tokio` executor.
    #[cfg(all(
        feature = "tokio",
        not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown"))
    ))]
    pub fn with_tokio_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        Self::with_executor(
            transport,
            behaviour,
            local_peer_id,
            crate::executor::TokioExecutor,
        )
    }

    /// Builds a new [`SwarmBuilder`] from the given transport, behaviour, local peer ID and a
    /// `async-std` executor.
    #[cfg(all(
        feature = "async-std",
        not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown"))
    ))]
    pub fn with_async_std_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        Self::with_executor(
            transport,
            behaviour,
            local_peer_id,
            crate::executor::AsyncStdExecutor,
        )
    }

    /// Creates a new [`SwarmBuilder`] from the given transport, behaviour and local peer ID. The
    /// `Swarm` with its underlying `Network` is obtained via [`SwarmBuilder::build`].
    ///
    /// ## ⚠️  Performance warning
    /// All connections will be polled on the current task, thus quite bad performance
    /// characteristics should be expected. Whenever possible use an executor and
    /// [`SwarmBuilder::with_executor`].
    pub fn without_executor(
        transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
        behaviour: TBehaviour,
        local_peer_id: PeerId,
    ) -> Self {
        Self {
            local_peer_id,
            transport,
            behaviour,
            pool_config: PoolConfig::new(None),
            connection_limits: Default::default(),
        }
    }

    /// Configures the number of events from the [`NetworkBehaviour`] in
    /// destination to the [`ConnectionHandler`] that can be buffered before
    /// the [`Swarm`] has to wait. An individual buffer with this number of
    /// events exists for each individual connection.
    ///
    /// The ideal value depends on the executor used, the CPU speed, and the
    /// volume of events. If this value is too low, then the [`Swarm`] will
    /// be sleeping more often than necessary. Increasing this value increases
    /// the overall memory usage.
    pub fn notify_handler_buffer_size(mut self, n: NonZeroUsize) -> Self {
        self.pool_config = self.pool_config.with_notify_handler_buffer_size(n);
        self
    }

    /// Configures the number of extra events from the [`ConnectionHandler`] in
    /// destination to the [`NetworkBehaviour`] that can be buffered before
    /// the [`ConnectionHandler`] has to go to sleep.
    ///
    /// There exists a buffer of events received from [`ConnectionHandler`]s
    /// that the [`NetworkBehaviour`] has yet to process. This buffer is
    /// shared between all instances of [`ConnectionHandler`]. Each instance of
    /// [`ConnectionHandler`] is guaranteed one slot in this buffer, meaning
    /// that delivering an event for the first time is guaranteed to be
    /// instantaneous. Any extra event delivery, however, must wait for that
    /// first event to be delivered or for an "extra slot" to be available.
    ///
    /// This option configures the number of such "extra slots" in this
    /// shared buffer. These extra slots are assigned in a first-come,
    /// first-served basis.
    ///
    /// The ideal value depends on the executor used, the CPU speed, the
    /// average number of connections, and the volume of events. If this value
    /// is too low, then the [`ConnectionHandler`]s will be sleeping more often
    /// than necessary. Increasing this value increases the overall memory
    /// usage, and more importantly the latency between the moment when an
    /// event is emitted and the moment when it is received by the
    /// [`NetworkBehaviour`].
    pub fn connection_event_buffer_size(mut self, n: usize) -> Self {
        self.pool_config = self.pool_config.with_connection_event_buffer_size(n);
        self
    }

    /// Number of addresses concurrently dialed for a single outbound connection attempt.
    pub fn dial_concurrency_factor(mut self, factor: NonZeroU8) -> Self {
        self.pool_config = self.pool_config.with_dial_concurrency_factor(factor);
        self
    }

    /// Configures the connection limits.
    pub fn connection_limits(mut self, limits: ConnectionLimits) -> Self {
        self.connection_limits = limits;
        self
    }

    /// Configures an override for the substream upgrade protocol to use.
    ///
    /// The subtream upgrade protocol is the multistream-select protocol
    /// used for protocol negotiation on substreams. Since a listener
    /// supports all existing versions, the choice of upgrade protocol
    /// only effects the "dialer", i.e. the peer opening a substream.
    ///
    /// > **Note**: If configured, specific upgrade protocols for
    /// > individual [`SubstreamProtocol`]s emitted by the `NetworkBehaviour`
    /// > are ignored.
    pub fn substream_upgrade_protocol_override(mut self, v: libp2p_core::upgrade::Version) -> Self {
        self.pool_config = self.pool_config.with_substream_upgrade_protocol_override(v);
        self
    }

    /// The maximum number of inbound streams concurrently negotiating on a
    /// connection. New inbound streams exceeding the limit are dropped and thus
    /// reset.
    ///
    /// Note: This only enforces a limit on the number of concurrently
    /// negotiating inbound streams. The total number of inbound streams on a
    /// connection is the sum of negotiating and negotiated streams. A limit on
    /// the total number of streams can be enforced at the
    /// [`StreamMuxerBox`](libp2p_core::muxing::StreamMuxerBox) level.
    pub fn max_negotiating_inbound_streams(mut self, v: usize) -> Self {
        self.pool_config = self.pool_config.with_max_negotiating_inbound_streams(v);
        self
    }

    /// Builds a `Swarm` with the current configuration.
    pub fn build(mut self) -> Swarm<TBehaviour> {
        let supported_protocols = self
            .behaviour
            .new_handler()
            .inbound_protocol()
            .protocol_info()
            .into_iter()
            .map(|info| info.protocol_name().to_vec())
            .collect();

        Swarm {
            local_peer_id: self.local_peer_id,
            transport: self.transport,
            pool: Pool::new(self.local_peer_id, self.pool_config, self.connection_limits),
            behaviour: self.behaviour,
            supported_protocols,
            listened_addrs: HashMap::new(),
            external_addrs: Addresses::default(),
            banned_peers: HashSet::new(),
            banned_peer_connections: HashSet::new(),
            pending_event: None,
        }
    }
}

/// The possible failures of dialing.
#[derive(Debug)]
pub enum DialError {
    /// The peer is currently banned.
    Banned,
    /// The configured limit for simultaneous outgoing connections
    /// has been reached.
    ConnectionLimit(ConnectionLimit),
    /// The peer being dialed is the local peer and thus the dial was aborted.
    LocalPeerId,
    /// [`NetworkBehaviour::addresses_of_peer`] returned no addresses
    /// for the peer to dial.
    NoAddresses,
    /// The provided [`dial_opts::PeerCondition`] evaluated to false and thus
    /// the dial was aborted.
    DialPeerConditionFalse(dial_opts::PeerCondition),
    /// Pending connection attempt has been aborted.
    Aborted,
    /// The provided peer identity is invalid.
    InvalidPeerId(Multihash),
    /// The peer identity obtained on the connection did not match the one that was expected.
    WrongPeerId {
        obtained: PeerId,
        endpoint: ConnectedPoint,
    },
    /// An I/O error occurred on the connection.
    ConnectionIo(io::Error),
    /// An error occurred while negotiating the transport protocol(s) on a connection.
    Transport(Vec<(Multiaddr, TransportError<io::Error>)>),
}

impl From<PendingOutboundConnectionError> for DialError {
    fn from(error: PendingOutboundConnectionError) -> Self {
        match error {
            PendingConnectionError::ConnectionLimit(limit) => DialError::ConnectionLimit(limit),
            PendingConnectionError::Aborted => DialError::Aborted,
            PendingConnectionError::WrongPeerId { obtained, endpoint } => {
                DialError::WrongPeerId { obtained, endpoint }
            }
            PendingConnectionError::IO(e) => DialError::ConnectionIo(e),
            PendingConnectionError::Transport(e) => DialError::Transport(e),
        }
    }
}

impl fmt::Display for DialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DialError::ConnectionLimit(err) => write!(f, "Dial error: {err}"),
            DialError::NoAddresses => write!(f, "Dial error: no addresses for peer."),
            DialError::LocalPeerId => write!(f, "Dial error: tried to dial local peer id."),
            DialError::Banned => write!(f, "Dial error: peer is banned."),
            DialError::DialPeerConditionFalse(c) => {
                write!(f, "Dial error: condition {c:?} for dialing peer was false.")
            }
            DialError::Aborted => write!(
                f,
                "Dial error: Pending connection attempt has been aborted."
            ),
            DialError::InvalidPeerId(multihash) => {
                write!(f, "Dial error: multihash {multihash:?} is not a PeerId")
            }
            DialError::WrongPeerId { obtained, endpoint } => write!(
                f,
                "Dial error: Unexpected peer ID {obtained} at {endpoint:?}."
            ),
            DialError::ConnectionIo(e) => write!(
                f,
                "Dial error: An I/O error occurred on the connection: {e:?}."
            ),
            DialError::Transport(errors) => {
                write!(f, "Failed to negotiate transport protocol(s): [")?;

                for (addr, error) in errors {
                    write!(f, "({addr}")?;
                    print_error_chain(f, error)?;
                    write!(f, ")")?;
                }
                write!(f, "]")?;

                Ok(())
            }
        }
    }
}

fn print_error_chain(f: &mut fmt::Formatter<'_>, e: &dyn error::Error) -> fmt::Result {
    write!(f, ": {e}")?;

    if let Some(source) = e.source() {
        print_error_chain(f, source)?;
    }

    Ok(())
}

impl error::Error for DialError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DialError::ConnectionLimit(err) => Some(err),
            DialError::LocalPeerId => None,
            DialError::NoAddresses => None,
            DialError::Banned => None,
            DialError::DialPeerConditionFalse(_) => None,
            DialError::Aborted => None,
            DialError::InvalidPeerId { .. } => None,
            DialError::WrongPeerId { .. } => None,
            DialError::ConnectionIo(_) => None,
            DialError::Transport(_) => None,
        }
    }
}

/// Information about the connections obtained by [`Swarm::network_info()`].
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

/// Ensures a given `Multiaddr` is a `/p2p/...` address for the given peer.
///
/// If the given address is already a `p2p` address for the given peer,
/// i.e. the last encapsulated protocol is `/p2p/<peer-id>`, this is a no-op.
///
/// If the given address is already a `p2p` address for a different peer
/// than the one given, the given `Multiaddr` is returned as an `Err`.
///
/// If the given address is not yet a `p2p` address for the given peer,
/// the `/p2p/<peer-id>` protocol is appended to the returned address.
fn p2p_addr(peer: Option<PeerId>, addr: Multiaddr) -> Result<Multiaddr, Multiaddr> {
    let peer = match peer {
        Some(p) => p,
        None => return Ok(addr),
    };

    if let Some(Protocol::P2p(hash)) = addr.iter().last() {
        if &hash != peer.as_ref() {
            return Err(addr);
        }
        Ok(addr)
    } else {
        Ok(addr.with(Protocol::P2p(peer.into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{CallTraceBehaviour, MockBehaviour};
    use either::Either;
    use futures::executor::block_on;
    use futures::executor::ThreadPool;
    use futures::future::poll_fn;
    use futures::{executor, future, ready};
    use libp2p_core::multiaddr::multiaddr;
    use libp2p_core::transport::memory::MemoryTransportError;
    use libp2p_core::transport::TransportEvent;
    use libp2p_core::{identity, multiaddr, transport, upgrade};
    use libp2p_core::{Endpoint, UpgradeError};
    use libp2p_plaintext as plaintext;
    use libp2p_yamux as yamux;
    use quickcheck::*;
    use void::Void;

    // Test execution state.
    // Connection => Disconnecting => Connecting.
    enum State {
        Connecting,
        Disconnecting,
    }

    fn new_test_swarm<T, O>(
        handler_proto: T,
    ) -> SwarmBuilder<CallTraceBehaviour<MockBehaviour<T, O>>>
    where
        T: ConnectionHandler + Clone,
        T::OutEvent: Clone,
        O: Send + 'static,
    {
        let id_keys = identity::Keypair::generate_ed25519();
        let local_public_key = id_keys.public();
        let transport = transport::MemoryTransport::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(plaintext::PlainText2Config {
                local_public_key: local_public_key.clone(),
            })
            .multiplex(yamux::YamuxConfig::default())
            .boxed();
        let behaviour = CallTraceBehaviour::new(MockBehaviour::new(handler_proto));
        match ThreadPool::new().ok() {
            Some(tp) => {
                SwarmBuilder::with_executor(transport, behaviour, local_public_key.into(), tp)
            }
            None => SwarmBuilder::without_executor(transport, behaviour, local_public_key.into()),
        }
    }

    fn swarms_connected<TBehaviour>(
        swarm1: &Swarm<CallTraceBehaviour<TBehaviour>>,
        swarm2: &Swarm<CallTraceBehaviour<TBehaviour>>,
        num_connections: usize,
    ) -> bool
    where
        TBehaviour: NetworkBehaviour,
        <<TBehaviour::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent: Clone,
    {
        swarm1
            .behaviour()
            .num_connections_to_peer(*swarm2.local_peer_id())
            == num_connections
            && swarm2
                .behaviour()
                .num_connections_to_peer(*swarm1.local_peer_id())
                == num_connections
            && swarm1.is_connected(swarm2.local_peer_id())
            && swarm2.is_connected(swarm1.local_peer_id())
    }

    fn swarms_disconnected<TBehaviour: NetworkBehaviour>(
        swarm1: &Swarm<CallTraceBehaviour<TBehaviour>>,
        swarm2: &Swarm<CallTraceBehaviour<TBehaviour>>,
    ) -> bool
    where
        TBehaviour: NetworkBehaviour,
        <<TBehaviour::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent: Clone
    {
        swarm1
            .behaviour()
            .num_connections_to_peer(*swarm2.local_peer_id())
            == 0
            && swarm2
                .behaviour()
                .num_connections_to_peer(*swarm1.local_peer_id())
                == 0
            && !swarm1.is_connected(swarm2.local_peer_id())
            && !swarm2.is_connected(swarm1.local_peer_id())
    }

    /// Establishes multiple connections between two peers,
    /// after which one peer bans the other.
    ///
    /// The test expects both behaviours to be notified via calls to [`NetworkBehaviour::on_swarm_event`]
    /// with pairs of [`FromSwarm::ConnectionEstablished`] / [`FromSwarm::ConnectionClosed`]
    /// while unbanned.
    ///
    /// While the ban is in effect, further dials occur. For these connections no
    /// [`FromSwarm::ConnectionEstablished`], [`FromSwarm::ConnectionClosed`]
    /// calls should be registered.
    #[test]
    fn test_connect_disconnect_ban() {
        // Since the test does not try to open any substreams, we can
        // use the dummy protocols handler.
        let handler_proto = keep_alive::ConnectionHandler;

        let mut swarm1 = new_test_swarm::<_, ()>(handler_proto.clone()).build();
        let mut swarm2 = new_test_swarm::<_, ()>(handler_proto).build();

        let addr1: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();
        let addr2: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();

        swarm1.listen_on(addr1).unwrap();
        swarm2.listen_on(addr2.clone()).unwrap();

        let swarm1_id = *swarm1.local_peer_id();

        enum Stage {
            /// Waiting for the peers to connect. Banning has not occurred.
            Connecting,
            /// Ban occurred.
            Banned,
            // Ban is in place and a dial is ongoing.
            BannedDial,
            // Mid-ban dial was registered and the peer was unbanned.
            Unbanned,
            // There are dial attempts ongoing for the no longer banned peers.
            Reconnecting,
        }

        let num_connections = 10;

        for _ in 0..num_connections {
            swarm1.dial(addr2.clone()).unwrap();
        }

        let mut s1_expected_conns = num_connections;
        let mut s2_expected_conns = num_connections;

        let mut stage = Stage::Connecting;

        executor::block_on(future::poll_fn(move |cx| loop {
            let poll1 = Swarm::poll_next_event(Pin::new(&mut swarm1), cx);
            let poll2 = Swarm::poll_next_event(Pin::new(&mut swarm2), cx);
            match stage {
                Stage::Connecting => {
                    if swarm1.behaviour.assert_connected(s1_expected_conns, 1)
                        && swarm2.behaviour.assert_connected(s2_expected_conns, 1)
                    {
                        // Setup to test that already established connections are correctly closed
                        // and reported as such after the peer is banned.
                        swarm2.ban_peer_id(swarm1_id);
                        stage = Stage::Banned;
                    }
                }
                Stage::Banned => {
                    if swarm1.behaviour.assert_disconnected(s1_expected_conns, 1)
                        && swarm2.behaviour.assert_disconnected(s2_expected_conns, 1)
                    {
                        // Setup to test that new connections of banned peers are not reported.
                        swarm1.dial(addr2.clone()).unwrap();
                        s1_expected_conns += 1;
                        stage = Stage::BannedDial;
                    }
                }
                Stage::BannedDial => {
                    if swarm2.network_info().num_peers() == 1 {
                        // The banned connection was established. Check that it was not reported to
                        // the behaviour of the banning swarm.
                        assert_eq!(
                            swarm2.behaviour.on_connection_established.len(), s2_expected_conns,
                            "No additional closed connections should be reported for the banned peer"
                        );

                        // Setup to test that the banned connection is not reported upon closing
                        // even if the peer is unbanned.
                        swarm2.unban_peer_id(swarm1_id);
                        stage = Stage::Unbanned;
                    }
                }
                Stage::Unbanned => {
                    if swarm2.network_info().num_peers() == 0 {
                        // The banned connection has closed. Check that it was not reported.
                        assert_eq!(
                            swarm2.behaviour.on_connection_closed.len(), s2_expected_conns,
                            "No additional closed connections should be reported for the banned peer"
                        );
                        assert!(swarm2.banned_peer_connections.is_empty());

                        // Setup to test that a ban lifted does not affect future connections.
                        for _ in 0..num_connections {
                            swarm1.dial(addr2.clone()).unwrap();
                        }
                        s1_expected_conns += num_connections;
                        s2_expected_conns += num_connections;
                        stage = Stage::Reconnecting;
                    }
                }
                Stage::Reconnecting => {
                    if swarm1.behaviour.on_connection_established.len() == s1_expected_conns
                        && swarm2.behaviour.assert_connected(s2_expected_conns, 2)
                    {
                        return Poll::Ready(());
                    }
                }
            }

            if poll1.is_pending() && poll2.is_pending() {
                return Poll::Pending;
            }
        }))
    }

    /// Establishes multiple connections between two peers,
    /// after which one peer disconnects the other using [`Swarm::disconnect_peer_id`].
    ///
    /// The test expects both behaviours to be notified via calls to [`NetworkBehaviour::on_swarm_event`]
    /// with pairs of [`FromSwarm::ConnectionEstablished`] / [`FromSwarm::ConnectionClosed`]
    #[test]
    fn test_swarm_disconnect() {
        // Since the test does not try to open any substreams, we can
        // use the dummy protocols handler.
        let handler_proto = keep_alive::ConnectionHandler;

        let mut swarm1 = new_test_swarm::<_, ()>(handler_proto.clone()).build();
        let mut swarm2 = new_test_swarm::<_, ()>(handler_proto).build();

        let addr1: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();
        let addr2: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();

        swarm1.listen_on(addr1.clone()).unwrap();
        swarm2.listen_on(addr2.clone()).unwrap();

        let swarm1_id = *swarm1.local_peer_id();

        let mut reconnected = false;
        let num_connections = 10;

        for _ in 0..num_connections {
            swarm1.dial(addr2.clone()).unwrap();
        }
        let mut state = State::Connecting;

        executor::block_on(future::poll_fn(move |cx| loop {
            let poll1 = Swarm::poll_next_event(Pin::new(&mut swarm1), cx);
            let poll2 = Swarm::poll_next_event(Pin::new(&mut swarm2), cx);
            match state {
                State::Connecting => {
                    if swarms_connected(&swarm1, &swarm2, num_connections) {
                        if reconnected {
                            return Poll::Ready(());
                        }
                        swarm2
                            .disconnect_peer_id(swarm1_id)
                            .expect("Error disconnecting");
                        state = State::Disconnecting;
                    }
                }
                State::Disconnecting => {
                    if swarms_disconnected(&swarm1, &swarm2) {
                        if reconnected {
                            return Poll::Ready(());
                        }
                        reconnected = true;
                        for _ in 0..num_connections {
                            swarm2.dial(addr1.clone()).unwrap();
                        }
                        state = State::Connecting;
                    }
                }
            }

            if poll1.is_pending() && poll2.is_pending() {
                return Poll::Pending;
            }
        }))
    }

    /// Establishes multiple connections between two peers,
    /// after which one peer disconnects the other
    /// using [`NetworkBehaviourAction::CloseConnection`] returned by a [`NetworkBehaviour`].
    ///
    /// The test expects both behaviours to be notified via calls to [`NetworkBehaviour::on_swarm_event`]
    /// with pairs of [`FromSwarm::ConnectionEstablished`] / [`FromSwarm::ConnectionClosed`]
    #[test]
    fn test_behaviour_disconnect_all() {
        // Since the test does not try to open any substreams, we can
        // use the dummy protocols handler.
        let handler_proto = keep_alive::ConnectionHandler;

        let mut swarm1 = new_test_swarm::<_, ()>(handler_proto.clone()).build();
        let mut swarm2 = new_test_swarm::<_, ()>(handler_proto).build();

        let addr1: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();
        let addr2: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();

        swarm1.listen_on(addr1.clone()).unwrap();
        swarm2.listen_on(addr2.clone()).unwrap();

        let swarm1_id = *swarm1.local_peer_id();

        let mut reconnected = false;
        let num_connections = 10;

        for _ in 0..num_connections {
            swarm1.dial(addr2.clone()).unwrap();
        }
        let mut state = State::Connecting;

        executor::block_on(future::poll_fn(move |cx| loop {
            let poll1 = Swarm::poll_next_event(Pin::new(&mut swarm1), cx);
            let poll2 = Swarm::poll_next_event(Pin::new(&mut swarm2), cx);
            match state {
                State::Connecting => {
                    if swarms_connected(&swarm1, &swarm2, num_connections) {
                        if reconnected {
                            return Poll::Ready(());
                        }
                        swarm2.behaviour.inner().next_action.replace(
                            NetworkBehaviourAction::CloseConnection {
                                peer_id: swarm1_id,
                                connection: CloseConnection::All,
                            },
                        );
                        state = State::Disconnecting;
                        continue;
                    }
                }
                State::Disconnecting => {
                    if swarms_disconnected(&swarm1, &swarm2) {
                        reconnected = true;
                        for _ in 0..num_connections {
                            swarm2.dial(addr1.clone()).unwrap();
                        }
                        state = State::Connecting;
                        continue;
                    }
                }
            }

            if poll1.is_pending() && poll2.is_pending() {
                return Poll::Pending;
            }
        }))
    }

    /// Establishes multiple connections between two peers,
    /// after which one peer closes a single connection
    /// using [`NetworkBehaviourAction::CloseConnection`] returned by a [`NetworkBehaviour`].
    ///
    /// The test expects both behaviours to be notified via calls to [`NetworkBehaviour::on_swarm_event`]
    /// with pairs of [`FromSwarm::ConnectionEstablished`] / [`FromSwarm::ConnectionClosed`]
    #[test]
    fn test_behaviour_disconnect_one() {
        // Since the test does not try to open any substreams, we can
        // use the dummy protocols handler.
        let handler_proto = keep_alive::ConnectionHandler;

        let mut swarm1 = new_test_swarm::<_, ()>(handler_proto.clone()).build();
        let mut swarm2 = new_test_swarm::<_, ()>(handler_proto).build();

        let addr1: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();
        let addr2: Multiaddr = multiaddr::Protocol::Memory(rand::random::<u64>()).into();

        swarm1.listen_on(addr1).unwrap();
        swarm2.listen_on(addr2.clone()).unwrap();

        let swarm1_id = *swarm1.local_peer_id();

        let num_connections = 10;

        for _ in 0..num_connections {
            swarm1.dial(addr2.clone()).unwrap();
        }
        let mut state = State::Connecting;
        let mut disconnected_conn_id = None;

        executor::block_on(future::poll_fn(move |cx| loop {
            let poll1 = Swarm::poll_next_event(Pin::new(&mut swarm1), cx);
            let poll2 = Swarm::poll_next_event(Pin::new(&mut swarm2), cx);
            match state {
                State::Connecting => {
                    if swarms_connected(&swarm1, &swarm2, num_connections) {
                        disconnected_conn_id = {
                            let conn_id =
                                swarm2.behaviour.on_connection_established[num_connections / 2].1;
                            swarm2.behaviour.inner().next_action.replace(
                                NetworkBehaviourAction::CloseConnection {
                                    peer_id: swarm1_id,
                                    connection: CloseConnection::One(conn_id),
                                },
                            );
                            Some(conn_id)
                        };
                        state = State::Disconnecting;
                    }
                }
                State::Disconnecting => {
                    for s in &[&swarm1, &swarm2] {
                        assert!(s
                            .behaviour
                            .on_connection_closed
                            .iter()
                            .all(|(.., remaining_conns)| *remaining_conns > 0));
                        assert_eq!(s.behaviour.on_connection_established.len(), num_connections);
                        s.behaviour.assert_connected(num_connections, 1);
                    }
                    if [&swarm1, &swarm2]
                        .iter()
                        .all(|s| s.behaviour.on_connection_closed.len() == 1)
                    {
                        let conn_id = swarm2.behaviour.on_connection_closed[0].1;
                        assert_eq!(Some(conn_id), disconnected_conn_id);
                        return Poll::Ready(());
                    }
                }
            }

            if poll1.is_pending() && poll2.is_pending() {
                return Poll::Pending;
            }
        }))
    }

    #[test]
    fn concurrent_dialing() {
        #[derive(Clone, Debug)]
        struct DialConcurrencyFactor(NonZeroU8);

        impl Arbitrary for DialConcurrencyFactor {
            fn arbitrary(g: &mut Gen) -> Self {
                Self(NonZeroU8::new(g.gen_range(1..11)).unwrap())
            }
        }

        fn prop(concurrency_factor: DialConcurrencyFactor) {
            block_on(async {
                let mut swarm = new_test_swarm::<_, ()>(keep_alive::ConnectionHandler)
                    .dial_concurrency_factor(concurrency_factor.0)
                    .build();

                // Listen on `concurrency_factor + 1` addresses.
                //
                // `+ 2` to ensure a subset of addresses is dialed by network_2.
                let num_listen_addrs = concurrency_factor.0.get() + 2;
                let mut listen_addresses = Vec::new();
                let mut transports = Vec::new();
                for _ in 0..num_listen_addrs {
                    let mut transport = transport::MemoryTransport::default().boxed();
                    transport.listen_on("/memory/0".parse().unwrap()).unwrap();

                    match transport.select_next_some().await {
                        TransportEvent::NewAddress { listen_addr, .. } => {
                            listen_addresses.push(listen_addr);
                        }
                        _ => panic!("Expected `NewListenAddr` event."),
                    }

                    transports.push(transport);
                }

                // Have swarm dial each listener and wait for each listener to receive the incoming
                // connections.
                swarm
                    .dial(
                        DialOpts::peer_id(PeerId::random())
                            .addresses(listen_addresses)
                            .build(),
                    )
                    .unwrap();
                for mut transport in transports.into_iter() {
                    loop {
                        match futures::future::select(transport.select_next_some(), swarm.next())
                            .await
                        {
                            future::Either::Left((TransportEvent::Incoming { .. }, _)) => {
                                break;
                            }
                            future::Either::Left(_) => {
                                panic!("Unexpected transport event.")
                            }
                            future::Either::Right((e, _)) => {
                                panic!("Expect swarm to not emit any event {e:?}")
                            }
                        }
                    }
                }

                match swarm.next().await.unwrap() {
                    SwarmEvent::OutgoingConnectionError { .. } => {}
                    e => panic!("Unexpected swarm event {e:?}"),
                }
            })
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_) -> _);
    }

    #[test]
    fn max_outgoing() {
        use rand::Rng;

        let outgoing_limit = rand::thread_rng().gen_range(1..10);

        let limits = ConnectionLimits::default().with_max_pending_outgoing(Some(outgoing_limit));
        let mut network = new_test_swarm::<_, ()>(keep_alive::ConnectionHandler)
            .connection_limits(limits)
            .build();

        let addr: Multiaddr = "/memory/1234".parse().unwrap();

        let target = PeerId::random();
        for _ in 0..outgoing_limit {
            network
                .dial(
                    DialOpts::peer_id(target)
                        .addresses(vec![addr.clone()])
                        .build(),
                )
                .expect("Unexpected connection limit.");
        }

        match network
            .dial(DialOpts::peer_id(target).addresses(vec![addr]).build())
            .expect_err("Unexpected dialing success.")
        {
            DialError::ConnectionLimit(limit) => {
                assert_eq!(limit.current, outgoing_limit);
                assert_eq!(limit.limit, outgoing_limit);
            }
            e => panic!("Unexpected error: {e:?}"),
        }

        let info = network.network_info();
        assert_eq!(info.num_peers(), 0);
        assert_eq!(
            info.connection_counters().num_pending_outgoing(),
            outgoing_limit
        );
    }

    #[test]
    fn max_established_incoming() {
        #[derive(Debug, Clone)]
        struct Limit(u32);

        impl Arbitrary for Limit {
            fn arbitrary(g: &mut Gen) -> Self {
                Self(g.gen_range(1..10))
            }
        }

        fn limits(limit: u32) -> ConnectionLimits {
            ConnectionLimits::default().with_max_established_incoming(Some(limit))
        }

        fn prop(limit: Limit) {
            let limit = limit.0;

            let mut network1 = new_test_swarm::<_, ()>(keep_alive::ConnectionHandler)
                .connection_limits(limits(limit))
                .build();
            let mut network2 = new_test_swarm::<_, ()>(keep_alive::ConnectionHandler)
                .connection_limits(limits(limit))
                .build();

            let _ = network1.listen_on(multiaddr![Memory(0u64)]).unwrap();
            let listen_addr = async_std::task::block_on(poll_fn(|cx| {
                match ready!(network1.poll_next_unpin(cx)).unwrap() {
                    SwarmEvent::NewListenAddr { address, .. } => Poll::Ready(address),
                    e => panic!("Unexpected network event: {e:?}"),
                }
            }));

            // Spawn and block on the dialer.
            async_std::task::block_on({
                let mut n = 0;
                network2.dial(listen_addr.clone()).unwrap();

                let mut expected_closed = false;
                let mut network_1_established = false;
                let mut network_2_established = false;
                let mut network_1_limit_reached = false;
                let mut network_2_limit_reached = false;
                poll_fn(move |cx| {
                    loop {
                        let mut network_1_pending = false;
                        let mut network_2_pending = false;

                        match network1.poll_next_unpin(cx) {
                            Poll::Ready(Some(SwarmEvent::IncomingConnection { .. })) => {}
                            Poll::Ready(Some(SwarmEvent::ConnectionEstablished { .. })) => {
                                network_1_established = true;
                            }
                            Poll::Ready(Some(SwarmEvent::IncomingConnectionError {
                                error: PendingConnectionError::ConnectionLimit(err),
                                ..
                            })) => {
                                assert_eq!(err.limit, limit);
                                assert_eq!(err.limit, err.current);
                                let info = network1.network_info();
                                let counters = info.connection_counters();
                                assert_eq!(counters.num_established_incoming(), limit);
                                assert_eq!(counters.num_established(), limit);
                                network_1_limit_reached = true;
                            }
                            Poll::Pending => {
                                network_1_pending = true;
                            }
                            e => panic!("Unexpected network event: {e:?}"),
                        }

                        match network2.poll_next_unpin(cx) {
                            Poll::Ready(Some(SwarmEvent::ConnectionEstablished { .. })) => {
                                network_2_established = true;
                            }
                            Poll::Ready(Some(SwarmEvent::ConnectionClosed { .. })) => {
                                assert!(expected_closed);
                                let info = network2.network_info();
                                let counters = info.connection_counters();
                                assert_eq!(counters.num_established_outgoing(), limit);
                                assert_eq!(counters.num_established(), limit);
                                network_2_limit_reached = true;
                            }
                            Poll::Pending => {
                                network_2_pending = true;
                            }
                            e => panic!("Unexpected network event: {e:?}"),
                        }

                        if network_1_pending && network_2_pending {
                            return Poll::Pending;
                        }

                        if network_1_established && network_2_established {
                            network_1_established = false;
                            network_2_established = false;

                            if n <= limit {
                                // Dial again until the limit is exceeded.
                                n += 1;
                                network2.dial(listen_addr.clone()).unwrap();

                                if n == limit {
                                    // The the next dialing attempt exceeds the limit, this
                                    // is the connection we expected to get closed.
                                    expected_closed = true;
                                }
                            } else {
                                panic!("Expect networks not to establish connections beyond the limit.")
                            }
                        }

                        if network_1_limit_reached && network_2_limit_reached {
                            return Poll::Ready(());
                        }
                    }
                })
            });
        }

        quickcheck(prop as fn(_));
    }

    #[test]
    fn invalid_peer_id() {
        // Checks whether dialing an address containing the wrong peer id raises an error
        // for the expected peer id instead of the obtained peer id.

        let mut swarm1 = new_test_swarm::<_, ()>(dummy::ConnectionHandler).build();
        let mut swarm2 = new_test_swarm::<_, ()>(dummy::ConnectionHandler).build();

        swarm1.listen_on("/memory/0".parse().unwrap()).unwrap();

        let address =
            futures::executor::block_on(future::poll_fn(|cx| match swarm1.poll_next_unpin(cx) {
                Poll::Ready(Some(SwarmEvent::NewListenAddr { address, .. })) => {
                    Poll::Ready(address)
                }
                Poll::Pending => Poll::Pending,
                _ => panic!("Was expecting the listen address to be reported"),
            }));

        let other_id = PeerId::random();
        let other_addr = address.with(Protocol::P2p(other_id.into()));

        swarm2.dial(other_addr.clone()).unwrap();

        let (peer_id, error) = futures::executor::block_on(future::poll_fn(|cx| {
            if let Poll::Ready(Some(SwarmEvent::IncomingConnection { .. })) =
                swarm1.poll_next_unpin(cx)
            {}

            match swarm2.poll_next_unpin(cx) {
                Poll::Ready(Some(SwarmEvent::OutgoingConnectionError {
                    peer_id, error, ..
                })) => Poll::Ready((peer_id, error)),
                Poll::Ready(x) => panic!("unexpected {x:?}"),
                Poll::Pending => Poll::Pending,
            }
        }));
        assert_eq!(peer_id.unwrap(), other_id);
        match error {
            DialError::WrongPeerId { obtained, endpoint } => {
                assert_eq!(obtained, *swarm1.local_peer_id());
                assert_eq!(
                    endpoint,
                    ConnectedPoint::Dialer {
                        address: other_addr,
                        role_override: Endpoint::Dialer,
                    }
                );
            }
            x => panic!("wrong error {x:?}"),
        }
    }

    #[test]
    fn dial_self() {
        // Check whether dialing ourselves correctly fails.
        //
        // Dialing the same address we're listening should result in three events:
        //
        // - The incoming connection notification (before we know the incoming peer ID).
        // - The connection error for the dialing endpoint (once we've determined that it's our own ID).
        // - The connection error for the listening endpoint (once we've determined that it's our own ID).
        //
        // The last two can happen in any order.

        let mut swarm = new_test_swarm::<_, ()>(dummy::ConnectionHandler).build();
        swarm.listen_on("/memory/0".parse().unwrap()).unwrap();

        let local_address =
            futures::executor::block_on(future::poll_fn(|cx| match swarm.poll_next_unpin(cx) {
                Poll::Ready(Some(SwarmEvent::NewListenAddr { address, .. })) => {
                    Poll::Ready(address)
                }
                Poll::Pending => Poll::Pending,
                _ => panic!("Was expecting the listen address to be reported"),
            }));

        swarm.listened_addrs.clear(); // This is a hack to actually execute the dial to ourselves which would otherwise be filtered.

        swarm.dial(local_address.clone()).unwrap();

        let mut got_dial_err = false;
        let mut got_inc_err = false;
        futures::executor::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
            loop {
                match swarm.poll_next_unpin(cx) {
                    Poll::Ready(Some(SwarmEvent::OutgoingConnectionError {
                        peer_id,
                        error: DialError::WrongPeerId { .. },
                        ..
                    })) => {
                        assert_eq!(&peer_id.unwrap(), swarm.local_peer_id());
                        assert!(!got_dial_err);
                        got_dial_err = true;
                        if got_inc_err {
                            return Poll::Ready(Ok(()));
                        }
                    }
                    Poll::Ready(Some(SwarmEvent::IncomingConnectionError {
                        local_addr, ..
                    })) => {
                        assert!(!got_inc_err);
                        assert_eq!(local_addr, local_address);
                        got_inc_err = true;
                        if got_dial_err {
                            return Poll::Ready(Ok(()));
                        }
                    }
                    Poll::Ready(Some(SwarmEvent::IncomingConnection { local_addr, .. })) => {
                        assert_eq!(local_addr, local_address);
                    }
                    Poll::Ready(ev) => {
                        panic!("Unexpected event: {ev:?}")
                    }
                    Poll::Pending => break Poll::Pending,
                }
            }
        }))
        .unwrap();
    }

    #[test]
    fn dial_self_by_id() {
        // Trying to dial self by passing the same `PeerId` shouldn't even be possible in the first
        // place.
        let swarm = new_test_swarm::<_, ()>(dummy::ConnectionHandler).build();
        let peer_id = *swarm.local_peer_id();
        assert!(!swarm.is_connected(&peer_id));
    }

    #[async_std::test]
    async fn multiple_addresses_err() {
        // Tries dialing multiple addresses, and makes sure there's one dialing error per address.

        let target = PeerId::random();

        let mut swarm = new_test_swarm::<_, ()>(dummy::ConnectionHandler).build();

        let addresses = HashSet::from([
            multiaddr![Ip4([0, 0, 0, 0]), Tcp(rand::random::<u16>())],
            multiaddr![Ip4([0, 0, 0, 0]), Tcp(rand::random::<u16>())],
            multiaddr![Ip4([0, 0, 0, 0]), Tcp(rand::random::<u16>())],
            multiaddr![Udp(rand::random::<u16>())],
            multiaddr![Udp(rand::random::<u16>())],
            multiaddr![Udp(rand::random::<u16>())],
            multiaddr![Udp(rand::random::<u16>())],
            multiaddr![Udp(rand::random::<u16>())],
        ]);

        swarm
            .dial(
                DialOpts::peer_id(target)
                    .addresses(addresses.iter().cloned().collect())
                    .build(),
            )
            .unwrap();

        match swarm.next().await.unwrap() {
            SwarmEvent::OutgoingConnectionError {
                peer_id,
                // multiaddr,
                error: DialError::Transport(errors),
            } => {
                assert_eq!(target, peer_id.unwrap());

                let failed_addresses = errors.into_iter().map(|(addr, _)| addr).collect::<Vec<_>>();
                let expected_addresses = addresses
                    .into_iter()
                    .map(|addr| addr.with(Protocol::P2p(target.into())))
                    .collect::<Vec<_>>();

                assert_eq!(expected_addresses, failed_addresses);
            }
            e => panic!("Unexpected event: {e:?}"),
        }
    }

    #[test]
    fn aborting_pending_connection_surfaces_error() {
        let _ = env_logger::try_init();

        let mut dialer = new_test_swarm::<_, ()>(dummy::ConnectionHandler).build();
        let mut listener = new_test_swarm::<_, ()>(dummy::ConnectionHandler).build();

        let listener_peer_id = *listener.local_peer_id();
        listener.listen_on(multiaddr![Memory(0u64)]).unwrap();
        let listener_address = match block_on(listener.next()).unwrap() {
            SwarmEvent::NewListenAddr { address, .. } => address,
            e => panic!("Unexpected network event: {e:?}"),
        };

        dialer
            .dial(
                DialOpts::peer_id(listener_peer_id)
                    .addresses(vec![listener_address])
                    .build(),
            )
            .unwrap();

        dialer
            .disconnect_peer_id(listener_peer_id)
            .expect_err("Expect peer to not yet be connected.");

        match block_on(dialer.next()).unwrap() {
            SwarmEvent::OutgoingConnectionError {
                error: DialError::Aborted,
                ..
            } => {}
            e => panic!("Unexpected swarm event {e:?}."),
        }
    }

    #[test]
    fn dial_error_prints_sources() {
        // This constitutes a fairly typical error for chained transports.
        let error = DialError::Transport(vec![(
            "/ip4/127.0.0.1/tcp/80".parse().unwrap(),
            TransportError::Other(io::Error::new(
                io::ErrorKind::Other,
                Either::<_, Void>::Left(Either::<Void, _>::Right(UpgradeError::Apply(
                    MemoryTransportError::Unreachable,
                ))),
            )),
        )]);

        let string = format!("{error}");

        // Unfortunately, we have some "empty" errors that lead to multiple colons without text but that is the best we can do.
        assert_eq!("Failed to negotiate transport protocol(s): [(/ip4/127.0.0.1/tcp/80: : Handshake failed: No listener on the given port.)]", string)
    }
}
