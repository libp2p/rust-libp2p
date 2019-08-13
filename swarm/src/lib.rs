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
use futures::prelude::*;
use libp2p_core::{
    Transport, Multiaddr, PeerId, InboundUpgrade, OutboundUpgrade, UpgradeInfo, ProtocolName,
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
use std::{error, fmt, io, ops::{Deref, DerefMut}};
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
    ///
    /// If the pair's second element is `AsyncSink::NotReady`, the event
    /// message has yet to be sent using `PeerMut::start_send_event`.
    ///
    /// If the pair's second element is `AsyncSink::Ready`, the event
    /// message has been sent and needs to be flushed using
    /// `PeerMut::complete_send_event`.
    send_event_to_complete: Option<(PeerId, AsyncSink<TInEvent>)>
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
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Error: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Error: Send + 'static,
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
    pub fn remove_listener(me: &mut Self, id: ListenerId) -> Option<TTransport::Listener> {
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
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Error: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Error: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<THandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <NodeHandlerWrapper<<THandler as IntoProtocolsHandler>::Handler> as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      TConnInfo: ConnectionInfo<PeerId = PeerId> + fmt::Debug + Clone + Send + 'static,
{
    type Item = TBehaviour::OutEvent;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
        loop {
            let mut network_not_ready = false;

            match self.network.poll() {
                Async::NotReady => network_not_ready = true,
                Async::Ready(NetworkEvent::NodeEvent { conn_info, event }) => {
                    self.behaviour.inject_node_event(conn_info.peer_id().clone(), event);
                },
                Async::Ready(NetworkEvent::Connected { conn_info, endpoint }) => {
                    if self.banned_peers.contains(conn_info.peer_id()) {
                        self.network.peer(conn_info.peer_id().clone())
                            .into_connected()
                            .expect("the Network just notified us that we were connected; QED")
                            .close();
                    } else {
                        self.behaviour.inject_connected(conn_info.peer_id().clone(), endpoint);
                    }
                },
                Async::Ready(NetworkEvent::NodeClosed { conn_info, endpoint, .. }) => {
                    self.behaviour.inject_disconnected(conn_info.peer_id(), endpoint);
                },
                Async::Ready(NetworkEvent::Replaced { new_info, closed_endpoint, endpoint, .. }) => {
                    self.behaviour.inject_replaced(new_info.peer_id().clone(), closed_endpoint, endpoint);
                },
                Async::Ready(NetworkEvent::IncomingConnection(incoming)) => {
                    let handler = self.behaviour.new_handler();
                    incoming.accept(handler.into_node_handler_builder());
                },
                Async::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) => {
                    if !self.listened_addrs.contains(&listen_addr) {
                        self.listened_addrs.push(listen_addr.clone())
                    }
                    self.behaviour.inject_new_listen_addr(&listen_addr);
                }
                Async::Ready(NetworkEvent::ExpiredListenerAddress { listen_addr, .. }) => {
                    self.listened_addrs.retain(|a| a != &listen_addr);
                    self.behaviour.inject_expired_listen_addr(&listen_addr);
                }
                Async::Ready(NetworkEvent::ListenerClosed { listener_id, .. }) =>
                    self.behaviour.inject_listener_closed(listener_id),
                Async::Ready(NetworkEvent::ListenerError { listener_id, error }) =>
                    self.behaviour.inject_listener_error(listener_id, &error),
                Async::Ready(NetworkEvent::IncomingConnectionError { .. }) => {},
                Async::Ready(NetworkEvent::DialError { peer_id, multiaddr, error, new_state }) => {
                    self.behaviour.inject_addr_reach_failure(Some(&peer_id), &multiaddr, &error);
                    if let network::PeerState::NotConnected = new_state {
                        self.behaviour.inject_dial_failure(&peer_id);
                    }
                },
                Async::Ready(NetworkEvent::UnknownPeerDialError { multiaddr, error, .. }) => {
                    self.behaviour.inject_addr_reach_failure(None, &multiaddr, &error);
                },
            }

            // Try to deliver pending event.
            if let Some((id, pending)) = self.send_event_to_complete.take() {
                if let Some(mut peer) = self.network.peer(id.clone()).into_connected() {
                    if let AsyncSink::NotReady(e) = pending {
                        if let Ok(a@AsyncSink::NotReady(_)) = peer.start_send_event(e) {
                            self.send_event_to_complete = Some((id, a))
                        } else if let Ok(Async::NotReady) = peer.complete_send_event() {
                            self.send_event_to_complete = Some((id, AsyncSink::Ready))
                        }
                    } else if let Ok(Async::NotReady) = peer.complete_send_event() {
                        self.send_event_to_complete = Some((id, AsyncSink::Ready))
                    }
                }
            }
            if self.send_event_to_complete.is_some() {
                return Ok(Async::NotReady)
            }

            let behaviour_poll = {
                let mut parameters = SwarmPollParameters {
                    local_peer_id: &mut self.network.local_peer_id(),
                    supported_protocols: &self.supported_protocols,
                    listened_addrs: &self.listened_addrs,
                    external_addrs: &self.external_addrs
                };
                self.behaviour.poll(&mut parameters)
            };

            match behaviour_poll {
                Async::NotReady if network_not_ready => return Ok(Async::NotReady),
                Async::NotReady => (),
                Async::Ready(NetworkBehaviourAction::GenerateEvent(event)) => {
                    return Ok(Async::Ready(Some(event)))
                },
                Async::Ready(NetworkBehaviourAction::DialAddress { address }) => {
                    let _ = ExpandedSwarm::dial_addr(self, address);
                },
                Async::Ready(NetworkBehaviourAction::DialPeer { peer_id }) => {
                    if self.banned_peers.contains(&peer_id) {
                        self.behaviour.inject_dial_failure(&peer_id);
                    } else {
                        ExpandedSwarm::dial(self, peer_id);
                    }
                },
                Async::Ready(NetworkBehaviourAction::SendEvent { peer_id, event }) => {
                    if let Some(mut peer) = self.network.peer(peer_id.clone()).into_connected() {
                        if let Ok(a@AsyncSink::NotReady(_)) = peer.start_send_event(event) {
                            self.send_event_to_complete = Some((peer_id, a))
                        } else if let Ok(Async::NotReady) = peer.complete_send_event() {
                            self.send_event_to_complete = Some((peer_id, AsyncSink::Ready))
                        }
                    }
                },
                Async::Ready(NetworkBehaviourAction::ReportObservedAddr { address }) => {
                    for addr in self.network.address_translation(&address) {
                        if self.external_addrs.iter().all(|a| *a != addr) {
                            self.behaviour.inject_new_external_addr(&addr);
                        }
                        self.external_addrs.add(addr)
                    }
                },
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
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Error: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Error: Send + 'static,
      <NodeHandlerWrapper<<<TBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler> as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      TConnInfo: ConnectionInfo<PeerId = PeerId> + fmt::Debug + Clone + Send + 'static,
{
    pub fn new(transport: TTransport, behaviour: TBehaviour, local_peer_id: PeerId) -> Self {
        SwarmBuilder {
            incoming_limit: None,
            local_peer_id,
            transport,
            behaviour,
        }
    }

    pub fn incoming_limit(mut self, incoming_limit: Option<u32>) -> Self {
        self.incoming_limit = incoming_limit;
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

        let network = Network::new_with_incoming_limit(self.transport, self.local_peer_id, self.incoming_limit);

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
    use std::marker::PhantomData;
    use tokio_io::{AsyncRead, AsyncWrite};
    use void::Void;

    #[derive(Clone)]
    struct DummyBehaviour<TSubstream> {
        marker: PhantomData<TSubstream>,
    }

    trait TSubstream: AsyncRead + AsyncWrite {}

    impl<TSubstream> NetworkBehaviour
        for DummyBehaviour<TSubstream>
        where TSubstream: AsyncRead + AsyncWrite
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

        fn poll(&mut self, _: &mut impl PollParameters) ->
            Async<NetworkBehaviourAction<<Self::ProtocolsHandler as
            ProtocolsHandler>::InEvent, Self::OutEvent>>
        {
            Async::NotReady
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
