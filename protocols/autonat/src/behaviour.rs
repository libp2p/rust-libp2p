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
    multiaddr::Protocol,
    ConnectedPoint, Multiaddr, PeerId,
};
use libp2p_request_response::{
    handler::RequestResponseHandlerEvent, ProtocolSupport, RequestResponse, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage, ResponseChannel,
};
use libp2p_swarm::{
    AddressRecord, DialError, DialPeerCondition, IntoProtocolsHandler, NetworkBehaviour,
    NetworkBehaviourAction, PollParameters,
};
use std::{
    collections::HashMap,
    iter,
    task::{Context, Poll},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
/// Config for the [`Behaviour`].
pub struct Config {
    // Timeout for requests.
    pub timeout: Duration,

    // Config if the peer should frequently re-determine its status.
    pub auto_retry: Option<AutoRetry>,

    // Config if the current peer also serves as server for other peers.
    // In case of `None`, the local peer will never do dial-attempts for other peers.
    pub server: Option<ServerConfig>,
}

/// Automatically retry the current NAT status at a certain frequency.
#[derive(Debug, Clone)]
pub struct AutoRetry {
    // Interval in which the NAT should be tested.
    pub interval: Duration,
    // Max peers to send a dial-request to.
    pub max_peers: usize,
    // Whether the static list of servers should be extended with currently connected peers, up to `max_peers`.
    pub extend_with_connected: bool,
    // Minimum amount of valid response (DialResponse::Ok || ResponseError::DialError)
    // from a remote, for it to count as proper attempt.
    pub min_valid: usize,
    // Required amount of successful dials for an address for it to count as public.
    pub required_success: usize,
}

/// Config if the local peer is a server for other peers and does dial-attempts for them.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Max addresses that are tried per peer.
    pub max_addresses: usize,
    /// Max simultaneous autonat dial-attempts.
    pub max_ongoing: usize,
}

#[derive(Debug, Clone)]
pub enum Reachability {
    Public(Multiaddr),
    Private,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Probe {
    required_success: usize,
    min_valid: usize,
    server_count: usize,
    addresses: HashMap<Multiaddr, usize>,
    /// Disqualified dial-requests where the remote could not be reached, or rejected the dial-request.
    dismissed: usize,
    pending_servers: Vec<PeerId>,
    errors: Vec<(PeerId, ResponseError)>,
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
        errors: Vec<(PeerId, ResponseError)>,
    },
    Unknown {
        server_count: usize,
        tried_addresses: Vec<(Multiaddr, usize)>,
        errors: Vec<(PeerId, ResponseError)>,
    },
}

/// Network Behaviour for AutoNAT.
pub struct Behaviour {
    // Inner protocol for sending requests and receiving the response.
    inner: RequestResponse<AutoNatCodec>,

    // Ongoing inbound requests, where no response has been sent back to the remote yet.
    ongoing_inbound: HashMap<PeerId, (Vec<Multiaddr>, ResponseChannel<DialResponse>)>,

    // Ongoing outbound dial-requests, where no response has been received from the remote yet.
    ongoing_outbound: Option<Probe>,

    // Manually initiated probe.
    pending_probe: Option<Probe>,

    // List of trusted public peers that are probed when attempting to determine the auto-nat status.
    static_servers: Vec<PeerId>,

    // List of connected peers and the address we last observed them at.
    connected: HashMap<PeerId, Multiaddr>,

    reachability: Reachability,

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
            connected: HashMap::default(),
            reachability: Reachability::Unknown,
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
        servers: Vec<PeerId>,
        extend_with_static: bool,
        extend_with_connected: bool,
        max_peers: usize,
        min_valid: usize,
    ) -> bool {
        let probe = self.new_probe(
            required_success,
            servers,
            extend_with_static,
            extend_with_connected,
            max_peers,
            min_valid,
        );
        self.pending_probe.replace(probe);
        self.ongoing_outbound.is_some()
    }

    pub fn reachability(&self) -> Reachability {
        self.reachability.clone()
    }

    pub fn ongoing_probe(&self) -> Option<&Probe> {
        self.ongoing_outbound.as_ref()
    }

    fn new_probe(
        &self,
        required_success: usize,
        mut servers: Vec<PeerId>,
        extend_with_static: bool,
        extend_with_connected: bool,
        max_peers: usize,
        min_valid: usize,
    ) -> Probe {
        servers.truncate(max_peers);
        if servers.len() < max_peers && extend_with_static {
            servers.extend(self.static_servers.clone())
        }
        if servers.len() < max_peers && extend_with_connected {
            let mut connected = self.connected.iter();
            // TODO: use random set
            for _ in 0..connected.len() {
                let (peer, _) = connected.next().unwrap();
                servers.push(*peer);
                if servers.len() >= max_peers {
                    break;
                }
            }
        }
        Probe {
            server_count: servers.len(),
            pending_servers: servers,
            addresses: HashMap::new(),
            required_success,
            dismissed: 0,
            errors: Vec::new(),
            min_valid,
        }
    }

    fn do_probe(&mut self, probe: Probe, mut addr_records: Vec<AddressRecord>) {
        // Sort so that the address with the highest score will be dialed first by the remote.
        addr_records.sort_by(|record_a, record_b| record_b.score.cmp(&record_a.score));
        let addrs: Vec<Multiaddr> = addr_records.into_iter().map(|r| r.addr).collect();
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
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = NatStatus;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        if let Some(addrs) = self.ongoing_inbound.get(peer).map(|(a, _)| a.clone()) {
            addrs
        } else {
            self.inner.addresses_of_peer(peer)
        }
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

        if let ConnectedPoint::Dialer { address } = endpoint {
            if let Some((addrs, _)) = self.ongoing_inbound.get(peer) {
                if addrs.contains(address) {
                    // Successfully dialed one of the addresses from the remote peer.
                    let channel = self.ongoing_inbound.remove(peer).unwrap().1;
                    let _ = self
                        .inner
                        .send_response(channel, DialResponse::Ok(address.clone()));
                    return;
                }
            }
        }
        self.connected
            .insert(*peer, endpoint.get_remote_address().clone());
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
        self.connected.retain(|p, _| p != peer);
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
        if let Some((_, channel)) = peer_id.and_then(|p| self.ongoing_inbound.remove(&p)) {
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
                        request: DialRequest { peer_id, addrs },
                        channel,
                    } => {
                        let server_config = self
                            .server_config
                            .as_ref()
                            .expect("Server config is present.");

                        // Refuse dial if:
                        // - the peer-id in the dial request is not the remote peer,
                        // - there is already an ongoing request to the remote
                        // - max simultaneous autonat dial-requests are reached.
                        if peer != peer_id
                            || self.ongoing_inbound.contains_key(&peer)
                            || self.ongoing_inbound.len() >= server_config.max_ongoing
                        {
                            let response = DialResponse::Err(ResponseError::DialRefused);
                            let _ = self.inner.send_response(channel, response);
                            continue;
                        }

                        let observed_remote_at =
                            self.connected.get(&peer).expect("Peer is connected");
                        let mut addrs = filter_invalid_addrs(peer, addrs, observed_remote_at);
                        addrs.truncate(server_config.max_addresses);
                        if addrs.is_empty() {
                            let response = DialResponse::Err(ResponseError::DialRefused);
                            let _ = self.inner.send_response(channel, response);
                            continue;
                        }

                        self.ongoing_inbound.insert(peer, (addrs, channel));

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
                                        self.reachability = Reachability::Public(addr);
                                        return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                                            status,
                                        ));
                                    }
                                }
                            }
                            DialResponse::Err(err) => {
                                if !matches!(err, ResponseError::DialError) {
                                    probe.dismissed += 1;
                                }
                                probe.errors.push((peer, err));
                                let remaining = probe.pending_servers.len();
                                let current_highest =
                                    probe.addresses.iter().fold(0, |acc, (_, &curr)| {
                                        if acc > curr {
                                            acc
                                        } else {
                                            curr
                                        }
                                    });
                                if remaining < probe.required_success - current_highest
                                    || probe.min_valid > probe.server_count - probe.dismissed
                                {
                                    let probe = self.ongoing_outbound.take().unwrap();
                                    let valid_attempts =
                                        probe.server_count - remaining - probe.dismissed;
                                    let status;
                                    if valid_attempts >= probe.min_valid {
                                        self.reachability = Reachability::Private;
                                        status = NatStatus::Private {
                                            server_count: probe.server_count,
                                            errors: probe.errors,
                                            tried_addresses: probe.addresses.into_iter().collect(),
                                        };
                                    } else {
                                        self.reachability = Reachability::Unknown;
                                        status = NatStatus::Unknown {
                                            server_count: probe.server_count,
                                            errors: probe.errors,
                                            tried_addresses: probe.addresses.into_iter().collect(),
                                        };
                                    }

                                    self.last_probe = Instant::now();
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
                let records: Vec<_> = params.external_addresses().collect();
                self.do_probe(probe, records)
            } else if let Some(auto_retry) = self.auto_retry.as_ref() {
                if Instant::now().duration_since(self.last_probe) > auto_retry.interval {
                    let probe = self.new_probe(
                        auto_retry.required_success,
                        Vec::new(),
                        true,
                        auto_retry.extend_with_connected,
                        auto_retry.max_peers,
                        auto_retry.min_valid,
                    );
                    let records: Vec<_> = params.external_addresses().collect();
                    self.do_probe(probe, records);
                }
            }
        }
    }
}

// Filter demanded dial addresses for validity, to prevent abuse.
fn filter_invalid_addrs(
    peer: PeerId,
    demanded: Vec<Multiaddr>,
    observed_remote_at: &Multiaddr,
) -> Vec<Multiaddr> {
    let observed_ip = match observed_remote_at.into_iter().next() {
        Some(Protocol::Ip4(ip)) => Protocol::Ip4(ip),
        Some(Protocol::Ip6(ip)) => Protocol::Ip6(ip),
        _ => return Vec::new(),
    };
    demanded
        .into_iter()
        .filter_map(|addr| {
            addr.replace(0, |proto| match proto {
                Protocol::Ip4(_) => Some(observed_ip.clone()),
                Protocol::Ip6(_) => Some(observed_ip.clone()),
                _ => None,
            })?;
            // TODO: Add more filters
            addr.iter()
                .all(|proto| match proto {
                    Protocol::P2pCircuit => false,
                    Protocol::P2p(hash) => hash == peer.into(),
                    _ => true,
                })
                .then(|| addr)
        })
        .collect()
}
