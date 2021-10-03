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

use crate::protocol::{AutoNatCodec, AutoNatProtocol, DialRequest, DialResponse, ResponseError};
use libp2p_core::{
    connection::{ConnectionId, ListenerId},
    ConnectedPoint, Multiaddr, PeerId,
};
use libp2p_request_response::{
    handler::RequestResponseHandlerEvent, ProtocolSupport, RequestResponse, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage, ResponseChannel,
};
use libp2p_swarm::{
    AddressScore, DialPeerCondition, IntoProtocolsHandler, NetworkBehaviour,
    NetworkBehaviourAction, PollParameters,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter,
    task::{Context, Poll},
    time::Duration,
};

type FiniteAddrScore = u32;

pub struct Config {
    timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timeout: Duration::from_secs(5),
        }
    }
}

impl Config {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

pub struct Behaviour {
    inner: RequestResponse<AutoNatCodec>,
    local_addresses: HashMap<Multiaddr, FiniteAddrScore>,
    pending_inbound: HashMap<PeerId, ResponseChannel<DialResponse>>,
    pending_outbound: HashSet<PeerId>,
    send_request: VecDeque<PeerId>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Behaviour::new(Config::default())
    }
}

impl Behaviour {
    pub fn new(config: Config) -> Self {
        let protocols = iter::once((AutoNatProtocol, ProtocolSupport::Full));
        let mut cfg = RequestResponseConfig::default();
        cfg.set_request_timeout(config.timeout);
        let inner = RequestResponse::new(AutoNatCodec, protocols, cfg);
        Self {
            inner,
            local_addresses: HashMap::default(),
            pending_inbound: HashMap::default(),
            pending_outbound: HashSet::default(),
            send_request: VecDeque::default(),
        }
    }

    pub fn add_local_address(&mut self, address: Multiaddr) {
        if self.local_addresses.get(&address).is_none() {
            self.local_addresses.insert(address, 1);
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = ();

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.inner.addresses_of_peer(peer)
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        self.inner.inject_connected(peer)
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        self.inner.inject_disconnected(peer)
    }

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.inner
            .inject_connection_established(peer, conn, endpoint);
        if !self.pending_outbound.contains(peer) {
            self.send_request.push_back(*peer);
        }
        if let ConnectedPoint::Dialer { address } = endpoint {
            if let Some(channel) = self.pending_inbound.remove(peer) {
                let _ = self
                    .inner
                    .send_response(channel, DialResponse::Ok(address.clone()));
            }
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
        handler: <Self::ProtocolsHandler as IntoProtocolsHandler>::Handler,
    ) {
        self.inner
            .inject_connection_closed(peer, conn, endpoint, handler);
        // Channel can be dropped, as the underlying substream already closed.
        self.pending_inbound.remove(peer);
        self.send_request.retain(|p| p != peer);
    }

    fn inject_address_change(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        self.inner.inject_address_change(peer, conn, old, new);
        if let ConnectedPoint::Listener {
            local_addr: old_addr,
            ..
        } = old
        {
            match new {
                ConnectedPoint::Listener {
                    local_addr: new_addr,
                    ..
                } if old_addr != new_addr => {
                    self.local_addresses.remove(old_addr);
                    if !self.local_addresses.contains_key(new_addr) {
                        self.local_addresses.insert(new_addr.clone(), 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        conn: ConnectionId,
        event: RequestResponseHandlerEvent<AutoNatCodec>,
    ) {
        self.inner.inject_event(peer_id, conn, event)
    }

    fn inject_addr_reach_failure(
        &mut self,
        peer_id: Option<&PeerId>,
        addr: &Multiaddr,
        error: &dyn std::error::Error,
    ) {
        self.inner.inject_addr_reach_failure(peer_id, addr, error)
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: &PeerId,
        handler: Self::ProtocolsHandler,
        error: libp2p_swarm::DialError,
    ) {
        self.inner.inject_dial_failure(peer_id, handler, error);
        if let Some(channel) = self.pending_inbound.remove(peer_id) {
            let _ = self
                .inner
                .send_response(channel, DialResponse::Err(ResponseError::DialError));
        }
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &Multiaddr,
        send_back_addr: &Multiaddr,
        handler: Self::ProtocolsHandler,
    ) {
        self.inner
            .inject_listen_failure(local_addr, send_back_addr, handler)
    }

    fn inject_new_listener(&mut self, id: ListenerId) {
        self.inner.inject_new_listener(id)
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_new_listen_addr(id, addr);
        if !self.local_addresses.contains_key(addr) {
            self.local_addresses.insert(addr.clone(), 0);
        }
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_expired_listen_addr(id, addr);
        self.local_addresses.remove(addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.inner.inject_listener_error(id, err)
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        self.inner.inject_listener_closed(id, reason)
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_new_external_addr(addr);
        match self.local_addresses.get_mut(addr) {
            Some(score) if *score == 0 => *score = 1,
            Some(_) => {}
            None => {
                self.local_addresses.insert(addr.clone(), 1);
            }
        }
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_expired_external_addr(addr);
        self.local_addresses.remove(addr);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        loop {
            if let Some(peer_id) = self.send_request.pop_front() {
                let mut scores: Vec<(Multiaddr, FiniteAddrScore)> =
                    self.local_addresses.clone().into_iter().collect();
                // Sort so that the address with the highest score will be dialed first by the remote.
                scores.sort_by(|(_, score_a), (_, score_b)| score_b.cmp(score_a));
                let addrs = scores.into_iter().map(|(a, _)| a).collect();
                self.inner
                    .send_request(&peer_id, DialRequest { peer_id, addrs });
            }
            match self.inner.poll(cx, params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::Message { peer, message },
                )) => match message {
                    RequestResponseMessage::Request {
                        request_id: _,
                        request: DialRequest { peer_id, addrs },
                        channel,
                    } => {
                        for addr in addrs {
                            self.inner.add_address(&peer, addr)
                        }
                        // TODO: Handle if there is already a pending request.
                        self.pending_inbound.insert(peer_id, channel);
                        return Poll::Ready(NetworkBehaviourAction::DialPeer {
                            peer_id: peer,
                            handler: self.inner.new_handler(),
                            condition: DialPeerCondition::Always,
                        });
                    }
                    RequestResponseMessage::Response {
                        request_id: _,
                        response,
                    } => {
                        self.pending_outbound.remove(&peer);
                        if let DialResponse::Ok(address) = response {
                            let score = self.local_addresses.entry(address.clone()).or_insert(1);
                            *score += 1;
                            return Poll::Ready(NetworkBehaviourAction::ReportObservedAddr {
                                address,
                                score: AddressScore::Finite(*score),
                            });
                        }
                    }
                },
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::ResponseSent { .. },
                )) => {}
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::OutboundFailure { peer, .. },
                )) => {
                    self.pending_outbound.remove(&peer);
                }
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::InboundFailure { peer, .. },
                )) => {
                    self.pending_inbound.remove(&peer);
                }
                Poll::Ready(NetworkBehaviourAction::DialAddress { address, handler }) => {
                    return Poll::Ready(NetworkBehaviourAction::DialAddress { address, handler })
                }
                Poll::Ready(NetworkBehaviourAction::DialPeer {
                    peer_id,
                    condition,
                    handler,
                }) => {
                    return Poll::Ready(NetworkBehaviourAction::DialPeer {
                        peer_id,
                        condition,
                        handler,
                    })
                }
                Poll::Ready(NetworkBehaviourAction::CloseConnection {
                    peer_id,
                    connection,
                }) => {
                    return Poll::Ready(NetworkBehaviourAction::CloseConnection {
                        peer_id,
                        connection,
                    })
                }
                Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                }) => {
                    return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler,
                        event,
                    })
                }
                Poll::Ready(NetworkBehaviourAction::ReportObservedAddr { address, score }) => {
                    return Poll::Ready(NetworkBehaviourAction::ReportObservedAddr {
                        address,
                        score,
                    })
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
