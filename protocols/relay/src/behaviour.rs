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
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, Multiaddr, PeerId};
use std::{collections::VecDeque, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};

/// Network behaviour that allows reaching nodes through relaying.
pub struct Relay<TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<RelayHandlerIn<TSubstream>, ()>>,

    /// List of peers the network is connected to.
    connected_peers: FnvHashSet<PeerId>,

    /// Requests for us to act as a destination, that are in the process of being fulfilled.
    /// Contains the request and the source of the request.
    pending_hop_requests: Vec<(PeerId, RelayHandlerHopRequest<TSubstream>)>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Relay<TSubstream> {
    /// Builds a new `Relay` behaviour.
    pub fn new() -> Self {
        Relay {
            events: VecDeque::new(),
            connected_peers: FnvHashSet::default(),
            pending_hop_requests: Vec::new(),
            marker: PhantomData,
        }
    }
}

impl<TSubstream> NetworkBehaviour for Relay<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = RelayHandler<TSubstream>;
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

    fn inject_connected(&mut self, id: PeerId, _: ConnectedPoint) {
        self.connected_peers.insert(id.clone());

        // Ask the newly-opened connection to be used as destination if relevant.
        while let Some(pos) = self.pending_hop_requests.iter().position(|p| p.1.destination_id() == &id) {
            let (source, hop_request) = self.pending_hop_requests.remove(pos);

            let send_back = RelayHandlerIn::DestinationRequest {
                source,
                source_addresses: Vec::new(),       // TODO: wrong
                substream: hop_request,
            };

            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: send_back
            });
        }
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        self.connected_peers.remove(id);

        // TODO: send back proper refusal message to the source
        self.pending_hop_requests.retain(|rq| rq.1.destination_id() != id);
    }

    fn inject_node_event(
        &mut self,
        event_source: PeerId,
        event: RelayHandlerEvent<TSubstream>,
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
                    self.events.push_back(NetworkBehaviourAction::SendEvent {
                        peer_id: dest_id,
                        event: send_back
                    });

                } else {
                    let dest_id = hop_request.destination_id().clone();
                    self.pending_hop_requests.push((event_source, hop_request));
                    self.events.push_back(NetworkBehaviourAction::DialPeer {
                        peer_id: dest_id,
                    });
                }
            },

            // Remote wants us to become a destination.
            RelayHandlerEvent::DestinationRequest(dest_request) => {
                // TODO: we would like to accept the destination request, but no API allows that
                // at the moment
                let send_back = RelayHandlerIn::DenyDestinationRequest(dest_request);
                self.events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: event_source,
                    event: send_back
                });
            },

            RelayHandlerEvent::RelayRequestDenied(_) => {}
            RelayHandlerEvent::RelayRequestSuccess(_, _) => {}
        }
    }

    fn poll(
        &mut self,
        _: &mut PollParameters<'_>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Async::Ready(event);
        }

        Async::NotReady
    }
}
