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
    DialError, DialPeerCondition, IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction,
    PollParameters,
};
use std::{
    collections::{HashMap, VecDeque},
    iter,
    task::{Context, Poll},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
/// Config for the [`Behaviour`].
pub struct Config {
    // Timeout for requests.
    timeout: Duration,

    // Config if the peer should frequently re-determine its status.
    auto_retry: Option<AutoRetry>,

    // Config if the current peer also serves as server for other peers./
    // In case of `None`, the local peer will never do dial-attempts to other peers.
    server: Option<ServerConfig>,
}

#[derive(Debug, Clone)]
pub struct AutoRetry {
    interval: Duration,
    min_peers: usize,
    max_peers: usize,
    required_success: usize,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    max_addresses: usize,
    max_ongoing: usize,
}

#[derive(Debug, Clone)]
pub enum Reachability {
    Public(Multiaddr),
    Private,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Probe {
    server_count: usize,
    pending_servers: Vec<PeerId>,
    addresses: HashMap<Multiaddr, usize>,
    required_success: usize,
}

#[derive(Debug, Clone)]
pub enum NatStatus {
    Public {
        address: Multiaddr,
        server_count: usize,
        successes: usize,
    },
    Private {
        server_count: usize,
        tried_addresses: Vec<(Multiaddr, usize)>,
    },
}

/// Network Behaviour for AutoNAT.
pub struct Behaviour {
    // Inner protocol for sending requests and receiving the response.
    inner: RequestResponse<AutoNatCodec>,

    // Ongoing inbound requests, where no response has been sent back to the remote yet.
    ongoing_inbound: HashMap<PeerId, ResponseChannel<DialResponse>>,

    // Ongoing outbound dial-requests, where no response has been received from the remote yet.
    ongoing_outbound: Option<Probe>,

    // Manually initiated probe.
    pending_probe: Option<Probe>,

    // List of trusted public peers that are probed when attempting to determine the auto-nat status.
    static_servers: Vec<PeerId>,

    connected: Vec<PeerId>,

    status: Reachability,

    server_config: Option<ServerConfig>,

    auto_retry: Option<AutoRetry>,

    last_probe: Instant,
}

impl Behaviour {
    pub fn new(config: Config) -> Self {
        let proto_support = match config.server {
            Some(_) => ProtocolSupport::Full,
            None => ProtocolSupport::Outbound,
        };
        let protocols = iter::once((AutoNatProtocol, proto_support));
        let mut cfg = RequestResponseConfig::default();
        cfg.set_request_timeout(config.timeout);
        let inner = RequestResponse::new(AutoNatCodec, protocols, cfg);
        Self {
            inner,
            ongoing_inbound: HashMap::default(),
            ongoing_outbound: None,
            pending_probe: None,
            static_servers: Vec::default(),
            connected: Vec::default(),
            status: Reachability::Unknown,
            server_config: config.server,
            auto_retry: None,
            last_probe: Instant::now(),
        }
    }

    // Manually retry determination of NAT status.
    //
    // Return whether there is already an ongoing Probe.
    pub fn retry_nat_status(
        &mut self,
        required_success: usize,
        mut servers: Vec<PeerId>,
        extend_with_static: bool,
        extend_with_connected: Option<usize>,
    ) -> bool {
        if extend_with_static {
            servers.extend(self.static_servers.clone())
        }
        if let Some(count) = extend_with_connected {
            // TODO: use random set instead
            for i in 0..count {
                match self.connected.get(i) {
                    Some(peer) => servers.push(*peer),
                    None => break,
                }
            }
        }
        let probe = Probe {
            server_count: servers.len(),
            pending_servers: servers,
            addresses: HashMap::new(),
            required_success,
        };
        self.pending_probe.replace(probe);
        self.ongoing_outbound.is_some()
    }

    pub fn reachability(&self) -> Reachability {
        self.status.clone()
    }

    pub fn ongoing_probe(&self) -> Option<&Probe> {
        self.ongoing_outbound.as_ref()
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = NatStatus;

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
        failed_addresses: Option<&Vec<Multiaddr>>,
    ) {
        self.inner
            .inject_connection_established(peer, conn, endpoint, failed_addresses);
        self.connected.push(*peer);

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
        self.connected.retain(|p| p != peer);
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

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ProtocolsHandler,
        error: &DialError,
    ) {
        self.inner.inject_dial_failure(peer_id, handler, error);
        if let Some(channel) = peer_id.and_then(|p| self.ongoing_inbound.remove(&p)) {
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
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_expired_listen_addr(id, addr);
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.inner.inject_listener_error(id, err)
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        self.inner.inject_listener_closed(id, reason)
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_new_external_addr(addr);
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_expired_external_addr(addr);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        loop {
            match self.inner.poll(cx, params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::Message { peer, message },
                )) => match message {
                    RequestResponseMessage::Request {
                        request_id: _,
                        request:
                            DialRequest {
                                peer_id: _peer,
                                addrs: _addrs,
                            },
                        channel,
                    } => {
                        let server_config = self.server_config.as_ref().unwrap();
                        if self.ongoing_inbound.len() >= server_config.max_ongoing {
                            let response = DialResponse::Err(ResponseError::DialRefused);
                            let _ = self.inner.send_response(channel, response);
                            continue;
                        }
                        let _observed_remote_at = todo!();
                        let mut _addrs = valid_addresses(_addrs, _observed_remote_at);
                        if _addrs.len() > server_config.max_addresses {
                            _addrs.drain(server_config.max_addresses..);
                        }
                        if _addrs.is_empty() {
                            let response = DialResponse::Err(ResponseError::DialRefused);
                            let _ = self.inner.send_response(channel, response);
                            continue;
                        }
                        // Add all addresses to the address book.
                        for addr in _addrs {
                            self.inner.add_address(&peer, addr)
                        }
                        // TODO: Handle if there is already a ongoing request.
                        self.ongoing_inbound.insert(_peer, channel);
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
                        let probe = match self.ongoing_outbound.as_mut() {
                            Some(p) if p.pending_servers.contains(&peer) => p,
                            _ => continue,
                        };
                        probe.pending_servers.retain(|p| p != &peer);
                        match response {
                            DialResponse::Ok(addr) => {
                                if let Some(score) = probe.addresses.get_mut(&addr) {
                                    *score += 1;
                                    if *score >= probe.required_success {
                                        let score = *score;
                                        let probe = self.ongoing_outbound.take().unwrap();
                                        self.last_probe = Instant::now();
                                        let status = NatStatus::Public {
                                            address: addr.clone(),
                                            server_count: probe.server_count,
                                            successes: score,
                                        };
                                        self.status = Reachability::Public(addr);
                                        return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                                            status,
                                        ));
                                    }
                                }
                            }
                            // TODO: Handle errors due to bad request, rejected, etc.
                            DialResponse::Err(_) => {
                                if probe.pending_servers.is_empty() {
                                    let probe = self.ongoing_outbound.take().unwrap();
                                    self.last_probe = Instant::now();
                                    let status = NatStatus::Private {
                                        server_count: probe.server_count,
                                        tried_addresses: probe.addresses.into_iter().collect(),
                                    };
                                    self.status = Reachability::Private;
                                    return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                                        status,
                                    ));
                                }
                            }
                        }
                    }
                },
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::ResponseSent { .. },
                )) => {}
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::OutboundFailure { peer, .. },
                )) => {
                    let probe = match self.ongoing_outbound.as_mut() {
                        Some(p) if p.pending_servers.contains(&peer) => p,
                        _ => continue,
                    };
                    probe.pending_servers.retain(|p| p != &peer);
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
            if let Some(probe) = self.pending_probe.take() {
                let mut records: Vec<_> = params.external_addresses().collect();
                // Sort so that the address with the highest score will be dialed first by the remote.
                records.sort_by(|record_a, record_b| record_b.score.cmp(&record_a.score));
                let addrs: Vec<Multiaddr> = records.into_iter().map(|r| r.addr).collect();
                for peer_id in probe.pending_servers.clone() {
                    self.inner.send_request(
                        &peer_id,
                        DialRequest {
                            peer_id,
                            addrs: addrs.clone(),
                        },
                    );
                }
                let _ = self.ongoing_outbound.insert(probe);
            } else if let Some(auto_retry) = self.auto_retry.as_ref() {
                if Instant::now().duration_since(self.last_probe) > auto_retry.interval {
                    // TODO: auto retry nat status
                }
            }
        }
    }
}

// Filter demanded dial addresses for validity, to prevent abuse.
fn valid_addresses(_demanded: Vec<Multiaddr>, _observed_remote_at: Multiaddr) -> Vec<Multiaddr> {
    todo!()
}
