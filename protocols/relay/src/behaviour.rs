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

use crate::handler::{RelayHandlerConfig, RelayHandlerEvent, RelayHandlerIn, RelayHandlerProto};
use crate::protocol;
use crate::transport::TransportToBehaviourMsg;
use crate::RequestId;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use libp2p_core::connection::{ConnectedPoint, ConnectionId, ListenerId};
use libp2p_core::multiaddr::Multiaddr;
use libp2p_core::PeerId;
use libp2p_swarm::{
    DialPeerCondition, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters,
};
use std::collections::{hash_map::Entry, HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};
use std::time::Duration;

/// Network behaviour that allows the local node to act as a source, a relay and as a destination.
pub struct Relay {
    config: RelayConfig,
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
    connected_peers: HashMap<PeerId, HashSet<ConnectionId>>,

    /// Requests by the local node to a relay to relay a connection for the local node to a
    /// destination.
    outgoing_relay_reqs: OutgoingRelayReqs,

    /// Requests for the local node to act as a relay from a source to a destination indexed by
    /// destination [`PeerId`].
    incoming_relay_reqs: HashMap<PeerId, Vec<IncomingRelayReq>>,

    /// List of relay nodes that act as a listener for the local node being a destination.
    listeners: HashMap<PeerId, RelayListener>,
}

#[derive(Default)]
struct OutgoingRelayReqs {
    /// Indexed by relay peer id.
    dialing: HashMap<PeerId, Vec<OutgoingDialingRelayReq>>,
    upgrading: HashMap<RequestId, OutgoingUpgradingRelayReq>,
}

struct OutgoingDialingRelayReq {
    request_id: RequestId,
    src_peer_id: PeerId,
    relay_addr: Multiaddr,
    dst_addr: Multiaddr,
    dst_peer_id: PeerId,
    send_back: oneshot::Sender<Result<protocol::Connection, OutgoingRelayReqError>>,
}

struct OutgoingUpgradingRelayReq {
    send_back: oneshot::Sender<Result<protocol::Connection, OutgoingRelayReqError>>,
}

enum IncomingRelayReq {
    DialingDst {
        src_id: PeerId,
        src_addr: Multiaddr,
        request_id: RequestId,
        req: protocol::IncomingRelayReq,
    },
}

pub struct RelayConfig {
    /// How long to keep connections alive when they're idle.
    ///
    /// For a server, acting as a relay, allowing other nodes to listen for
    /// incoming connections via oneself, this should likely be increased in
    /// order not to force the peer to reconnect too regularly.
    pub connection_idle_timeout: Duration,
}

impl Default for RelayConfig {
    fn default() -> Self {
        RelayConfig {
            connection_idle_timeout: Duration::from_secs(10),
        }
    }
}

// TODO: Should one be able to only specify relay servers via
// Swarm::listen_on(Multiaddress(<realy_server>/p2p-circuit/)) or should one also be able to add
// them via the Relay behaviour? The latter would allow other behaviours to manage ones relay
// servers.
impl Relay {
    /// Builds a new `Relay` behaviour.
    pub(crate) fn new(
        config: RelayConfig,
        to_transport: mpsc::Sender<BehaviourToTransportMsg>,
        from_transport: mpsc::Receiver<TransportToBehaviourMsg>,
    ) -> Self {
        Relay {
            config,
            to_transport,
            from_transport,
            outbox_to_transport: Default::default(),
            outbox_to_swarm: Default::default(),
            connected_peers: Default::default(),
            incoming_relay_reqs: Default::default(),
            outgoing_relay_reqs: Default::default(),
            listeners: Default::default(),
        }
    }
}

impl NetworkBehaviour for Relay {
    type ProtocolsHandler = RelayHandlerProto;
    type OutEvent = ();

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        RelayHandlerProto {
            config: RelayHandlerConfig {
                connection_idle_timeout: self.config.connection_idle_timeout,
            },
        }
    }

    fn addresses_of_peer(&mut self, remote_peer_id: &PeerId) -> Vec<Multiaddr> {
        let mut addresses = Vec::new();

        addresses.extend(self.listeners.iter().filter_map(|(peer_id, r)| {
            if let RelayListener::Connecting(address) = r {
                if peer_id == remote_peer_id {
                    return Some(address.clone());
                }
            }
            None
        }));

        addresses.extend(
            self.outgoing_relay_reqs
                .dialing
                .get(remote_peer_id)
                .into_iter()
                .flatten()
                .map(|OutgoingDialingRelayReq { relay_addr, .. }| relay_addr.clone()),
        );

        addresses.extend(
            self.incoming_relay_reqs
                .get(remote_peer_id)
                .into_iter()
                .flatten()
                .map(|IncomingRelayReq::DialingDst { req, .. }| req.dst_peer().addrs.clone())
                .flatten(),
        );

        addresses
    }

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        connection_id: &ConnectionId,
        _: &ConnectedPoint,
    ) {
        let is_first = self
            .connected_peers
            .entry(*peer)
            .or_default()
            .insert(*connection_id);
        assert!(
            is_first,
            "`inject_connection_established` called with known connection id"
        );

        if let Some(RelayListener::Connecting(_)) = self.listeners.get_mut(peer) {
            self.outbox_to_swarm
                .push_back(NetworkBehaviourAction::NotifyHandler {
                    peer_id: *peer,
                    handler: NotifyHandler::One(*connection_id),
                    event: RelayHandlerIn::UsedForListening(true),
                });
            self.listeners
                .insert(*peer, RelayListener::Connected(*connection_id));
        }
    }

    fn inject_connected(&mut self, id: &PeerId) {
        assert!(
            self.connected_peers
                .get(id)
                .map(|cs| !cs.is_empty())
                .unwrap_or(false),
            "Expect to be connected to peer with at least one connection."
        );

        if let Some(reqs) = self.outgoing_relay_reqs.dialing.remove(id) {
            for req in reqs {
                let OutgoingDialingRelayReq {
                    request_id,
                    src_peer_id,
                    relay_addr: _,
                    dst_addr,
                    dst_peer_id,
                    send_back,
                } = req;
                self.outbox_to_swarm
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: id.clone(),
                        handler: NotifyHandler::Any,
                        event: RelayHandlerIn::OutgoingRelayReq {
                            src_peer_id,
                            request_id,
                            dst_peer_id,
                            dst_addr: dst_addr.clone(),
                        },
                    });

                self.outgoing_relay_reqs
                    .upgrading
                    .insert(request_id, OutgoingUpgradingRelayReq { send_back });
            }
        }

        // Ask the newly-opened connection to be used as destination if relevant.
        if let Some(reqs) = self.incoming_relay_reqs.remove(id) {
            for req in reqs {
                let IncomingRelayReq::DialingDst {
                    src_id,
                    src_addr,
                    request_id,
                    req,
                } = req;
                let event = RelayHandlerIn::OutgoingDstReq {
                    src: src_id,
                    request_id,
                    src_addr,
                    substream: req,
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
        if let Entry::Occupied(o) = self.listeners.entry(*peer_id) {
            if matches!(o.get(), RelayListener::Connecting(_)) {
                // TODO: Should one retry? Should this be logged? Should this be returned to the
                // listener created by transport.rs?
                o.remove_entry();
            }
        }

        if let Some(reqs) = self.outgoing_relay_reqs.dialing.remove(peer_id) {
            for req in reqs {
                let _ = req.send_back.send(Err(OutgoingRelayReqError::DialingRelay));
            }
        }

        if let Some(reqs) = self.incoming_relay_reqs.remove(peer_id) {
            for req in reqs {
                let IncomingRelayReq::DialingDst { src_id, req, .. } = req;
                self.outbox_to_swarm
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: src_id,
                        handler: NotifyHandler::Any,
                        event: RelayHandlerIn::DenyIncomingRelayReq(req),
                    })
            }
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        connection: &ConnectionId,
        _: &ConnectedPoint,
    ) {
        // Remove connection from the set of connections for the given peer. In case the set is
        // empty it will be removed in `inject_disconnected`.
        let was_present = self
            .connected_peers
            .get_mut(peer)
            .expect("`inject_connection_closed` called for connected peer.")
            .remove(connection);
        assert!(
            was_present,
            "`inject_connection_closed` called for known connection"
        );

        if let Some(RelayListener::Connected(primary_connection)) = self.listeners.get(peer) {
            if primary_connection == connection {
                if let Some(new_primary) = self
                    .connected_peers
                    .get(peer)
                    .and_then(|cs| cs.iter().next())
                {
                    self.listeners
                        .insert(*peer, RelayListener::Connected(*new_primary));
                    self.outbox_to_swarm
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id: *peer,
                            handler: NotifyHandler::One(*new_primary),
                            event: RelayHandlerIn::UsedForListening(true),
                        });
                } else {
                    // There are no more connections to the relay left that
                    // could be promoted as primary.

                    // TODO: Should one retry? Should this be logged? Should
                    // this be returned to the listener created by transport.rs?
                }
            }
        }
    }

    fn inject_addr_reach_failure(
        &mut self,
        _peer_id: Option<&PeerId>,
        _addr: &Multiaddr,
        _error: &dyn std::error::Error,
    ) {
        // Handled in `inject_dial_failure`.
    }

    fn inject_listener_error(&mut self, _id: ListenerId, _err: &(dyn std::error::Error + 'static)) {
    }

    fn inject_listener_closed(&mut self, _id: ListenerId, _reason: Result<(), &std::io::Error>) {}

    fn inject_disconnected(&mut self, id: &PeerId) {
        self.connected_peers.remove(id);

        if let Some(reqs) = self.incoming_relay_reqs.remove(id) {
            for req in reqs {
                let IncomingRelayReq::DialingDst { src_id, req, .. } = req;
                self.outbox_to_swarm
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: src_id,
                        handler: NotifyHandler::Any,
                        event: RelayHandlerIn::DenyIncomingRelayReq(req),
                    })
            }
        }
    }

    fn inject_event(
        &mut self,
        event_source: PeerId,
        _connection: ConnectionId,
        event: RelayHandlerEvent,
    ) {
        match event {
            // Remote wants us to become a relay.
            RelayHandlerEvent::IncomingRelayReq {
                request_id,
                src_addr,
                req,
            } => {
                if self.connected_peers.get(&req.dst_peer().peer_id).is_some() {
                    let dest_id = req.dst_peer().peer_id;
                    let event = RelayHandlerIn::OutgoingDstReq {
                        src: event_source,
                        request_id,
                        src_addr,
                        substream: req,
                    };
                    self.outbox_to_swarm
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id: dest_id,
                            handler: NotifyHandler::Any,
                            event,
                        });
                } else {
                    let dest_id = req.dst_peer().peer_id;
                    self.incoming_relay_reqs.entry(dest_id).or_default().push(
                        IncomingRelayReq::DialingDst {
                            request_id,
                            req,
                            src_id: event_source,
                            src_addr,
                        },
                    );
                    self.outbox_to_swarm
                        .push_back(NetworkBehaviourAction::DialPeer {
                            peer_id: dest_id,
                            condition: DialPeerCondition::NotDialing,
                        });
                }
            }
            // Remote wants us to become a destination.
            RelayHandlerEvent::IncomingDstReq(src, request_id) => {
                let send_back = RelayHandlerIn::AcceptDstReq(src, request_id);
                self.outbox_to_swarm
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id: event_source,
                        handler: NotifyHandler::Any,
                        event: send_back,
                    });
            }
            RelayHandlerEvent::OutgoingRelayReqError(_dst_peer_id, request_id) => {
                self.outgoing_relay_reqs
                    .upgrading
                    .remove(&request_id)
                    .unwrap();
            }
            RelayHandlerEvent::OutgoingRelayReqSuccess(_dst, request_id, stream) => {
                let send_back = self
                    .outgoing_relay_reqs
                    .upgrading
                    .remove(&request_id)
                    .map(|OutgoingUpgradingRelayReq { send_back, .. }| send_back)
                    .unwrap();
                send_back.send(Ok(stream)).unwrap();
            }
            RelayHandlerEvent::IncomingDstReqSuccess { stream, src_peer_id, relay_addr } => self
                .outbox_to_transport
                .push(BehaviourToTransportMsg::IncomingRelayedConnection { stream, src_peer_id, relay_addr }),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        poll_parameters: &mut impl PollParameters,
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
                Poll::Ready(Some(TransportToBehaviourMsg::DialReq {
                    request_id,
                    relay_addr,
                    relay_peer_id,
                    dst_addr,
                    dst_peer_id,
                    send_back,
                })) => {
                    if let Some(_) = self.connected_peers.get(&relay_peer_id) {
                        // In case we are already listening via the relay, prefer the primary
                        // connection.
                        let handler = self
                            .listeners
                            .get(&relay_peer_id)
                            .and_then(|s| {
                                if let RelayListener::Connected(id) = s {
                                    Some(NotifyHandler::One(*id))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(NotifyHandler::Any);
                        self.outbox_to_swarm
                            .push_back(NetworkBehaviourAction::NotifyHandler {
                                peer_id: relay_peer_id,
                                handler,
                                event: RelayHandlerIn::OutgoingRelayReq {
                                    request_id,
                                    src_peer_id: *poll_parameters.local_peer_id(),
                                    dst_peer_id: dst_peer_id.clone(),
                                    dst_addr: dst_addr.clone(),
                                },
                            });

                        self.outgoing_relay_reqs
                            .upgrading
                            .insert(request_id, OutgoingUpgradingRelayReq { send_back });
                    } else {
                        self.outgoing_relay_reqs
                            .dialing
                            .entry(relay_peer_id)
                            .or_default()
                            .push(OutgoingDialingRelayReq {
                                src_peer_id: *poll_parameters.local_peer_id(),
                                request_id,
                                relay_addr,
                                dst_addr,
                                dst_peer_id,
                                send_back,
                            });
                        return Poll::Ready(NetworkBehaviourAction::DialPeer {
                            peer_id: relay_peer_id,
                            condition: DialPeerCondition::Disconnected,
                        });
                    }
                }
                Poll::Ready(Some(TransportToBehaviourMsg::ListenReq { address, peer_id })) => {
                    if let Some(connections) = self.connected_peers.get(&peer_id) {
                        let primary_connection =
                            connections.iter().next().expect("At least one connection.");
                        let prev = self
                            .listeners
                            .insert(peer_id, RelayListener::Connected(*primary_connection));
                        assert!(
                            prev.is_none(),
                            "Not to attempt to listen via same relay twice."
                        );

                        self.outbox_to_swarm
                            .push_back(NetworkBehaviourAction::NotifyHandler {
                                peer_id: peer_id,
                                handler: NotifyHandler::One(*primary_connection),
                                event: RelayHandlerIn::UsedForListening(true),
                            });
                    } else {
                        let prev = self
                            .listeners
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
        stream: protocol::Connection,
        src_peer_id: PeerId,
        relay_addr: Multiaddr,
    },
}

enum RelayListener {
    Connecting(Multiaddr),
    Connected(ConnectionId),
}

#[derive(Debug, Eq, PartialEq)]
pub enum OutgoingRelayReqError {
    DialingRelay,
}
