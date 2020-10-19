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

use crate::handler::{RelayHandler, RelayHandlerIn, RelayHandlerEvent, RelayHandlerHopRequest};
use fnv::FnvHashSet;
use futures::prelude::*;
use libp2p_swarm::{NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, DialPeerCondition, PollParameters, ProtocolsHandler};
use libp2p_core::{connection::ConnectionId, Multiaddr, PeerId};
use std::{collections::VecDeque, marker::PhantomData};
use std::task::{Context, Poll};

/// Network behaviour that allows reaching nodes through relaying.
pub struct Relay {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<RelayHandlerIn, ()>>,

    /// List of peers the network is connected to.
    connected_peers: FnvHashSet<PeerId>,

    /// Requests for us to act as a destination, that are in the process of being fulfilled.
    /// Contains the request and the source of the request.
    pending_hop_requests: Vec<(PeerId, RelayHandlerHopRequest)>,
}

impl Relay {
    /// Builds a new `Relay` behaviour.
    pub fn new() -> Self {
        Relay {
            events: VecDeque::new(),
            connected_peers: FnvHashSet::default(),
            pending_hop_requests: Vec::new(),
        }
    }
}

impl NetworkBehaviour for Relay {
    type ProtocolsHandler = RelayHandler;
    type OutEvent = ();

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        RelayHandler::default()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        // We return the addresses that potential relaying sources have given us for potential
        // destination.
        // For example, if node A connects to us and says "I want to connect to node B whose
        // address is M", and then `addresses_of_peer(B)` is called, then we return `M`.
        self.pending_hop_requests
            .iter()
            .filter(|rq| rq.1.destination_id() == peer_id)
            .flat_map(|rq| rq.1.destination_addresses())
            .cloned()
            .collect()
    }

    fn inject_connected(&mut self, id: &PeerId) {
        self.connected_peers.insert(id.clone());

        // Ask the newly-opened connection to be used as destination if relevant.
        while let Some(pos) = self.pending_hop_requests.iter().position(|p| p.1.destination_id() == id) {
            let (source, hop_request) = self.pending_hop_requests.remove(pos);

            let send_back = RelayHandlerIn::DestinationRequest {
                source,
                source_addresses: Vec::new(),       // TODO: wrong
                substream: hop_request,
            };

            self.events.push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: id.clone(),
                handler: NotifyHandler::Any,
                event: send_back
            });
        }
    }

    fn inject_disconnected(&mut self, id: &PeerId) {
        self.connected_peers.remove(id);

        // TODO: send back proper refusal message to the source
        self.pending_hop_requests.retain(|rq| rq.1.destination_id() != id);
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
                        source_addresses: Vec::new(),       // TODO: wrong
                        substream: hop_request,
                    };
                    self.events.push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: dest_id,
                        // TODO: Any correct here?
                        handler: NotifyHandler::Any,
                        event: send_back
                    });

                } else {
                    let dest_id = hop_request.destination_id().clone();
                    self.pending_hop_requests.push((event_source, hop_request));
                    self.events.push_back(NetworkBehaviourAction::DialPeer {
                        peer_id: dest_id,
                        condition: DialPeerCondition::NotDialing,
                    });
                }
            },

            // Remote wants us to become a destination.
            RelayHandlerEvent::DestinationRequest(dest_request) => {
                // TODO: we would like to accept the destination request, but no API allows that
                // at the moment
                let send_back = RelayHandlerIn::DenyDestinationRequest(dest_request);
                self.events.push_back(NetworkBehaviourAction::NotifyHandler {
                    peer_id: event_source,
                    // TODO: Any correct here?
                    handler: NotifyHandler::Any,
                    event: send_back
                });
            },

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

        Poll::Pending
    }
}
