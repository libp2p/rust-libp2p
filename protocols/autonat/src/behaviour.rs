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

/// Config for the [`Behaviour`].
pub struct Config {
    // Timeout for requests.
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
    /// Set the timeout for dial-requests.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Network Behaviour for AutoNAT.
pub struct Behaviour {
    // Inner protocol for sending requests and receiving the response.
    inner: RequestResponse<AutoNatCodec>,
    // Local listening addresses with a score indicating their reachability.
    // The score increases each time a remote peer successfully dials this address.
    addresses: HashMap<Multiaddr, FiniteAddrScore>,
    // Ongoing inbound requests, where no response has been sent back to the remote yet.
    ongoing_inbound: HashMap<PeerId, ResponseChannel<DialResponse>>,
    // Ongoing outbound dial-requests, where no response has been received from the remote yet.
    ongoing_outbound: HashSet<PeerId>,
    // Recently connected peers to which we want to send a dial-request.
    pending_requests: VecDeque<PeerId>,
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
            addresses: HashMap::default(),
            ongoing_inbound: HashMap::default(),
            ongoing_outbound: HashSet::default(),
            pending_requests: VecDeque::default(),
        }
    }

    /// Add a new address to the address list that is send to remote peers in a dial-request.
    pub fn add_local_address(&mut self, address: Multiaddr) {
        if self.addresses.get(&address).is_none() {
            self.addresses.insert(address, 1);
        }
    }

    /// Get the list of local addresses with their current score.
    ///
    /// The score of an address increases each time a remote peer successfully dialed us via this address.
    /// Therefore higher scores indicate a higher reachability.
    pub fn address_list(&self) -> impl Iterator<Item = (&Multiaddr, &FiniteAddrScore)> {
        self.addresses.iter()
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
        self.inner.inject_disconnected(peer);
        self.ongoing_inbound.remove(peer);
    }

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.inner
            .inject_connection_established(peer, conn, endpoint);

        // Initiate a new dial request if there is none pending.
        if !self.ongoing_outbound.contains(peer) {
            self.pending_requests.push_back(*peer);
        }

        if let ConnectedPoint::Dialer { address } = endpoint {
            if let Some(channel) = self.ongoing_inbound.remove(peer) {
                // Successfully dialed one of the addresses from the remote peer.
                // TODO: Check if the address was part of the list received in the dial-request.
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
    }

    fn inject_address_change(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        self.inner.inject_address_change(peer, conn, old, new);
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
        if let Some(channel) = self.ongoing_inbound.remove(peer_id) {
            // Failed to dial any of the addresses sent by the remote peer in their dial-request.
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
        if !self.addresses.contains_key(addr) {
            self.addresses.insert(addr.clone(), 0);
        }
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_expired_listen_addr(id, addr);
        self.addresses.remove(addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.inner.inject_listener_error(id, err)
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        self.inner.inject_listener_closed(id, reason)
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_new_external_addr(addr);
        // Add the address to the local address list.
        match self.addresses.get_mut(addr) {
            Some(score) if *score == 0 => *score = 1,
            Some(_) => {}
            None => {
                self.addresses.insert(addr.clone(), 1);
            }
        }
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_expired_external_addr(addr);
        self.addresses.remove(addr);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        loop {
            if let Some(peer_id) = self.pending_requests.pop_front() {
                let mut scores: Vec<_> = self.addresses.clone().into_iter().collect();
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
                        // Add all addresses to the address book.
                        for addr in addrs {
                            self.inner.add_address(&peer, addr)
                        }
                        // TODO: Handle if there is already a ongoing request.
                        self.ongoing_inbound.insert(peer_id, channel);
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
                        self.ongoing_outbound.remove(&peer);
                        if let DialResponse::Ok(address) = response {
                            // Increase score of the successfully dialed address.
                            let score = self.addresses.entry(address.clone()).or_insert(1);
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
                    self.ongoing_outbound.remove(&peer);
                }
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::InboundFailure { peer, .. },
                )) => {
                    self.ongoing_inbound.remove(&peer);
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
