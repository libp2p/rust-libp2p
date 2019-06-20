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

use crate::{
    Transport, Multiaddr, PeerId, InboundUpgrade, OutboundUpgrade, UpgradeInfo, ProtocolName,
    muxing::StreamMuxer,
    nodes::{
        collection::ConnectionInfo,
        handled_node::NodeHandler,
        node::Substream,
        raw_swarm::{self, RawSwarm, RawSwarmEvent}
    },
    protocols_handler::{NodeHandlerWrapperBuilder, NodeHandlerWrapper, NodeHandlerWrapperError, IntoProtocolsHandler, ProtocolsHandler},
    swarm::{PollParameters, NetworkBehaviour, NetworkBehaviourAction, registry::{Addresses, AddressIntoIter}},
    transport::TransportError,
};
use futures::prelude::*;
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
    raw_swarm: RawSwarm<
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
        me.raw_swarm.transport()
    }

    /// Starts listening on the given address.
    ///
    /// Returns an error if the address is not supported.
    pub fn listen_on(me: &mut Self, addr: Multiaddr) -> Result<(), TransportError<TTransport::Error>> {
        me.raw_swarm.listen_on(addr)
    }

    /// Tries to dial the given address.
    ///
    /// Returns an error if the address is not supported.
    pub fn dial_addr(me: &mut Self, addr: Multiaddr) -> Result<(), TransportError<TTransport::Error>> {
        let handler = me.behaviour.new_handler();
        me.raw_swarm.dial(addr, handler.into_node_handler_builder())
    }

    /// Tries to reach the given peer using the elements in the topology.
    ///
    /// Has no effect if we are already connected to that peer, or if no address is known for the
    /// peer.
    pub fn dial(me: &mut Self, peer_id: PeerId) {
        let addrs = me.behaviour.addresses_of_peer(&peer_id);
        match me.raw_swarm.peer(peer_id.clone()) {
            raw_swarm::Peer::NotConnected(peer) => {
                let handler = me.behaviour.new_handler().into_node_handler_builder();
                if peer.connect_iter(addrs, handler).is_err() {
                    me.behaviour.inject_dial_failure(&peer_id);
                }
            },
            raw_swarm::Peer::PendingConnect(mut peer) => {
                peer.append_multiaddr_attempts(addrs)
            },
            raw_swarm::Peer::Connected(_) | raw_swarm::Peer::LocalNode => {}
        }
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    pub fn listeners(me: &Self) -> impl Iterator<Item = &Multiaddr> {
        me.raw_swarm.listen_addrs()
    }

    /// Returns an iterator that produces the list of addresses that other nodes can use to reach
    /// us.
    pub fn external_addresses(me: &Self) -> impl Iterator<Item = &Multiaddr> {
        me.external_addrs.iter()
    }

    /// Returns the peer ID of the swarm passed as parameter.
    pub fn local_peer_id(me: &Self) -> &PeerId {
        &me.raw_swarm.local_peer_id()
    }

    /// Adds an external address.
    ///
    /// An external address is an address we are listening on but that accounts for things such as
    /// NAT traversal.
    pub fn add_external_address(me: &mut Self, addr: Multiaddr) {
        me.external_addrs.add(addr)
    }

    /// Returns the connection info of a node, or `None` if we're not connected to it.
    // TODO: should take &self instead of &mut self, but the API in raw_swarm requires &mut
    pub fn connection_info(me: &mut Self, peer_id: &PeerId) -> Option<TConnInfo> {
        if let Some(mut n) = me.raw_swarm.peer(peer_id.clone()).into_connected() {
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
        if let Some(c) = me.raw_swarm.peer(peer_id).into_connected() {
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
            let mut raw_swarm_not_ready = false;

            match self.raw_swarm.poll() {
                Async::NotReady => raw_swarm_not_ready = true,
                Async::Ready(RawSwarmEvent::NodeEvent { conn_info, event }) => {
                    self.behaviour.inject_node_event(conn_info.peer_id().clone(), event);
                },
                Async::Ready(RawSwarmEvent::Connected { conn_info, endpoint }) => {
                    if self.banned_peers.contains(conn_info.peer_id()) {
                        self.raw_swarm.peer(conn_info.peer_id().clone())
                            .into_connected()
                            .expect("the RawSwarm just notified us that we were connected; QED")
                            .close();
                    } else {
                        self.behaviour.inject_connected(conn_info.peer_id().clone(), endpoint);
                    }
                },
                Async::Ready(RawSwarmEvent::NodeClosed { conn_info, endpoint, .. }) => {
                    self.behaviour.inject_disconnected(conn_info.peer_id(), endpoint);
                },
                Async::Ready(RawSwarmEvent::Replaced { new_info, closed_endpoint, endpoint, .. }) => {
                    self.behaviour.inject_replaced(new_info.peer_id().clone(), closed_endpoint, endpoint);
                },
                Async::Ready(RawSwarmEvent::IncomingConnection(incoming)) => {
                    let handler = self.behaviour.new_handler();
                    incoming.accept(handler.into_node_handler_builder());
                },
                Async::Ready(RawSwarmEvent::NewListenerAddress { listen_addr }) => {
                    if !self.listened_addrs.contains(&listen_addr) {
                        self.listened_addrs.push(listen_addr.clone())
                    }
                    self.behaviour.inject_new_listen_addr(&listen_addr);
                }
                Async::Ready(RawSwarmEvent::ExpiredListenerAddress { listen_addr }) => {
                    self.listened_addrs.retain(|a| a != &listen_addr);
                    self.behaviour.inject_expired_listen_addr(&listen_addr);
                }
                Async::Ready(RawSwarmEvent::ListenerClosed { .. }) => {},
                Async::Ready(RawSwarmEvent::IncomingConnectionError { .. }) => {},
                Async::Ready(RawSwarmEvent::DialError { peer_id, multiaddr, error, new_state }) => {
                    self.behaviour.inject_addr_reach_failure(Some(&peer_id), &multiaddr, &error);
                    if let raw_swarm::PeerState::NotConnected = new_state {
                        self.behaviour.inject_dial_failure(&peer_id);
                    }
                },
                Async::Ready(RawSwarmEvent::UnknownPeerDialError { multiaddr, error, .. }) => {
                    self.behaviour.inject_addr_reach_failure(None, &multiaddr, &error);
                },
            }

            let behaviour_poll = {
                let mut parameters = SwarmPollParameters {
                    local_peer_id: &mut self.raw_swarm.local_peer_id(),
                    supported_protocols: &self.supported_protocols,
                    listened_addrs: &self.listened_addrs,
                    external_addrs: &self.external_addrs
                };
                self.behaviour.poll(&mut parameters)
            };

            match behaviour_poll {
                Async::NotReady if raw_swarm_not_ready => return Ok(Async::NotReady),
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
                    if let Some(mut peer) = self.raw_swarm.peer(peer_id).into_connected() {
                        peer.send_event(event);
                    }
                },
                Async::Ready(NetworkBehaviourAction::ReportObservedAddr { address }) => {
                    for addr in self.raw_swarm.nat_traversal(&address) {
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

        let raw_swarm = RawSwarm::new_with_incoming_limit(self.transport, self.local_peer_id, self.incoming_limit);

        ExpandedSwarm {
            raw_swarm,
            behaviour: self.behaviour,
            supported_protocols,
            listened_addrs: SmallVec::new(),
            external_addrs: Addresses::default(),
            banned_peers: HashSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{identity, PeerId, PublicKey};
    use crate::protocols_handler::{DummyProtocolsHandler, ProtocolsHandler};
    use crate::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters, SwarmBuilder};
    use crate::tests::dummy_transport::DummyTransport;
    use futures::prelude::*;
    use multiaddr::Multiaddr;
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
        let transport = DummyTransport::new();
        let behaviour = DummyBehaviour{marker: PhantomData};
        let swarm = SwarmBuilder::new(transport, behaviour, id.into())
            .incoming_limit(Some(4)).build();
        assert_eq!(swarm.raw_swarm.incoming_limit(), Some(4));
    }

    #[test]
    fn test_build_swarm_with_max_listeners_none() {
        let id = get_random_id();
        let transport = DummyTransport::new();
        let behaviour = DummyBehaviour{marker: PhantomData};
        let swarm = SwarmBuilder::new(transport, behaviour, id.into()).build();
        assert!(swarm.raw_swarm.incoming_limit().is_none())
    }
}
