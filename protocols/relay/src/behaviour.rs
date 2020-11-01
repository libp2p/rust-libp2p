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

use crate::handler::{RelayHandler, RelayHandlerEvent, RelayHandlerHopRequest, RelayHandlerIn};
use crate::transport::{RelayConnection, TransportToBehaviourMsg};
use fnv::FnvHashSet;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use libp2p_core::{
    connection::{ConnectedPoint, ConnectionId},
    multiaddr::{Multiaddr, Protocol},
    PeerId,
};
use libp2p_swarm::{
    DialPeerCondition, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction,
    NotifyHandler, PollParameters, ProtocolsHandler,
};
use std::task::{Context, Poll};
use std::{
    collections::{HashMap, VecDeque},
    marker::PhantomData,
};

/// Network behaviour that allows reaching nodes through relaying.
pub struct Relay {
    // TODO: Document
    to_transport: mpsc::Sender<BehaviourToTransportMsg>,
    from_transport: mpsc::Receiver<TransportToBehaviourMsg>,

    outbox_to_transport: Vec<NegotiatedSubstream>,

    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<RelayHandlerIn, ()>>,

    /// List of peers the network is connected to.
    connected_peers: FnvHashSet<PeerId>,

    /// Requests for us to act as a relay, that are in the process of being fulfilled.
    /// Contains the request and the source of the request.
    pending_incoming_hop_requests: Vec<(PeerId, RelayHandlerHopRequest)>,

    /// Us requesting a relay to relay for us.
    pending_outgoing_hop_requests: HashMap<PeerId, OutgoingHopRequest>,

    /// List of relay nodes that act as a listener for us.
    relay_listeners: HashMap<PeerId, RelayListener>,
}

enum OutgoingHopRequest {
    Dialing {
        relay_addr: Multiaddr,
        relay_peer_id: PeerId,
        destination_addr: Multiaddr,
        destination_peer_id: PeerId,
        send_back: oneshot::Sender<RelayConnection>,
    },
}

// TODO: Should one be able to only specify relay servers via
// Swarm::listen_on(Multiaddress(<realy_server>/p2p-circuit/)) or should one also be able to add
// them via the Relay behaviour? The latter would allow other behaviours to manage ones relay
// servers.
impl Relay {
    /// Builds a new `Relay` behaviour.
    pub fn new(
        to_transport: mpsc::Sender<BehaviourToTransportMsg>,
        from_transport: mpsc::Receiver<TransportToBehaviourMsg>,
    ) -> Self {
        Relay {
            to_transport,
            from_transport,
            events: Default::default(),
            connected_peers: Default::default(),
            pending_incoming_hop_requests: Default::default(),
            pending_outgoing_hop_requests: Default::default(),
            relay_listeners: Default::default(),
        }
    }
}

impl NetworkBehaviour for Relay {
    type ProtocolsHandler = RelayHandler;
    type OutEvent = ();

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        println!("Relay::new_handler()");
        RelayHandler::default()
    }

    fn addresses_of_peer(&mut self, remote_peer_id: &PeerId) -> Vec<Multiaddr> {
        println!("Relay::addresses_of_peer()");
        let relay_listener_addresses = self.relay_listeners.iter().filter_map(|(peer_id, r)| {
            if let RelayListener::Connecting(address) = r {
                if peer_id == remote_peer_id {
                    return Some(address.clone());
                }
            }
            None
        });
        let outgoing_hop_request_addresses =
            self.pending_outgoing_hop_requests
                .iter()
                .filter_map(|(peer_id, r)| {
                    if let OutgoingHopRequest::Dialing { relay_addr, .. } = r {
                        if peer_id == remote_peer_id {
                            return Some(relay_addr.clone());
                        }
                    }
                    None
                });

        relay_listener_addresses
            .chain(outgoing_hop_request_addresses)
            .collect()

        // We return the addresses that potential relaying sources have given us for potential
        // destination.
        // For example, if node A connects to us and says "I want to connect to node B whose
        // address is M", and then `addresses_of_peer(B)` is called, then we return `M`.
        // self.pending_hop_requests
        //     .iter()
        //     .filter(|rq| rq.1.destination_id() == remote_peer_id)
        //     .flat_map(|rq| rq.1.destination_addresses())
        //     .cloned()
        //     .collect()
    }

    fn inject_connection_established(&mut self, _: &PeerId, _: &ConnectionId, _: &ConnectedPoint) {}

    fn inject_connected(&mut self, id: &PeerId) {
        self.connected_peers.insert(id.clone());

        if let Some(RelayListener::Connecting(addr)) = self.relay_listeners.get(id) {
            self.relay_listeners
                .insert(id.clone(), RelayListener::Connected(addr.clone()));
        }

        if let Some(OutgoingHopRequest::Dialing {
            relay_addr,
            relay_peer_id,
            destination_addr,
            destination_peer_id,
            send_back,
        }) = self.pending_outgoing_hop_requests.get(id)
        {
            self.events
                .push_back(NetworkBehaviourAction::NotifyHandler {
                    peer_id: id.clone(),
                    handler: NotifyHandler::Any,
                    event: RelayHandlerIn::RelayRequest {
                        target: destination_peer_id.clone(),
                        addresses: vec![destination_addr.clone()],
                    },
                });
        }

        // Ask the newly-opened connection to be used as destination if relevant.
        while let Some(pos) = self
            .pending_incoming_hop_requests
            .iter()
            .position(|p| p.1.destination_id() == id)
        {
            let (source, hop_request) = self.pending_incoming_hop_requests.remove(pos);

            let send_back = RelayHandlerIn::DestinationRequest {
                source,
                source_addresses: Vec::new(), // TODO: wrong
                substream: hop_request,
            };

            self.events
                .push_back(NetworkBehaviourAction::NotifyHandler {
                    peer_id: id.clone(),
                    handler: NotifyHandler::Any,
                    event: send_back,
                });
        }
    }

    fn inject_dial_failure(&mut self, _peer_id: &PeerId) {
        unimplemented!();
    }

    fn inject_disconnected(&mut self, id: &PeerId) {
        self.connected_peers.remove(id);

        // TODO: send back proper refusal message to the source
        self.pending_incoming_hop_requests
            .retain(|rq| rq.1.destination_id() != id);
    }

    fn inject_event(
        &mut self,
        event_source: PeerId,
        _connection: ConnectionId,
        event: RelayHandlerEvent,
    ) {
        match event {
            // Remote wants us to become a relay.
            RelayHandlerEvent::HopRequest(hop_request) => {
                if self.connected_peers.contains(hop_request.destination_id()) {
                    let dest_id = hop_request.destination_id().clone();
                    let send_back = RelayHandlerIn::DestinationRequest {
                        source: event_source,
                        source_addresses: Vec::new(), // TODO: wrong
                        substream: hop_request,
                    };
                    self.events
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id: dest_id,
                            // TODO: Any correct here?
                            handler: NotifyHandler::Any,
                            event: send_back,
                        });
                } else {
                    let dest_id = hop_request.destination_id().clone();
                    self.pending_incoming_hop_requests
                        .push((event_source, hop_request));
                    self.events.push_back(NetworkBehaviourAction::DialPeer {
                        peer_id: dest_id,
                        condition: DialPeerCondition::NotDialing,
                    });
                }
            }

            // Remote wants us to become a destination.
            RelayHandlerEvent::DestinationRequest(dest_request) => {
                // TODO: we would like to accept the destination request, but no API allows that
                // at the moment
                let send_back = RelayHandlerIn::AcceptDestinationRequest(dest_request);
                self.events
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: event_source,
                        // TODO: Any correct here?
                        handler: NotifyHandler::Any,
                        event: send_back,
                    });
            }

            RelayHandlerEvent::RelayRequestDenied(_) => {}
            RelayHandlerEvent::OutgoingRelayRequestSuccess(_, _) => unimplemented!(),
            RelayHandlerEvent::IncomingRelayRequestSuccess(_) => unimplemented!(),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        loop {
            match self.from_transport.poll_next_unpin(cx) {
                Poll::Ready(Some(TransportToBehaviourMsg::DialRequest {
                    relay_addr,
                    relay_peer_id,
                    destination_addr,
                    destination_peer_id,
                    send_back,
                })) => {
                    if self.connected_peers.contains(&relay_peer_id) {
                        unimplemented!();
                    } else {
                        self.pending_outgoing_hop_requests.insert(
                            relay_peer_id.clone(),
                            OutgoingHopRequest::Dialing {
                                relay_addr,
                                relay_peer_id: relay_peer_id.clone(),
                                destination_addr,
                                destination_peer_id,
                                send_back,
                            },
                        );
                        return Poll::Ready(NetworkBehaviourAction::DialPeer {
                            peer_id: relay_peer_id,
                            condition: DialPeerCondition::Disconnected,
                        });
                    }
                }
                Poll::Ready(Some(TransportToBehaviourMsg::ListenRequest { address, peer_id })) => {
                    if self.connected_peers.contains(&peer_id) {
                        self.relay_listeners
                            .insert(peer_id, RelayListener::Connected(address));
                    } else {
                        self.relay_listeners
                            .insert(peer_id.clone(), RelayListener::Connecting(address));
                        return Poll::Ready(NetworkBehaviourAction::DialPeer {
                            peer_id,
                            condition: DialPeerCondition::Disconnected,
                        });
                    }
                }
                Poll::Ready(None) => panic!("Channel to transport wrapper is closed"),
                Poll::Pending => break,
            }
        }

        Poll::Pending
    }
}

pub enum BehaviourToTransportMsg {}

enum RelayListener {
    Connecting(Multiaddr),
    Connected(Multiaddr),
}
