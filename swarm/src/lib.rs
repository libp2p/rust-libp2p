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

//! High level manager of the network.
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
//! The [`ProtocolsHandler`] trait defines how each active connection to a
//! remote should behave: how to handle incoming substreams, which protocols
//! are supported, when to open a new outbound substream, etc.
//!

mod behaviour;
mod registry;

pub mod protocols_handler;
pub mod toggle;

pub use behaviour::{
    NetworkBehaviour,
    NetworkBehaviourAction,
    NetworkBehaviourEventProcess,
    PollParameters
};
pub use protocols_handler::{
    IntoProtocolsHandler,
    IntoProtocolsHandlerSelect,
    KeepAlive,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerSelect,
    ProtocolsHandlerUpgrErr,
    OneShotHandler,
    SubstreamProtocol
};

use protocols_handler::{NodeHandlerWrapperBuilder, NodeHandlerWrapper, NodeHandlerWrapperError};
use futures::{prelude::*, executor::{ThreadPool, ThreadPoolBuilder}, future::FutureObj};
use libp2p_core::{
    Executor,
    Transport, Multiaddr, Negotiated, PeerId, InboundUpgrade, OutboundUpgrade, UpgradeInfo, ProtocolName,
    muxing::StreamMuxer,
    nodes::{
        ListenerId,
        collection::ConnectionInfo,
        handled_node::NodeHandler,
        node::Substream,
        network::{self, Network, NetworkEvent}
    },
    transport::TransportError
};
use registry::{Addresses, AddressIntoIter};
use smallvec::SmallVec;
use std::{error, fmt, ops::{Deref, DerefMut}, pin::Pin, task::{Context, Poll}};
use std::collections::HashSet;

/// Contains the state of the network, plus the way it should behave.
pub type Swarm<TTransport, TBehaviour, TConnInfo = PeerId> = ExpandedSwarm<
    TTransport,
    TBehaviour,
    <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent,
    <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
    <TBehaviour as NetworkBehaviour>::ProtocolsHandler,
    <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::Error,
    TConnInfo,
>;

/// Event generated by the `Swarm`.
#[derive(Debug)]
pub enum SwarmEvent<TBvEv> {
    /// Event generated by the `NetworkBehaviour`.
    Behaviour(TBvEv),
    /// We are now connected to the given peer.
    Connected(PeerId),
    /// We are now disconnected from the given peer.
    Disconnected(PeerId),
    /// One of our listeners has reported a new local listening address.
    NewListenAddr(Multiaddr),
    /// One of our listeners has reported the expiration of a listening address.
    ExpiredListenAddr(Multiaddr),
    /// Tried to dial an address but it ended up being unreachaable.
    UnreachableAddr {
        /// `PeerId` that we were trying to reach. `None` if we don't know in advance which peer
        /// we were trying to reach.
        peer_id: Option<PeerId>,
        /// Address that we failed to reach.
        address: Multiaddr,
        /// Error that has been encountered.
        error: Box<dyn error::Error + Send>,
    },
    /// Startng to try to reach the given peer.
    StartConnect(PeerId),
}

/// Contains the state of the network, plus the way it should behave.
pub struct ExpandedSwarm<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo = PeerId>
where
    TTransport: Transport,
{
    network: Network<
        TTransport,
        TInEvent,
        TOutEvent,
        NodeHandlerWrapperBuilder<THandler>,
        NodeHandlerWrapperError<THandlerErr>,
        TConnInfo,
        PeerId,
    >,

    /// Handles which nodes to connect to and how to handle the events sent back by the protocol
    /// handlers.
    behaviour: TBehaviour,

    /// List of protocols that the behaviour says it supports.
    supported_protocols: SmallVec<[Vec<u8>; 16]>,

    /// List of multiaddresses we're listening on.
    listened_addrs: SmallVec<[Multiaddr; 8]>,

    /// List of multiaddresses we're listening on, after account for external IP addresses and
    /// similar mechanisms.
    external_addrs: Addresses,

    /// List of nodes for which we deny any incoming connection.
    banned_peers: HashSet<PeerId>,

    /// Pending event message to be delivered.
    send_event_to_complete: Option<(PeerId, TInEvent)>
}

impl<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo> Deref for
    ExpandedSwarm<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo>
where
    TTransport: Transport,
{
    type Target = TBehaviour;

    fn deref(&self) -> &Self::Target {
        &self.behaviour
    }
}

impl<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo> DerefMut for
    ExpandedSwarm<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo>
where
    TTransport: Transport,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.behaviour
    }
}

impl<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo> Unpin for
    ExpandedSwarm<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo>
where
    TTransport: Transport,
{
}

impl<TTransport, TBehaviour, TMuxer, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo>
    ExpandedSwarm<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo>
where TBehaviour: NetworkBehaviour<ProtocolsHandler = THandler>,
      TMuxer: StreamMuxer + Send + Sync + 'static,
      <TMuxer as StreamMuxer>::OutboundSubstream: Send + 'static,
      <TMuxer as StreamMuxer>::Substream: Send + 'static,
      TTransport: Transport<Output = (TConnInfo, TMuxer)> + Clone,
      TTransport::Error: Send + 'static,
      TTransport::Listener: Send + 'static,
      TTransport::ListenerUpgrade: Send + 'static,
      TTransport::Dial: Send + 'static,
      THandlerErr: error::Error,
      THandler: IntoProtocolsHandler + Send + 'static,
      <THandler as IntoProtocolsHandler>::Handler: ProtocolsHandler<InEvent = TInEvent, OutEvent = TOutEvent, Substream = Substream<TMuxer>, Error = THandlerErr> + Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::Error: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Negotiated<Substream<TMuxer>>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Negotiated<Substream<TMuxer>>>>::Error: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Negotiated<Substream<TMuxer>>>>::Future: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Negotiated<Substream<TMuxer>>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Negotiated<Substream<TMuxer>>>>::Future: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Negotiated<Substream<TMuxer>>>>::Error: Send + 'static,
      <NodeHandlerWrapper<<THandler as IntoProtocolsHandler>::Handler> as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      TConnInfo: ConnectionInfo<PeerId = PeerId> + fmt::Debug + Clone + Send + 'static,
{
    /// Builds a new `Swarm`.
    pub fn new(transport: TTransport, behaviour: TBehaviour, local_peer_id: PeerId) -> Self {
        SwarmBuilder::new(transport, behaviour, local_peer_id)
            .build()
    }

    /// Returns the transport passed when building this object.
    pub fn transport(me: &Self) -> &TTransport {
        me.network.transport()
    }

    /// Starts listening on the given address.
    ///
    /// Returns an error if the address is not supported.
    pub fn listen_on(me: &mut Self, addr: Multiaddr) -> Result<ListenerId, TransportError<TTransport::Error>> {
        me.network.listen_on(addr)
    }

    /// Remove some listener.
    ///
    /// Returns `Ok(())` if there was a listener with this ID.
    pub fn remove_listener(me: &mut Self, id: ListenerId) -> Result<(), ()> {
        me.network.remove_listener(id)
    }

    /// Tries to dial the given address.
    ///
    /// Returns an error if the address is not supported.
    pub fn dial_addr(me: &mut Self, addr: Multiaddr) -> Result<(), TransportError<TTransport::Error>> {
        let handler = me.behaviour.new_handler();
        me.network.dial(addr, handler.into_node_handler_builder())
    }

    /// Tries to reach the given peer using the elements in the topology.
    ///
    /// Has no effect if we are already connected to that peer, or if no address is known for the
    /// peer.
    pub fn dial(me: &mut Self, peer_id: PeerId) {
        let addrs = me.behaviour.addresses_of_peer(&peer_id);
        match me.network.peer(peer_id.clone()) {
            network::Peer::NotConnected(peer) => {
                let handler = me.behaviour.new_handler().into_node_handler_builder();
                if peer.connect_iter(addrs, handler).is_err() {
                    me.behaviour.inject_dial_failure(&peer_id);
                }
            },
            network::Peer::PendingConnect(mut peer) => {
                peer.append_multiaddr_attempts(addrs)
            },
            network::Peer::Connected(_) | network::Peer::LocalNode => {}
        }
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    pub fn listeners(me: &Self) -> impl Iterator<Item = &Multiaddr> {
        me.network.listen_addrs()
    }

    /// Returns an iterator that produces the list of addresses that other nodes can use to reach
    /// us.
    pub fn external_addresses(me: &Self) -> impl Iterator<Item = &Multiaddr> {
        me.external_addrs.iter()
    }

    /// Returns the peer ID of the swarm passed as parameter.
    pub fn local_peer_id(me: &Self) -> &PeerId {
        &me.network.local_peer_id()
    }

    /// Adds an external address.
    ///
    /// An external address is an address we are listening on but that accounts for things such as
    /// NAT traversal.
    pub fn add_external_address(me: &mut Self, addr: Multiaddr) {
        me.external_addrs.add(addr)
    }

    /// Returns the connection info of a node, or `None` if we're not connected to it.
    // TODO: should take &self instead of &mut self, but the API in network requires &mut
    pub fn connection_info(me: &mut Self, peer_id: &PeerId) -> Option<TConnInfo> {
        if let Some(mut n) = me.network.peer(peer_id.clone()).into_connected() {
            Some(n.connection_info().clone())
        } else {
            None
        }
    }

    /// Bans a peer by its peer ID.
    ///
    /// Any incoming connection and any dialing attempt will immediately be rejected.
    /// This function has no effect is the peer is already banned.
    pub fn ban_peer_id(me: &mut Self, peer_id: PeerId) {
        me.banned_peers.insert(peer_id.clone());
        if let Some(c) = me.network.peer(peer_id).into_connected() {
            c.close();
        }
    }

    /// Unbans a peer.
    pub fn unban_peer_id(me: &mut Self, peer_id: PeerId) {
        me.banned_peers.remove(&peer_id);
    }

    /// Returns the next event that happens in the `Swarm`.
    ///
    /// Includes events from the `NetworkBehaviour` but also events about the connections status.
    pub async fn next_event(&mut self) -> SwarmEvent<TBehaviour::OutEvent> {
        future::poll_fn(move |cx| ExpandedSwarm::poll_next_event(Pin::new(self), cx)).await
    }

    /// Returns the next event produced by the [`NetworkBehaviour`].
    pub async fn next(&mut self) -> TBehaviour::OutEvent {
        future::poll_fn(move |cx| {
            loop {
                let event = futures::ready!(ExpandedSwarm::poll_next_event(Pin::new(self), cx));
                if let SwarmEvent::Behaviour(event) = event {
                    return Poll::Ready(event);
                }
            }
        }).await
    }

    /// Internal function used by everything event-related.
    ///
    /// Polls the `Swarm` for the next event.
    fn poll_next_event(mut self: Pin<&mut Self>, cx: &mut Context)
        -> Poll<SwarmEvent<TBehaviour::OutEvent>>
    {
        // We use a `this` variable because the compiler can't mutably borrow multiple times
        // across a `Deref`.
        let this = &mut *self;

        loop {
            let mut network_not_ready = false;

            match this.network.poll(cx) {
                Poll::Pending => network_not_ready = true,
                Poll::Ready(NetworkEvent::NodeEvent { conn_info, event }) => {
                    this.behaviour.inject_node_event(conn_info.peer_id().clone(), event);
                },
                Poll::Ready(NetworkEvent::Connected { conn_info, endpoint }) => {
                    if this.banned_peers.contains(conn_info.peer_id()) {
                        this.network.peer(conn_info.peer_id().clone())
                            .into_connected()
                            .expect("the Network just notified us that we were connected; QED")
                            .close();
                    } else {
                        this.behaviour.inject_connected(conn_info.peer_id().clone(), endpoint);
                        return Poll::Ready(SwarmEvent::Connected(conn_info.peer_id().clone()));
                    }
                },
                Poll::Ready(NetworkEvent::NodeClosed { conn_info, endpoint, .. }) => {
                    this.behaviour.inject_disconnected(conn_info.peer_id(), endpoint);
                    return Poll::Ready(SwarmEvent::Disconnected(conn_info.peer_id().clone()));
                },
                Poll::Ready(NetworkEvent::Replaced { new_info, closed_endpoint, endpoint, .. }) => {
                    this.behaviour.inject_replaced(new_info.peer_id().clone(), closed_endpoint, endpoint);
                },
                Poll::Ready(NetworkEvent::IncomingConnection(incoming)) => {
                    let handler = this.behaviour.new_handler();
                    incoming.accept(handler.into_node_handler_builder());
                },
                Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) => {
                    if !this.listened_addrs.contains(&listen_addr) {
                        this.listened_addrs.push(listen_addr.clone())
                    }
                    this.behaviour.inject_new_listen_addr(&listen_addr);
                    return Poll::Ready(SwarmEvent::NewListenAddr(listen_addr));
                }
                Poll::Ready(NetworkEvent::ExpiredListenerAddress { listen_addr, .. }) => {
                    this.listened_addrs.retain(|a| a != &listen_addr);
                    this.behaviour.inject_expired_listen_addr(&listen_addr);
                    return Poll::Ready(SwarmEvent::ExpiredListenAddr(listen_addr));
                }
                Poll::Ready(NetworkEvent::ListenerClosed { listener_id, .. }) =>
                    this.behaviour.inject_listener_closed(listener_id),
                Poll::Ready(NetworkEvent::ListenerError { listener_id, error }) =>
                    this.behaviour.inject_listener_error(listener_id, &error),
                Poll::Ready(NetworkEvent::IncomingConnectionError { .. }) => {},
                Poll::Ready(NetworkEvent::DialError { peer_id, multiaddr, error, new_state }) => {
                    this.behaviour.inject_addr_reach_failure(Some(&peer_id), &multiaddr, &error);
                    if let network::PeerState::NotConnected = new_state {
                        this.behaviour.inject_dial_failure(&peer_id);
                    }
                    return Poll::Ready(SwarmEvent::UnreachableAddr {
                        peer_id: Some(peer_id.clone()),
                        address: multiaddr,
                        error: Box::new(error),
                    });
                },
                Poll::Ready(NetworkEvent::UnknownPeerDialError { multiaddr, error, .. }) => {
                    this.behaviour.inject_addr_reach_failure(None, &multiaddr, &error);
                    return Poll::Ready(SwarmEvent::UnreachableAddr {
                        peer_id: None,
                        address: multiaddr,
                        error: Box::new(error),
                    });
                },
            }

            // Try to deliver pending event.
            if let Some((id, pending)) = this.send_event_to_complete.take() {
                if let Some(mut peer) = this.network.peer(id.clone()).into_connected() {
                    match peer.poll_ready_event(cx) {
                        Poll::Ready(()) => peer.start_send_event(pending),
                        Poll::Pending => {
                            this.send_event_to_complete = Some((id, pending));
                            return Poll::Pending
                        },
                    }
                }
            }

            let behaviour_poll = {
                let mut parameters = SwarmPollParameters {
                    local_peer_id: &mut this.network.local_peer_id(),
                    supported_protocols: &this.supported_protocols,
                    listened_addrs: &this.listened_addrs,
                    external_addrs: &this.external_addrs
                };
                this.behaviour.poll(cx, &mut parameters)
            };

            match behaviour_poll {
                Poll::Pending if network_not_ready => return Poll::Pending,
                Poll::Pending => (),
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(event)) => {
                    return Poll::Ready(SwarmEvent::Behaviour(event))
                },
                Poll::Ready(NetworkBehaviourAction::DialAddress { address }) => {
                    let _ = ExpandedSwarm::dial_addr(&mut *this, address);
                },
                Poll::Ready(NetworkBehaviourAction::DialPeer { peer_id }) => {
                    if this.banned_peers.contains(&peer_id) {
                        this.behaviour.inject_dial_failure(&peer_id);
                    } else {
                        ExpandedSwarm::dial(&mut *this, peer_id.clone());
                        return Poll::Ready(SwarmEvent::StartConnect(peer_id))
                    }
                },
                Poll::Ready(NetworkBehaviourAction::SendEvent { peer_id, event }) => {
                    if let Some(mut peer) = this.network.peer(peer_id.clone()).into_connected() {
                        if let Poll::Ready(()) = peer.poll_ready_event(cx) {
                            peer.start_send_event(event);
                        } else {
                            debug_assert!(this.send_event_to_complete.is_none());
                            this.send_event_to_complete = Some((peer_id, event));
                            return Poll::Pending;
                        }
                    }
                },
                Poll::Ready(NetworkBehaviourAction::ReportObservedAddr { address }) => {
                    for addr in this.network.address_translation(&address) {
                        if this.external_addrs.iter().all(|a| *a != addr) {
                            this.behaviour.inject_new_external_addr(&addr);
                        }
                        this.external_addrs.add(addr);
                    }
                },
            }
        }
    }
}

impl<TTransport, TBehaviour, TMuxer, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo> Stream for
    ExpandedSwarm<TTransport, TBehaviour, TInEvent, TOutEvent, THandler, THandlerErr, TConnInfo>
where TBehaviour: NetworkBehaviour<ProtocolsHandler = THandler>,
      TMuxer: StreamMuxer + Send + Sync + 'static,
      <TMuxer as StreamMuxer>::OutboundSubstream: Send + 'static,
      <TMuxer as StreamMuxer>::Substream: Send + 'static,
      TTransport: Transport<Output = (TConnInfo, TMuxer)> + Clone,
      TTransport::Error: Send + 'static,
      TTransport::Listener: Send + 'static,
      TTransport::ListenerUpgrade: Send + 'static,
      TTransport::Dial: Send + 'static,
      THandlerErr: error::Error,
      THandler: IntoProtocolsHandler + Send + 'static,
      <THandler as IntoProtocolsHandler>::Handler: ProtocolsHandler<InEvent = TInEvent, OutEvent = TOutEvent, Substream = Substream<TMuxer>, Error = THandlerErr> + Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::Error: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Negotiated<Substream<TMuxer>>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Negotiated<Substream<TMuxer>>>>::Future: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Negotiated<Substream<TMuxer>>>>::Error: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Negotiated<Substream<TMuxer>>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Negotiated<Substream<TMuxer>>>>::Future: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Negotiated<Substream<TMuxer>>>>::Error: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <NodeHandlerWrapper<<THandler as IntoProtocolsHandler>::Handler> as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      TConnInfo: ConnectionInfo<PeerId = PeerId> + fmt::Debug + Clone + Send + 'static,
{
    type Item = TBehaviour::OutEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            let event = futures::ready!(ExpandedSwarm::poll_next_event(self.as_mut(), cx));
            if let SwarmEvent::Behaviour(event) = event {
                return Poll::Ready(Some(event));
            }
        }
    }
}

/// Parameters passed to `poll()`, that the `NetworkBehaviour` has access to.
// TODO: #[derive(Debug)]
pub struct SwarmPollParameters<'a> {
    local_peer_id: &'a PeerId,
    supported_protocols: &'a [Vec<u8>],
    listened_addrs: &'a [Multiaddr],
    external_addrs: &'a Addresses,
}

impl<'a> PollParameters for SwarmPollParameters<'a> {
    type SupportedProtocolsIter = std::vec::IntoIter<Vec<u8>>;
    type ListenedAddressesIter = std::vec::IntoIter<Multiaddr>;
    type ExternalAddressesIter = AddressIntoIter;

    fn supported_protocols(&self) -> Self::SupportedProtocolsIter {
        self.supported_protocols.to_vec().into_iter()
    }

    fn listened_addresses(&self) -> Self::ListenedAddressesIter {
        self.listened_addrs.to_vec().into_iter()
    }

    fn external_addresses(&self) -> Self::ExternalAddressesIter {
        self.external_addrs.clone().into_iter()
    }

    fn local_peer_id(&self) -> &PeerId {
        self.local_peer_id
    }
}

pub struct SwarmBuilder<TTransport, TBehaviour> {
    incoming_limit: Option<u32>,
    executor: Option<Box<dyn Executor>>,
    local_peer_id: PeerId,
    transport: TTransport,
    behaviour: TBehaviour,
}

impl<TTransport, TBehaviour, TMuxer, TConnInfo> SwarmBuilder<TTransport, TBehaviour>
where TBehaviour: NetworkBehaviour,
      TMuxer: StreamMuxer + Send + Sync + 'static,
      <TMuxer as StreamMuxer>::OutboundSubstream: Send + 'static,
      <TMuxer as StreamMuxer>::Substream: Send + 'static,
      TTransport: Transport<Output = (TConnInfo, TMuxer)> + Clone,
      TTransport::Error: Send + 'static,
      TTransport::Listener: Send + 'static,
      TTransport::ListenerUpgrade: Send + 'static,
      TTransport::Dial: Send + 'static,
      <TBehaviour as NetworkBehaviour>::ProtocolsHandler: Send + 'static,
      <<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler: ProtocolsHandler<Substream = Substream<TMuxer>> + Send + 'static,
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent: Send + 'static,
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent: Send + 'static,
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::Error: Send + 'static,
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Negotiated<Substream<TMuxer>>> + Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Negotiated<Substream<TMuxer>>>>::Error: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Negotiated<Substream<TMuxer>>>>::Future: Send + 'static,
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Negotiated<Substream<TMuxer>>> + Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Negotiated<Substream<TMuxer>>>>::Future: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Negotiated<Substream<TMuxer>>>>::Error: Send + 'static,
      <NodeHandlerWrapper<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler> as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      TConnInfo: ConnectionInfo<PeerId = PeerId> + fmt::Debug + Clone + Send + 'static,
{
    pub fn new(transport: TTransport, behaviour: TBehaviour, local_peer_id: PeerId) -> Self {
        SwarmBuilder {
            incoming_limit: None,
            local_peer_id,
            executor: None,
            transport,
            behaviour,
        }
    }

    pub fn incoming_limit(mut self, incoming_limit: Option<u32>) -> Self {
        self.incoming_limit = incoming_limit;
        self
    }

    /// Sets the executor to use to spawn background tasks.
    ///
    /// By default, uses a threads pool.
    pub fn executor(mut self, executor: impl Executor + 'static) -> Self {
        self.executor = Some(Box::new(executor));
        self
    }

    /// Shortcut for calling `executor` with an object that calls the given closure.
    pub fn executor_fn(mut self, executor: impl Fn(FutureObj<'static, ()>) + 'static) -> Self {
        struct SpawnImpl<F>(F);
        impl<F: Fn(FutureObj<'static, ()>)> Executor for SpawnImpl<F> {
            fn exec(&self, f: FutureObj<'static, ()>) {
                (self.0)(f)
            }
        }
        self.executor = Some(Box::new(SpawnImpl(executor)));
        self
    }

    pub fn build(mut self) -> Swarm<TTransport, TBehaviour, TConnInfo> {
        let supported_protocols = self.behaviour
            .new_handler()
            .inbound_protocol()
            .protocol_info()
            .into_iter()
            .map(|info| info.protocol_name().to_vec())
            .collect();

        let executor = self.executor.or_else(|| {
            struct PoolWrapper(ThreadPool);
            impl Executor for PoolWrapper {
                fn exec(&self, f: FutureObj<'static, ()>) {
                    self.0.spawn_ok(f)
                }
            }

            ThreadPoolBuilder::new()
                .name_prefix("libp2p-task-")
                .create()
                .ok()
                .map(|tp| Box::new(PoolWrapper(tp)) as Box<_>)
        });

        let network = Network::new_with_incoming_limit(
            self.transport,
            self.local_peer_id,
            executor,
            self.incoming_limit
        );

        ExpandedSwarm {
            network,
            behaviour: self.behaviour,
            supported_protocols,
            listened_addrs: SmallVec::new(),
            external_addrs: Addresses::default(),
            banned_peers: HashSet::new(),
            send_event_to_complete: None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::protocols_handler::{DummyProtocolsHandler, ProtocolsHandler};
    use crate::{NetworkBehaviour, NetworkBehaviourAction, PollParameters, SwarmBuilder};
    use libp2p_core::{
        ConnectedPoint,
        identity,
        Multiaddr,
        PeerId,
        PublicKey,
        transport::dummy::{DummyStream, DummyTransport}
    };
    use libp2p_mplex::Multiplex;
    use futures::prelude::*;
    use std::{marker::PhantomData, task::Context, task::Poll};
    use void::Void;

    #[derive(Clone)]
    struct DummyBehaviour<TSubstream> {
        marker: PhantomData<TSubstream>,
    }

    impl<TSubstream> NetworkBehaviour
        for DummyBehaviour<TSubstream>
        where TSubstream: AsyncRead + AsyncWrite + Unpin
    {
        type ProtocolsHandler = DummyProtocolsHandler<TSubstream>;
        type OutEvent = Void;

        fn new_handler(&mut self) -> Self::ProtocolsHandler {
            DummyProtocolsHandler::default()
        }

        fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
            Vec::new()
        }

        fn inject_connected(&mut self, _: PeerId, _: ConnectedPoint) {}

        fn inject_disconnected(&mut self, _: &PeerId, _: ConnectedPoint) {}

        fn inject_node_event(&mut self, _: PeerId,
            _: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent) {}

        fn poll(&mut self, _: &mut Context, _: &mut impl PollParameters) ->
            Poll<NetworkBehaviourAction<<Self::ProtocolsHandler as
            ProtocolsHandler>::InEvent, Self::OutEvent>>
        {
            Poll::Pending
        }

    }

    fn get_random_id() -> PublicKey {
        identity::Keypair::generate_ed25519().public()
    }

    #[test]
    fn test_build_swarm() {
        let id = get_random_id();
        let transport = DummyTransport::<(PeerId, Multiplex<DummyStream>)>::new();
        let behaviour = DummyBehaviour{marker: PhantomData};
        let swarm = SwarmBuilder::new(transport, behaviour, id.into())
            .incoming_limit(Some(4)).build();
        assert_eq!(swarm.network.incoming_limit(), Some(4));
    }

    #[test]
    fn test_build_swarm_with_max_listeners_none() {
        let id = get_random_id();
        let transport = DummyTransport::<(PeerId, Multiplex<DummyStream>)>::new();
        let behaviour = DummyBehaviour{marker: PhantomData};
        let swarm = SwarmBuilder::new(transport, behaviour, id.into()).build();
        assert!(swarm.network.incoming_limit().is_none())
    }
}
