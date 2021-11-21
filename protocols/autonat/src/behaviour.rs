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
use futures::FutureExt;
use futures_timer::Delay;
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
    dial_opts::{DialOpts, PeerCondition},
    AddressRecord, DialError, IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction,
    PollParameters,
};
use std::{
    collections::{HashMap, HashSet},
    iter,
    task::{Context, Poll},
    time::Duration,
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

impl From<&NatStatus> for Reachability {
    fn from(status: &NatStatus) -> Self {
        match status {
            NatStatus::Public { address, .. } => Reachability::Public(address.clone()),
            NatStatus::Private { .. } => Reachability::Private,
            NatStatus::Unknown { .. } => Reachability::Unknown,
        }
    }
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

    // List of connected peers.
    connected: Vec<PeerId>,

    reachability: Reachability,

    server_config: Option<ServerConfig>,

    auto_retry: Option<(AutoRetry, Delay)>,
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
        let auto_retry = config
            .auto_retry
            .map(|a| (a, Delay::new(Duration::new(0, 0))));
        Self {
            inner,
            ongoing_inbound: HashMap::default(),
            ongoing_outbound: None,
            pending_probe: None,
            static_servers: Vec::default(),
            connected: Vec::default(),
            reachability: Reachability::Unknown,
            server_config: config.server,
            auto_retry,
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

    pub fn add_server(&mut self, peer: PeerId, address: Option<Multiaddr>) {
        self.static_servers.push(peer);
        if let Some(addr) = address {
            self.inner.add_address(&peer, addr);
        }
    }

    pub fn remove_server(&mut self, peer: &PeerId) {
        self.static_servers.retain(|p| p != peer)
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
                let peer = connected.next().unwrap();
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

    fn do_probe(&mut self, probe: Probe, addr_records: Vec<AddressRecord>) {
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

    // Handle the inbound request and collect the valid addresses to be dialed.
    fn handle_request(&mut self, sender: PeerId, request: DialRequest) -> Option<Vec<Multiaddr>> {
        let config = self
            .server_config
            .as_ref()
            .expect("Server config is present.");

        // Validate that the peer to be dialed is the request's sender.
        if request.peer_id != sender {
            return None;
        }
        // Check that there is no ongoing dial to the remote.
        if self.ongoing_inbound.contains_key(&sender) {
            return None;
        }
        // Check if max simultaneous autonat dial-requests are reached.
        if self.ongoing_inbound.len() >= config.max_ongoing {
            return None;
        }

        let known_addrs = self.inner.addresses_of_peer(&sender);
        // At least one observed address was added, either in the `RequestResponse` protocol
        // if we dialed the remote, or in Self::inject_connection_established if the
        // remote dialed us.
        let observed_addr = known_addrs.first().expect("An address is known.");
        // Filter valid addresses.
        let mut addrs = filter_valid_addrs(sender, request.addrs, observed_addr);
        addrs.truncate(config.max_addresses);

        if addrs.is_empty() {
            return None;
        }

        Some(addrs)
    }

    fn handle_response(
        &mut self,
        sender: PeerId,
        response: Option<DialResponse>,
    ) -> Option<NatStatus> {
        let mut probe = self.ongoing_outbound.take()?;
        if !probe.pending_servers.contains(&sender) {
            let _ = self.ongoing_outbound.insert(probe);
            return None;
        };
        probe.pending_servers.retain(|p| p != &sender);
        match response {
            Some(DialResponse::Ok(addr)) => self.handle_dial_ok(probe, addr),
            Some(DialResponse::Err(err)) => {
                if !matches!(err, ResponseError::DialError) {
                    probe.dismissed += 1;
                }
                probe.errors.push((sender, err));
                self.handle_dial_err(probe)
            }
            None => {
                probe.dismissed += 1;
                self.handle_dial_err(probe)
            }
        }
    }

    fn handle_dial_ok(&mut self, mut probe: Probe, address: Multiaddr) -> Option<NatStatus> {
        let score = probe.addresses.get_mut(&address)?;
        *score += 1;
        if *score < probe.required_success {
            let _ = self.ongoing_outbound.insert(probe);
            return None;
        }
        Some(NatStatus::Public {
            address: address.clone(),
            server_count: probe.server_count,
            successes: *score,
        })
    }

    fn handle_dial_err(&mut self, probe: Probe) -> Option<NatStatus> {
        let remaining = probe.pending_servers.len();
        let current_highest = probe.addresses.iter().max_by(|(_, a), (_, b)| a.cmp(b))?.1;

        // Check if the probe can still be successful.
        if remaining >= probe.required_success - current_highest {
            let _ = self.ongoing_outbound.insert(probe);
            return None;
        }

        let valid_attempts = probe.server_count - remaining - probe.dismissed;
        if valid_attempts >= probe.min_valid {
            Some(NatStatus::Private {
                server_count: probe.server_count,
                errors: probe.errors,
                tried_addresses: probe.addresses.into_iter().collect(),
            })
        } else {
            Some(NatStatus::Unknown {
                server_count: probe.server_count,
                errors: probe.errors,
                tried_addresses: probe.addresses.into_iter().collect(),
            })
        }
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

        match endpoint {
            ConnectedPoint::Dialer { address } => {
                if let Some((addrs, _)) = self.ongoing_inbound.get(peer) {
                    if addrs.contains(address) {
                        // Successfully dialed one of the addresses from the remote peer.
                        let channel = self.ongoing_inbound.remove(peer).unwrap().1;
                        let _ = self
                            .inner
                            .send_response(channel, DialResponse::Ok(address.clone()));
                    }
                }
            }
            ConnectedPoint::Listener { send_back_addr, .. } => {
                // `RequestResponse::addresses_of_peer` only includes addresses from connected peers
                // where the remote is ConnectedPoint::Dialer.
                // Add the send_back_addr so its observed ip can be used for `filter_valid_addrs`.
                self.inner.add_address(peer, send_back_addr.clone());
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
        if let ConnectedPoint::Listener { send_back_addr, .. } = new {
            self.inner.add_address(peer, send_back_addr.clone());
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
        if let Some((ref auto_retry, ref mut delay)) = self.auto_retry {
            if delay.poll_unpin(cx).is_ready()
                && self.ongoing_outbound.is_none()
                && self.pending_probe.is_none()
            {
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
        };
        if let Some(probe) = self.pending_probe.take() {
            let records: Vec<_> = params.external_addresses().collect();
            self.do_probe(probe, records);
        }
        loop {
            match self.inner.poll(cx, params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(event)) => match event {
                    RequestResponseEvent::Message { peer, message } => match message {
                        RequestResponseMessage::Request {
                            request_id: _,
                            request,
                            channel,
                        } => match self.handle_request(peer, request) {
                            Some(addrs) => {
                                self.ongoing_inbound.insert(peer, (addrs.clone(), channel));
                                return Poll::Ready(NetworkBehaviourAction::Dial {
                                    opts: DialOpts::peer_id(peer)
                                        .condition(PeerCondition::Always)
                                        .addresses(addrs)
                                        .build(),
                                    handler: self.inner.new_handler(),
                                });
                            }
                            None => {
                                let response = DialResponse::Err(ResponseError::DialRefused);
                                let _ = self.inner.send_response(channel, response);
                            }
                        },
                        RequestResponseMessage::Response {
                            request_id: _,
                            response,
                        } => {
                            if let Some(status) = self.handle_response(peer, Some(response)) {
                                self.reachability = Reachability::from(&status);
                                if let Some((ref auto_retry, ref mut delay)) = self.auto_retry {
                                    delay.reset(auto_retry.interval);
                                }
                                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(status));
                            }
                        }
                    },
                    RequestResponseEvent::ResponseSent { .. } => {}
                    RequestResponseEvent::OutboundFailure { peer, .. } => {
                        if let Some(status) = self.handle_response(peer, None) {
                            self.reachability = Reachability::from(&status);
                            if let Some((ref auto_retry, ref mut delay)) = self.auto_retry {
                                delay.reset(auto_retry.interval);
                            }
                            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(status));
                        }
                    }
                    RequestResponseEvent::InboundFailure { peer, .. } => {
                        self.ongoing_inbound.remove(&peer);
                    }
                },
                Poll::Ready(action) => return Poll::Ready(action.map_out(|_| unreachable!())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// Filter demanded dial addresses for validity, to prevent abuse.
fn filter_valid_addrs(
    peer: PeerId,
    demanded: Vec<Multiaddr>,
    observed_remote_at: &Multiaddr,
) -> Vec<Multiaddr> {
    let observed_ip = observed_remote_at.into_iter().find(|p| match p {
        Protocol::Ip4(ip) => {
            // NOTE: The below logic is copied from `std::net::Ipv4Addr::is_global`, which at the current
            // point is behind the unstable `ip` feature.
            // See https://github.com/rust-lang/rust/issues/27709 for more info.
            // The check for the unstable `Ipv4Addr::is_benchmarking` and `Ipv4Addr::is_reserved `
            // were skipped, because they should never occur in an observed address.

            // check if this address is 192.0.0.9 or 192.0.0.10. These addresses are the only two
            // globally routable addresses in the 192.0.0.0/24 range.
            if u32::from_be_bytes(ip.octets()) == 0xc0000009
                || u32::from_be_bytes(ip.octets()) == 0xc000000a
            {
                return true;
            }

            let is_shared = ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000 == 0b0100_0000);

            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && !is_shared
        }
        Protocol::Ip6(_) => {
            // TODO: filter addresses for global ones
            true
        }
        _ => false,
    });
    let observed_ip = match observed_ip {
        Some(ip) => ip,
        None => return Vec::new(),
    };
    let mut distinct = HashSet::new();
    demanded
        .into_iter()
        .filter_map(|addr| {
            // Replace the demanded ip with the observed one.
            let i = addr
                .iter()
                .position(|p| matches!(p, Protocol::Ip4(_) | Protocol::Ip6(_)))?;
            addr.replace(i, |_| Some(observed_ip.clone()))?;
            // Filter relay addresses and addresses with invalid peer id.
            let is_valid = addr.iter().all(|proto| match proto {
                Protocol::P2pCircuit => false,
                Protocol::P2p(hash) => hash == peer.into(),
                _ => true,
            });

            if !is_valid {
                return None;
            }
            // Only collect distinct addresses.
            distinct.insert(addr.clone()).then(|| addr)
        })
        .collect()
}
