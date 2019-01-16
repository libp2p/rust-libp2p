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

//! High level manager of the network.
//!
//! The `Swarm` struct contains the state of the network as a whole. The entire behaviour of a
//! libp2p network can be controlled through the `Swarm`.
//!
//! # Initializing a Swarm
//!
//! Creating a `Swarm` requires three things:
//!
//! - An implementation of the `Transport` trait. This is the type that will be used in order to
//!   reach nodes on the network based on their address. See the `transport` module for more
//!   information.
//! - An implementation of the `NetworkBehaviour` trait. This is a state machine that defines how
//!   the swarm should behave once it is connected to a node.
//! - An implementation of the `Topology` trait. This is a container that holds the list of nodes
//!   that we think are part of the network. See the `topology` module for more information.
//!
//! # Network behaviour
//!
//! The `NetworkBehaviour` trait is implemented on types that indicate to the swarm how it should
//! behave. This includes which protocols are supported and which nodes to try to connect to.
//!

use crate::{
    Transport, Multiaddr, PublicKey, PeerId, InboundUpgrade, OutboundUpgrade, UpgradeInfo, ProtocolName,
    muxing::StreamMuxer,
    nodes::{
        handled_node::NodeHandler,
        node::Substream,
        raw_swarm::{RawSwarm, RawSwarmEvent}
    },
    protocols_handler::{NodeHandlerWrapperBuilder, NodeHandlerWrapper, IntoProtocolsHandler, ProtocolsHandler},
    topology::Topology,
    transport::TransportError,
    topology::DisconnectReason,
};
use futures::prelude::*;
use smallvec::SmallVec;
use std::{fmt, io, ops::{Deref, DerefMut}};

pub use crate::nodes::raw_swarm::ConnectedPoint;

/// Contains the state of the network, plus the way it should behave.
pub struct Swarm<TTransport, TBehaviour, TTopology>
where TTransport: Transport,
      TBehaviour: NetworkBehaviour<TTopology>,
{
    raw_swarm: RawSwarm<
        TTransport,
        <<<TBehaviour as NetworkBehaviour<TTopology>>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent,
        <<<TBehaviour as NetworkBehaviour<TTopology>>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
        NodeHandlerWrapperBuilder<TBehaviour::ProtocolsHandler>,
        <<<TBehaviour as NetworkBehaviour<TTopology>>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::Error,
    >,

    /// Handles which nodes to connect to and how to handle the events sent back by the protocol
    /// handlers.
    behaviour: TBehaviour,

    /// Holds the topology of the network. In other words all the nodes that we think exist, even
    /// if we're not connected to them.
    topology: TTopology,

    /// List of protocols that the behaviour says it supports.
    supported_protocols: SmallVec<[Vec<u8>; 16]>,

    /// List of multiaddresses we're listening on.
    listened_addrs: SmallVec<[Multiaddr; 8]>,
}

impl<TTransport, TBehaviour, TTopology> Deref for Swarm<TTransport, TBehaviour, TTopology>
where TTransport: Transport,
      TBehaviour: NetworkBehaviour<TTopology>,
{
    type Target = TBehaviour;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.behaviour
    }
}

impl<TTransport, TBehaviour, TTopology> DerefMut for Swarm<TTransport, TBehaviour, TTopology>
where TTransport: Transport,
      TBehaviour: NetworkBehaviour<TTopology>,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.behaviour
    }
}

impl<TTransport, TBehaviour, TMuxer, TTopology> Swarm<TTransport, TBehaviour, TTopology>
where TBehaviour: NetworkBehaviour<TTopology>,
      TMuxer: StreamMuxer + Send + Sync + 'static,
      <TMuxer as StreamMuxer>::OutboundSubstream: Send + 'static,
      <TMuxer as StreamMuxer>::Substream: Send + 'static,
      TTransport: Transport<Output = (PeerId, TMuxer)> + Clone,
      TTransport::Error: Send + 'static,
      TTransport::Listener: Send + 'static,
      TTransport::ListenerUpgrade: Send + 'static,
      TTransport::Dial: Send + 'static,
      TBehaviour::ProtocolsHandler: Send + 'static,
      <TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler: ProtocolsHandler<Substream = Substream<TMuxer>> + Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::Error: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Error: fmt::Debug + Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Error: fmt::Debug + Send + 'static,
      <NodeHandlerWrapper<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler> as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      TTopology: Topology,
{
    /// Builds a new `Swarm`.
    #[inline]
    pub fn new(transport: TTransport, mut behaviour: TBehaviour, topology: TTopology) -> Self {
        let supported_protocols = behaviour
            .new_handler()
            .into_handler(topology.local_peer_id())
            .listen_protocol()
            .protocol_info()
            .into_iter()
            .map(|info| info.protocol_name().to_vec())
            .collect();

        let raw_swarm = RawSwarm::new(transport, topology.local_peer_id().clone());

        Swarm {
            raw_swarm,
            behaviour,
            topology,
            supported_protocols,
            listened_addrs: SmallVec::new(),
        }
    }

    /// Returns the transport passed when building this object.
    #[inline]
    pub fn transport(me: &Self) -> &TTransport {
        me.raw_swarm.transport()
    }

    /// Starts listening on the given address.
    ///
    /// Returns an error if the address is not supported.
    /// On success, returns an alternative version of the address.
    #[inline]
    pub fn listen_on(me: &mut Self, addr: Multiaddr) -> Result<Multiaddr, TransportError<TTransport::Error>> {
        let result = me.raw_swarm.listen_on(addr);
        if let Ok(ref addr) = result {
            me.listened_addrs.push(addr.clone());
        }
        result
    }

    /// Tries to dial the given address.
    ///
    /// Returns an error if the address is not supported.
    #[inline]
    pub fn dial_addr(me: &mut Self, addr: Multiaddr) -> Result<(), TransportError<TTransport::Error>> {
        let handler = me.behaviour.new_handler();
        me.raw_swarm.dial(addr, handler.into_node_handler_builder())
    }

    /// Tries to reach the given peer using the elements in the topology.
    ///
    /// Has no effect if we are already connected to that peer, or if no address is known for the
    /// peer.
    #[inline]
    pub fn dial(me: &mut Self, peer_id: PeerId) {
        let addrs = me.topology.addresses_of_peer(&peer_id);
        let handler = me.behaviour.new_handler().into_node_handler_builder();
        if let Some(peer) = me.raw_swarm.peer(peer_id).as_not_connected() {
            let _ = peer.connect_iter(addrs, handler);
        }
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    #[inline]
    pub fn listeners(me: &Self) -> impl Iterator<Item = &Multiaddr> {
        RawSwarm::listeners(&me.raw_swarm)
    }

    /// Returns the peer ID of the swarm passed as parameter.
    #[inline]
    pub fn local_peer_id(me: &Self) -> &PeerId {
        &me.raw_swarm.local_peer_id()
    }

    /// Returns the topology of the swarm.
    #[inline]
    pub fn topology(me: &Self) -> &TTopology {
        &me.topology
    }

    /// Returns the topology of the swarm.
    #[inline]
    pub fn topology_mut(me: &mut Self) -> &mut TTopology {
        &mut me.topology
    }
}

impl<TTransport, TBehaviour, TMuxer, TTopology> Stream for Swarm<TTransport, TBehaviour, TTopology>
where TBehaviour: NetworkBehaviour<TTopology>,
      TMuxer: StreamMuxer + Send + Sync + 'static,
      <TMuxer as StreamMuxer>::OutboundSubstream: Send + 'static,
      <TMuxer as StreamMuxer>::Substream: Send + 'static,
      TTransport: Transport<Output = (PeerId, TMuxer)> + Clone,
      TTransport::Error: Send + 'static,
      TTransport::Listener: Send + 'static,
      TTransport::ListenerUpgrade: Send + 'static,
      TTransport::Dial: Send + 'static,
      TBehaviour::ProtocolsHandler: Send + 'static,
      <TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler: ProtocolsHandler<Substream = Substream<TMuxer>> + Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::Error: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as InboundUpgrade<Substream<TMuxer>>>::Error: fmt::Debug + Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<Substream<TMuxer>> + Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Future: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as OutboundUpgrade<Substream<TMuxer>>>::Error: fmt::Debug + Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::Info: Send + 'static,
      <<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter: Send + 'static,
      <<<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutboundProtocol as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send + 'static,
      <NodeHandlerWrapper<<TBehaviour::ProtocolsHandler as IntoProtocolsHandler>::Handler> as NodeHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
      TTopology: Topology,
{
    type Item = TBehaviour::OutEvent;
    type Error = io::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Option<TBehaviour::OutEvent>, io::Error> {
        loop {
            let mut raw_swarm_not_ready = false;

            let mut addrs_to_dial = SmallVec::new();
            let mut peers_to_dial = SmallVec::new();
            let mut events_to_send: SmallVec<[(_, _); 4]> = SmallVec::new();

            macro_rules! poll_behaviour {
                ($me:expr, $event:expr) => {{
                    let transport = $me.raw_swarm.transport();
                    let mut parameters = PollParameters {
                        topology: &mut $me.topology,
                        supported_protocols: &$me.supported_protocols,
                        listened_addrs: &$me.listened_addrs,
                        nat_traversal: &move |a, b| transport.nat_traversal(a, b),
                        addrs_to_dial: &mut addrs_to_dial,
                        peers_to_dial: &mut peers_to_dial,
                        events_to_send: Box::new(|dst, ev| events_to_send.push((dst, ev))),
                    };
                    $me.behaviour.poll($event, &mut parameters)
                }};
            }

            let behaviour_poll = match self.raw_swarm.poll() {
                Async::NotReady => {
                    raw_swarm_not_ready = true;
                    poll_behaviour!(self, SwarmEvent::None)
                },
                Async::Ready(RawSwarmEvent::NodeEvent { peer_id, event }) => {
                    poll_behaviour!(self, SwarmEvent::ProtocolsHandlerEvent { peer_id, event })
                },
                Async::Ready(RawSwarmEvent::Connected { peer_id, endpoint }) => {
                    self.topology.set_connected(&peer_id, &endpoint);
                    poll_behaviour!(self, SwarmEvent::Connected { peer_id: &peer_id, endpoint: &endpoint })
                },
                Async::Ready(RawSwarmEvent::NodeClosed { peer_id, endpoint }) => {
                    self.topology.set_disconnected(&peer_id, &endpoint, DisconnectReason::Graceful);
                    poll_behaviour!(self, SwarmEvent::Disconnected { peer_id: &peer_id, endpoint: &endpoint })
                },
                Async::Ready(RawSwarmEvent::NodeError { peer_id, endpoint, .. }) => {
                    self.topology.set_disconnected(&peer_id, &endpoint, DisconnectReason::Error);
                    poll_behaviour!(self, SwarmEvent::Disconnected { peer_id: &peer_id, endpoint: &endpoint })
                },
                Async::Ready(RawSwarmEvent::Replaced { peer_id, closed_endpoint, endpoint }) => {
                    self.topology.set_disconnected(&peer_id, &closed_endpoint, DisconnectReason::Replaced);
                    self.topology.set_connected(&peer_id, &endpoint);
                    // TODO: poll_behaviour!(self, SwarmEvent::Connected { peer_id: &peer_id, endpoint: &endpoint });
                    // TODO: poll_behaviour!(self, SwarmEvent::Disconnected { peer_id: &peer_id, endpoint: &endpoint });
                    poll_behaviour!(self, SwarmEvent::None)
                },
                Async::Ready(RawSwarmEvent::IncomingConnection(incoming)) => {
                    let handler = self.behaviour.new_handler();
                    incoming.accept(handler.into_node_handler_builder());
                    poll_behaviour!(self, SwarmEvent::None)
                },
                Async::Ready(RawSwarmEvent::ListenerClosed { .. }) => poll_behaviour!(self, SwarmEvent::None),
                Async::Ready(RawSwarmEvent::IncomingConnectionError { .. }) => poll_behaviour!(self, SwarmEvent::None),
                Async::Ready(RawSwarmEvent::DialError { multiaddr, .. }) => {
                    self.topology.set_unreachable(&multiaddr);
                    poll_behaviour!(self, SwarmEvent::None)
                },
                Async::Ready(RawSwarmEvent::UnknownPeerDialError { multiaddr, .. }) => {
                    self.topology.set_unreachable(&multiaddr);
                    poll_behaviour!(self, SwarmEvent::None)
                },
            };

            for addr in addrs_to_dial {
                let _ = Swarm::dial_addr(self, addr);
            }

            for peer in peers_to_dial {
                Swarm::dial(self, peer);
            }

            for (dst, event) in events_to_send {
                if let Some(mut peer) = self.raw_swarm.peer(dst).as_connected() {
                    peer.send_event(event);
                }
            }

            match behaviour_poll {
                Async::NotReady if raw_swarm_not_ready => return Ok(Async::NotReady),
                Async::NotReady => (),
                Async::Ready(event) => {
                    return Ok(Async::Ready(Some(event)));
                },
            }
        }
    }
}

/// A behaviour for the network. Allows customizing the swarm.
///
/// This trait has been designed to be composable. Multiple implementations can be combined into
/// one that handles all the behaviours at once.
pub trait NetworkBehaviour<TTopology> {
    /// Handler for all the protocols the network supports.
    type ProtocolsHandler: IntoProtocolsHandler;
    /// Event generated by the swarm.
    type OutEvent;

    /// Builds a new `ProtocolsHandler`.
    fn new_handler(&mut self) -> Self::ProtocolsHandler;

    /// Polls for things that swarm should do.
    ///
    /// This API mimics the API of the `Stream` trait.
    ///
    /// The swarm will continue calling this method repeatedly until `event` is `SwarmEvent::None`
    /// and this function returned `NotReady`.
    fn poll(&mut self, event: SwarmEvent<<<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent>, topology: &mut PollParameters<TTopology, <<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent>) -> Async<Self::OutEvent>;
}

/// Event passed from the swarm to the `NetworkBehaviour`.
#[derive(Debug)]
pub enum SwarmEvent<'a, TProtoOutEv> {
    /// The swarm has connected to a node.
    Connected {
        /// Id of the node that was connected.
        peer_id: &'a PeerId,
        /// How we are connected to this node.
        endpoint: &'a ConnectedPoint,
    },
    /// The swarm has disconnected from a node.
    Disconnected {
        /// Id of the node that has disconnected.
        peer_id: &'a PeerId,
        /// How we were connected to this node (but no longer are).
        endpoint: &'a ConnectedPoint,
    },
    /// An event has been generated by the protocols handler.
    ProtocolsHandlerEvent {
        /// Id of the node that generated the event.
        peer_id: PeerId,
        /// Event that has been generated.
        event: TProtoOutEv,
    },
    /// Nothing happened on the swarm.
    None,
}

/// Used when deriving `NetworkBehaviour`. When deriving `NetworkBehaviour`, must be implemented
/// for all the possible event types generated by the various fields.
// TODO: document how the custom behaviour works and link this here
pub trait NetworkBehaviourEventProcess<TEvent> {
    /// Called when one of the fields of the type you're deriving `NetworkBehaviour` on generates
    /// an event.
    fn inject_event(&mut self, event: TEvent);
}

/// Parameters passed to `poll()`, that the `NetworkBehaviour` has access to.
// TODO: #[derive(Debug)]
pub struct PollParameters<'a, TTopology: 'a, TEvent: 'a> {
    topology: &'a mut TTopology,
    supported_protocols: &'a [Vec<u8>],
    listened_addrs: &'a [Multiaddr],
    nat_traversal: &'a dyn Fn(&Multiaddr, &Multiaddr) -> Option<Multiaddr>,
    addrs_to_dial: &'a mut SmallVec<[Multiaddr; 4]>,
    peers_to_dial: &'a mut SmallVec<[PeerId; 4]>,
    events_to_send: Box<dyn FnMut(PeerId, TEvent) + 'a>,
}

impl<'a, TTopology, TEvent> PollParameters<'a, TTopology, TEvent> {
    /// Returns a reference to the topology of the network.
    #[inline]
    pub fn topology(&mut self) -> &mut TTopology {
        &mut self.topology
    }

    /// Returns the list of protocol the behaviour supports when a remote negotiates a protocol on
    /// an inbound substream.
    ///
    /// The iterator's elements are the ASCII names as reported on the wire.
    ///
    /// Note that the list is computed once at initialization and never refreshed.
    #[inline]
    pub fn supported_protocols(&self) -> impl ExactSizeIterator<Item = &[u8]> {
        self.supported_protocols.iter().map(AsRef::as_ref)
    }

    /// Returns the list of the addresses we're listening on.
    #[inline]
    pub fn listened_addresses(&self) -> impl ExactSizeIterator<Item = &Multiaddr> {
        self.listened_addrs.iter()
    }

    /// Returns the list of the addresses nodes can use to reach us.
    #[inline]
    pub fn external_addresses<'b>(&'b mut self) -> impl ExactSizeIterator<Item = Multiaddr> + 'b
    where TTopology: Topology
    {
        let local_peer_id = self.topology.local_peer_id().clone();
        self.topology.addresses_of_peer(&local_peer_id).into_iter()
    }

    /// Returns the public key of the local node.
    #[inline]
    pub fn local_public_key(&self) -> &PublicKey
    where TTopology: Topology
    {
        self.topology.local_public_key()
    }

    /// Returns the peer id of the local node.
    #[inline]
    pub fn local_peer_id(&self) -> &PeerId
    where TTopology: Topology
    {
        self.topology.local_peer_id()
    }

    /// Calls the `nat_traversal` method on the underlying transport of the `Swarm`.
    #[inline]
    pub fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        (self.nat_traversal)(server, observed)
    }

    /// Notifies that a new address has been observed for us.
    pub fn report_observed_address(&mut self, address: &Multiaddr)
    where TTopology: Topology
    {
        let nat_traversal = &self.nat_traversal;
        let iter = self.listened_addrs.iter()
            .filter_map(move |server| {
                nat_traversal(server, address)
            });

        self.topology.add_local_external_addrs(iter);
    }

    /// Start dialing an address without knowing the `PeerId` of the destination.
    // TODO: add proper return type as a `Result`? Complicated because we don't know the type
    //       of the `Transport`
    pub fn dial_address(&mut self, address: Multiaddr) {
        self.addrs_to_dial.push(address);
    }

    /// Start dialing a peer.
    // TODO: add proper return type as a `Result`? Complicated because we don't know the type
    //       of the `Transport`
    pub fn dial(&mut self, peer_id: PeerId) {
        self.peers_to_dial.push(peer_id);
    }

    /// Send an event to a peer.
    // TODO: add proper return type as a `Result`? Complicated because we don't know the type
    //       of the `Transport`
    pub fn send_event(&mut self, peer_id: PeerId, event: TEvent) {
        (self.events_to_send)(peer_id, event);
    }

    /// This method is a hack for the custom derive to work.
    #[doc(hidden)]
    pub fn with_event_map<TNewEv>(&mut self, mut map: impl FnMut(TNewEv) -> TEvent + 'a) -> PollParameters<TTopology, TNewEv> {
        let events_to_send = &mut self.events_to_send;

        PollParameters {
            topology: &mut self.topology,
            supported_protocols: self.supported_protocols,
            listened_addrs: self.listened_addrs,
            nat_traversal: self.nat_traversal,
            addrs_to_dial: &mut self.addrs_to_dial,
            peers_to_dial: &mut self.peers_to_dial,
            events_to_send: Box::new(move |dst, ev| events_to_send(dst, map(ev))),
        }
    }
}
