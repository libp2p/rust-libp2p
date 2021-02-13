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

use crate::handler::{RelayHandlerEvent, RelayHandlerIn, RelayHandlerProto};
use crate::protocol;
use crate::transport::TransportToBehaviourMsg;
use crate::RequestId;
use fnv::FnvHashSet;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use libp2p_core::{
    connection::{ConnectedPoint, ConnectionId, ListenerId},
    multiaddr::Multiaddr,
    PeerId,
};
use libp2p_swarm::{
    DialPeerCondition, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction,
    NotifyHandler, PollParameters,
};
use std::collections::{HashMap, VecDeque};
use std::task::{Context, Poll};

/// Network behaviour that allows the local node to act as a source, a relay and as a destination.
pub struct Relay {
    /// Channel sender to [`crate::RelayTransportWrapper`].
    to_transport: mpsc::Sender<BehaviourToTransportMsg>,
    /// Channel receiver from [`crate::RelayTransportWrapper`].
    from_transport: mpsc::Receiver<TransportToBehaviourMsg>,

    /// Events that need to be send to the [`Crate::RelayTransportWrapper`] via
    /// [`Self::to_transport`].
    outbox_to_transport: Vec<BehaviourToTransportMsg>,
    /// Events that need to be yielded to the outside when polling.
    outbox_to_swarm: VecDeque<NetworkBehaviourAction<RelayHandlerIn, ()>>,

    /// List of peers the network is connected to.
    connected_peers: FnvHashSet<PeerId>,

    /// Requests by the local node to a relay to relay a connection for the local node to a
    /// destination.
    outgoing_relay_requests: OutgoingRelayRequests,

    /// Requests for the local node to act as a relay from a source to a destination indexed by
    /// destination [`PeerId`].
    incoming_relay_requests: HashMap<PeerId, Vec<IncomingRelayRequest>>,

    /// List of relay nodes that act as a listener for the local node acting as a destination.
    relay_listeners: HashMap<PeerId, RelayListener>,
}

#[derive(Default)]
struct OutgoingRelayRequests {
    /// Indexed by relay peer id.
    dialing: HashMap<PeerId, Vec<OutgoingDialingRelayRequest>>,
    upgrading: HashMap<RequestId, OutgoingUpgradingRelayRequest>,
}

struct OutgoingDialingRelayRequest {
    request_id: RequestId,
    relay_addr: Multiaddr,
    destination_addr: Multiaddr,
    destination_peer_id: PeerId,
    send_back: oneshot::Sender<Result<NegotiatedSubstream, OutgoingRelayRequestError>>,
}

struct OutgoingUpgradingRelayRequest {
    send_back: oneshot::Sender<Result<NegotiatedSubstream, OutgoingRelayRequestError>>,
}

enum IncomingRelayRequest {
    DialingDestination {
        source_id: PeerId,
        source_addr: Multiaddr,
        request_id: RequestId,
        request: protocol::IncomingRelayRequest<NegotiatedSubstream>,
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
            outbox_to_transport: Default::default(),
            outbox_to_swarm: Default::default(),
            connected_peers: Default::default(),
            incoming_relay_requests: Default::default(),
            outgoing_relay_requests: Default::default(),
            relay_listeners: Default::default(),
        }
    }
}

impl NetworkBehaviour for Relay {
    type ProtocolsHandler = RelayHandlerProto;
    type OutEvent = ();

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        RelayHandlerProto::default()
    }

    fn addresses_of_peer(&mut self, remote_peer_id: &PeerId) -> Vec<Multiaddr> {
        let mut addresses = Vec::new();

        addresses.extend(self.relay_listeners.iter().filter_map(|(peer_id, r)| {
            if let RelayListener::Connecting(address) = r {
                if peer_id == remote_peer_id {
                    return Some(address.clone());
                }
            }
            None
        }));

        addresses.extend(
            self.outgoing_relay_requests
                .dialing
                .get(remote_peer_id)
                .into_iter()
                .flatten()
                .map(|OutgoingDialingRelayRequest { relay_addr, .. }| relay_addr.clone()),
        );

        addresses.extend(
            self.incoming_relay_requests
                .get(remote_peer_id)
                .into_iter()
                .flatten()
                .map(|IncomingRelayRequest::DialingDestination { request, .. }| {
                    request.destination_addresses().cloned()
                })
                .flatten(),
        );

        addresses
    }

    fn inject_connection_established(&mut self, _: &PeerId, _: &ConnectionId, _: &ConnectedPoint) {}

    fn inject_connected(&mut self, id: &PeerId) {
        self.connected_peers.insert(id.clone());

        if let Some(RelayListener::Connecting(addr)) = self.relay_listeners.remove(id) {
            self.relay_listeners
                .insert(id.clone(), RelayListener::Connected(addr.clone()));
        }

        if let Some(reqs) = self.outgoing_relay_requests.dialing.remove(id) {
            for req in reqs {
                let OutgoingDialingRelayRequest {
                    request_id,
                    relay_addr: _,
                    destination_addr,
                    destination_peer_id,
                    send_back,
                } = req;
                self.outbox_to_swarm
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: id.clone(),
                        handler: NotifyHandler::Any,
                        event: RelayHandlerIn::OutgoingRelayRequest {
                            request_id,
                            destination_peer_id,
                            destination_address: destination_addr.clone(),
                        },
                    });

                self.outgoing_relay_requests
                    .upgrading
                    .insert(request_id, OutgoingUpgradingRelayRequest { send_back });
            }
        }

        // Ask the newly-opened connection to be used as destination if relevant.
        if let Some(reqs) = self.incoming_relay_requests.remove(id) {
            for req in reqs {
                let IncomingRelayRequest::DialingDestination {
                    source_id,
                    source_addr,
                    request_id,
                    request,
                } = req;
                let event = RelayHandlerIn::OutgoingDestinationRequest {
                    source: source_id,
                    request_id,
                    source_addr,
                    substream: request,
                };

                self.outbox_to_swarm
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: id.clone(),
                        handler: NotifyHandler::Any,
                        event: event,
                    });
            }
        }
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        if let Some(reqs) = self.outgoing_relay_requests.dialing.remove(peer_id) {
            for req in reqs {
                let _ = req.send_back.send(Err(OutgoingRelayRequestError::DialingRelay));
            }
        }

        if let Some(reqs) = self.incoming_relay_requests.remove(peer_id) {
            for req in reqs {
                let IncomingRelayRequest::DialingDestination { source_id, request, .. } = req;
                self.outbox_to_swarm.push_back(
                    NetworkBehaviourAction::NotifyHandler {
                        peer_id: source_id,
                        handler: NotifyHandler::Any,
                        event: RelayHandlerIn::DenyIncomingRelayRequest(request),
                    }
                )
            }

        }
    }

    fn inject_connection_closed(&mut self, _: &PeerId, _: &ConnectionId, _: &ConnectedPoint) {}

    fn inject_addr_reach_failure(
        &mut self,
        _peer_id: Option<&PeerId>,
        _addr: &Multiaddr,
        _error: &dyn std::error::Error,
    ) {
        // Handled in `inject_dial_failure`.
    }

    fn inject_listener_error(&mut self, _id: ListenerId, _err: &(dyn std::error::Error + 'static)) {
        unimplemented!();
    }

    fn inject_listener_closed(&mut self, _id: ListenerId, _reason: Result<(), &std::io::Error>) {
        unimplemented!();
    }

    fn inject_disconnected(&mut self, id: &PeerId) {
        self.connected_peers.remove(id);

        // TODO: send back proper refusal message to the source
        self.incoming_relay_requests.remove(id);
    }

    fn inject_event(
        &mut self,
        event_source: PeerId,
        _connection: ConnectionId,
        event: RelayHandlerEvent,
    ) {
        match event {
            // Remote wants us to become a relay.
            RelayHandlerEvent::IncomingRelayRequest {
                request_id,
                source_addr,
                request,
            } => {
                if self.connected_peers.contains(request.destination_id()) {
                    let dest_id = request.destination_id().clone();
                    let event = RelayHandlerIn::OutgoingDestinationRequest {
                        source: event_source,
                        request_id,
                        source_addr,
                        substream: request,
                    };
                    self.outbox_to_swarm
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id: dest_id,
                            handler: NotifyHandler::Any,
                            event,
                        });
                } else {
                    let dest_id = request.destination_id().clone();
                    self.incoming_relay_requests
                        .entry(dest_id)
                        .or_default()
                        .push(IncomingRelayRequest::DialingDestination {
                            request_id,
                            request,
                            source_id: event_source,
                            source_addr,
                        });
                    self.outbox_to_swarm
                        .push_back(NetworkBehaviourAction::DialPeer {
                            peer_id: dest_id,
                            condition: DialPeerCondition::NotDialing,
                        });
                }
            }

            // Remote wants us to become a destination.
            RelayHandlerEvent::IncomingDestinationRequest(source, request_id) => {
                let send_back = RelayHandlerIn::AcceptDestinationRequest(source, request_id);
                self.outbox_to_swarm
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: event_source,
                        handler: NotifyHandler::Any,
                        event: send_back,
                    });
            }

            RelayHandlerEvent::OutgoingRelayRequestError(_destination_peer_id, request_id) => {
                self.outgoing_relay_requests
                    .upgrading
                    .remove(&request_id)
                    .unwrap();
            }
            RelayHandlerEvent::OutgoingRelayRequestSuccess(_destination, request_id, stream) => {
                let send_back = self
                    .outgoing_relay_requests
                    .upgrading
                    .remove(&request_id)
                    .map(|OutgoingUpgradingRelayRequest { send_back, .. }| send_back)
                    .unwrap();
                send_back.send(Ok(stream)).unwrap();
            }
            RelayHandlerEvent::IncomingRelayRequestSuccess { stream, source } => self
                .outbox_to_transport
                .push(BehaviourToTransportMsg::IncomingRelayedConnection { stream, source }),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RelayHandlerIn, Self::OutEvent>> {
        if !self.outbox_to_transport.is_empty() {
            match self.to_transport.poll_ready(cx) {
                Poll::Ready(Ok(())) => {
                    self.to_transport
                        .start_send(self.outbox_to_transport.pop().unwrap())
                        .unwrap();
                }
                Poll::Ready(Err(_)) => unimplemented!(),
                Poll::Pending => {}
            }
        }

        loop {
            match self.from_transport.poll_next_unpin(cx) {
                Poll::Ready(Some(TransportToBehaviourMsg::DialRequest {
                    request_id,
                    relay_addr,
                    relay_peer_id,
                    destination_addr,
                    destination_peer_id,
                    send_back,
                })) => {
                    if self.connected_peers.contains(&relay_peer_id) {
                        self.outbox_to_swarm
                            .push_back(NetworkBehaviourAction::NotifyHandler {
                                peer_id: relay_peer_id,
                                handler: NotifyHandler::Any,
                                event: RelayHandlerIn::OutgoingRelayRequest {
                                    request_id,
                                    destination_peer_id: destination_peer_id.clone(),
                                    destination_address: destination_addr.clone(),
                                },
                            });

                        self.outgoing_relay_requests
                            .upgrading
                            .insert(request_id, OutgoingUpgradingRelayRequest { send_back });
                    } else {
                        self.outgoing_relay_requests
                            .dialing
                            .entry(relay_peer_id)
                            .or_default()
                            .push(OutgoingDialingRelayRequest {
                                request_id,
                                relay_addr,
                                destination_addr,
                                destination_peer_id,
                                send_back,
                            });
                        return Poll::Ready(NetworkBehaviourAction::DialPeer {
                            peer_id: relay_peer_id,
                            condition: DialPeerCondition::Disconnected,
                        });
                    }
                }
                Poll::Ready(Some(TransportToBehaviourMsg::ListenRequest { address, peer_id })) => {
                    if self.connected_peers.contains(&peer_id) {
                        let prev = self
                            .relay_listeners
                            .insert(peer_id, RelayListener::Connected(address));
                        assert!(
                            prev.is_none(),
                            "Not to attempt to listen via same relay twice."
                        );
                    } else {
                        let prev = self
                            .relay_listeners
                            .insert(peer_id.clone(), RelayListener::Connecting(address));
                        assert!(
                            prev.is_none(),
                            "Not to attempt to listen via same relay twice."
                        );
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

        if let Some(event) = self.outbox_to_swarm.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}

pub enum BehaviourToTransportMsg {
    IncomingRelayedConnection {
        stream: NegotiatedSubstream,
        source: PeerId,
    },
}

enum RelayListener {
    Connecting(Multiaddr),
    Connected(Multiaddr),
}

#[derive(Debug, Eq, PartialEq)]
pub enum OutgoingRelayRequestError {
    DialingRelay,
}
