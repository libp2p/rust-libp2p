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
use crate::transport::TransportToBehaviourMsg;
use fnv::FnvHashSet;
use futures::channel::mpsc;
use futures::prelude::*;
use libp2p_core::{
    connection::ConnectionId,
    multiaddr::{Multiaddr, Protocol},
    PeerId,
};
use libp2p_swarm::{
    DialPeerCondition, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters,
    ProtocolsHandler,
};
use std::task::{Context, Poll};
use std::{collections::VecDeque, marker::PhantomData};

/// Network behaviour that allows reaching nodes through relaying.
pub struct Relay {
    // TODO: Document
    to_transport: mpsc::Sender<BehaviourToTransportMsg>,
    from_transport: mpsc::Receiver<TransportToBehaviourMsg>,

    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<RelayHandlerIn, ()>>,

    /// List of peers the network is connected to.
    connected_peers: FnvHashSet<PeerId>,

    /// Requests for us to act as a destination, that are in the process of being fulfilled.
    /// Contains the request and the source of the request.
    pending_hop_requests: Vec<(PeerId, RelayHandlerHopRequest)>,

    local_listen_requests: Vec<LocalListenRequest>,
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
            pending_hop_requests: Default::default(),
            local_listen_requests: Default::default(),
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
        self.local_listen_requests
            .iter()
            .filter_map(|r| {
                if let LocalListenRequest::ConnectingToRelay { peer_id, address } = r {
                    if peer_id == remote_peer_id {
                        return Some(address.clone());
                    }
                }
                None
            })
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

    fn inject_connected(&mut self, id: &PeerId) {
        unimplemented!();
        self.connected_peers.insert(id.clone());

        // Ask the newly-opened connection to be used as destination if relevant.
        while let Some(pos) = self
            .pending_hop_requests
            .iter()
            .position(|p| p.1.destination_id() == id)
        {
            let (source, hop_request) = self.pending_hop_requests.remove(pos);

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

    fn inject_disconnected(&mut self, id: &PeerId) {
        self.connected_peers.remove(id);

        // TODO: send back proper refusal message to the source
        self.pending_hop_requests
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
                    self.pending_hop_requests.push((event_source, hop_request));
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
                let send_back = RelayHandlerIn::DenyDestinationRequest(dest_request);
                self.events
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: event_source,
                        // TODO: Any correct here?
                        handler: NotifyHandler::Any,
                        event: send_back,
                    });
            }

            RelayHandlerEvent::RelayRequestDenied(_) => {}
            RelayHandlerEvent::RelayRequestSuccess(_, _) => {}
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
                Poll::Ready(Some(TransportToBehaviourMsg::DialRequest(destination, oneshot))) => {}
                Poll::Ready(Some(TransportToBehaviourMsg::ListenRequest { address, peer_id })) => {
                    if self.connected_peers.contains(&peer_id) {
                        self.local_listen_requests
                            .push(LocalListenRequest::Negotiating);
                    } else {
                        self.local_listen_requests
                            .push(LocalListenRequest::ConnectingToRelay {
                                address,
                                peer_id: peer_id.clone(),
                            });
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

enum LocalListenRequest {
    ConnectingToRelay { address: Multiaddr, peer_id: PeerId },
    Negotiating,
}
