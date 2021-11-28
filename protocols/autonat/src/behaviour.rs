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
    handler::RequestResponseHandlerEvent, OutboundFailure, ProtocolSupport, RequestId,
    RequestResponse, RequestResponseConfig, RequestResponseEvent, RequestResponseMessage,
    ResponseChannel,
};
use libp2p_swarm::{
    dial_opts::{DialOpts, PeerCondition},
    AddressScore, DialError, IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction,
    PollParameters,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter,
    task::{Context, Poll},
    time::Duration,
};

/// Config for the [`Behaviour`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Timeout for requests.
    pub timeout: Duration,

    /// Config if the peer should frequently re-determine its status.
    pub auto_probe: Option<AutoProbe>,

    /// Config if the current peer also serves as server for other peers.
    /// In case of `None`, the local peer will never do dial-attempts for other peers.
    pub server: Option<ServerConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timeout: Duration::from_secs(30),
            auto_probe: Some(AutoProbe::default()),
            server: Some(ServerConfig::default()),
        }
    }
}

/// Automatically retry the current NAT status at a certain frequency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoProbe {
    /// Interval in which the NAT should be tested.
    pub interval: Duration,
    /// Config for the frequent probes.
    pub config: ProbeConfig,
}

impl Default for AutoProbe {
    fn default() -> Self {
        AutoProbe {
            interval: Duration::from_secs(90),
            config: ProbeConfig::default(),
        }
    }
}

/// Config if the local peer may serve a server for other peers and do dial-attempts for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// Max addresses that are tried per peer.
    pub max_addresses: usize,
    /// Max simultaneous autonat dial-attempts.
    pub max_ongoing: usize,
    /// Dial only addresses in public IP range.
    pub only_public: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            max_addresses: 10,
            max_ongoing: 10,
            only_public: true,
        }
    }
}

/// Current reachability derived from the most recent probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reachability {
    Public(Multiaddr),
    Private,
    Unknown,
}

impl Reachability {
    /// Whether we can assume ourself to be public
    pub fn is_public(&self) -> bool {
        matches!(self, Reachability::Public(_))
    }
}

impl From<&NatStatus> for Reachability {
    fn from(status: &NatStatus) -> Self {
        status.reachability.clone()
    }
}

/// Identifier for a NAT-status probe.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeId(u64);

/// Single probe for determining out NAT-Status
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    id: ProbeId,
    // Min. required Dial successes or failures for us to determine out status.
    // If the confidence can not be reached, out reachability will be `Unknown`
    min_confidence: usize,
    // Number of servers to which we sent a dial request.
    server_count: usize,
    // External addresses and the current number ob successful dials to them reported by remote peers.
    //
    // Apart from our demanded addresses this may also contains addresses that the remote observed us at.
    addresses: HashMap<Multiaddr, usize>,
    // Serves from which we did not receive a response yet.
    pending_servers: Vec<(PeerId, Option<RequestId>)>,
    // Received response errors.
    errors: Vec<(PeerId, ResponseError)>,
    // Failed requests.
    outbound_failures: Vec<(PeerId, OutboundFailure)>,
}

impl Probe {
    // Evaluate current state to whether we already have enough results to derive the NAT status.
    fn evaluate(self) -> Result<NatStatus, Self> {
        // Find highest score of successful dials for an address.
        let (address, highest) = match self.addresses.iter().max_by(|(_, a), (_, b)| a.cmp(b)) {
            Some(max) => max,
            None => return Ok(self.into_nat_status(Reachability::Unknown)),
        };

        // Check if the required amount of successful dials was reached.
        if *highest >= self.min_confidence {
            let addr = address.clone();
            return Ok(self.into_nat_status(Reachability::Public(addr)));
        }

        // Check if the probe can still be successful.
        if self.pending_servers.len() >= self.min_confidence - highest {
            return Err(self);
        }

        let error_response_count = self
            .errors
            .iter()
            .filter(|(_, e)| matches!(e, ResponseError::DialError))
            .count();

        // Check if enough errors were received to reach min confidence.
        if error_response_count >= self.min_confidence {
            return Ok(self.into_nat_status(Reachability::Private));
        }

        // Check if min confidence for errors can still be reached.
        if self.pending_servers.len() >= self.min_confidence - error_response_count {
            return Err(self);
        }

        Ok(self.into_nat_status(Reachability::Unknown))
    }

    fn into_nat_status(self, reachability: Reachability) -> NatStatus {
        NatStatus {
            reachability,
            errors: self.errors,
            tried_addresses: self.addresses.into_iter().collect(),
            outbound_failures: self.outbound_failures,
            probe_id: self.id,
        }
    }
}

// Configuration for a single probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeConfig {
    /// List of trusted public peers that are probed when attempting to determine the auto-nat status.
    pub servers: Vec<PeerId>,
    /// Whether the list of servers should be extended with currently connected peers, up to `max_peers`.
    pub extend_with_connected: bool,
    /// Max peers to send a dial-request to.
    pub max_peers: usize,
    /// Minimum amount of DialResponse::Ok / ResponseError::DialFailure from different remote peers,
    /// for it to count as a valid result. If the minimum confidence was not reached, the reachability will be
    /// [`Reachability::Unknown`].
    pub min_confidence: usize,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        ProbeConfig {
            servers: Vec::new(),
            extend_with_connected: true,
            max_peers: 10,
            min_confidence: 3,
        }
    }
}

impl ProbeConfig {
    fn build(&self, id: ProbeId, addresses: Vec<Multiaddr>, connected: Vec<&PeerId>) -> Probe {
        let mut pending_servers = self.servers.clone();
        pending_servers.truncate(self.max_peers);
        if pending_servers.len() < self.max_peers && self.extend_with_connected {
            let mut connected: Vec<_> = connected.into_iter().copied().collect();
            while !connected.is_empty() && pending_servers.len() >= self.max_peers {
                let peer = connected.remove(rand::random::<usize>() % connected.len());
                if !pending_servers.contains(&peer) {
                    pending_servers.push(peer);
                }
            }
        }
        let addresses = addresses.into_iter().map(|a| (a, 0)).collect();
        let pending_servers: Vec<_> = pending_servers.into_iter().map(|p| (p, None)).collect();

        Probe {
            id,
            min_confidence: self.min_confidence,
            server_count: pending_servers.len(),
            pending_servers,
            addresses,
            errors: Vec::new(),
            outbound_failures: Vec::new(),
        }
    }
}

/// Outcome of a [`Probe`].
#[derive(Debug, Clone, PartialEq)]
pub struct NatStatus {
    /// Our assumed reachability derived from the dial responses we received
    pub reachability: Reachability,
    /// External addresses and the number ob successful dials to them reported by remote peers.
    ///
    /// Apart from our demanded addresses this may also contains addresses that the remote observed us at.
    pub tried_addresses: Vec<(Multiaddr, usize)>,
    /// Received dial-response errors.
    pub errors: Vec<(PeerId, ResponseError)>,
    /// Failed requests.
    pub outbound_failures: Vec<(PeerId, OutboundFailure)>,

    /// Id of the probe that resulted in this status.
    pub probe_id: ProbeId,
}

/// Network Behaviour for AutoNAT.
pub struct Behaviour {
    // Own peer id
    local_peer_id: PeerId,

    // Inner protocol for sending requests and receiving the response.
    inner: RequestResponse<AutoNatCodec>,

    // Ongoing inbound requests, where no response has been sent back to the remote yet.
    ongoing_inbound: HashMap<PeerId, (Vec<Multiaddr>, ResponseChannel<DialResponse>)>,

    // Ongoing outbound dial-requests, where no response has been received from the remote yet.
    ongoing_outbound: Option<Probe>,

    // Manually initiated probe.
    pending_probes: VecDeque<(ProbeId, ProbeConfig)>,

    // Connected peers with their observed address
    connected: HashMap<PeerId, Multiaddr>,

    // Assumed reachability derived from the most recent probe.
    reachability: Reachability,

    // See `Config::server`
    server_config: Option<ServerConfig>,

    // See `Config::auto_probe`
    auto_probe: Option<(AutoProbe, Delay)>,

    // Out events that should be reported to the user
    pending_out_event: VecDeque<NatStatus>,

    next_probe_id: ProbeId,
}

impl Behaviour {
    pub fn new(local_peer_id: PeerId, config: Config) -> Self {
        let proto_support = match config.server {
            Some(_) => ProtocolSupport::Full,
            None => ProtocolSupport::Outbound,
        };
        let protocols = iter::once((AutoNatProtocol, proto_support));
        let mut cfg = RequestResponseConfig::default();
        cfg.set_request_timeout(config.timeout);
        let inner = RequestResponse::new(AutoNatCodec, protocols, cfg);
        let auto_probe = config.auto_probe.map(|a| (a, Delay::new(Duration::ZERO)));
        Self {
            local_peer_id,
            inner,
            ongoing_inbound: HashMap::default(),
            ongoing_outbound: None,
            pending_probes: VecDeque::new(),
            connected: HashMap::default(),
            reachability: Reachability::Unknown,
            server_config: config.server,
            auto_probe,
            pending_out_event: VecDeque::new(),
            next_probe_id: ProbeId(1),
        }
    }

    // Manually retry determination of NAT status.
    pub fn retry_nat_status(&mut self, probe_config: ProbeConfig) -> ProbeId {
        let id = self.next_probe_id();
        self.pending_probes.push_back((id, probe_config));
        id
    }

    // Assumed reachability derived from the most recent probe.
    pub fn reachability(&self) -> Reachability {
        self.reachability.clone()
    }

    /// Add peer to the list of trusted public peers that are probed in the auto-retry nat status.
    ///
    /// Return false if auto-retry is `None`.
    pub fn add_server(&mut self, peer: PeerId, address: Option<Multiaddr>) -> bool {
        match self.auto_probe {
            Some((ref mut retry, _)) => {
                retry.config.servers.push(peer);
                if let Some(addr) = address {
                    self.inner.add_address(&peer, addr);
                }
                true
            }
            None => false,
        }
    }

    /// Remove a peer from the list of servers tried in the auto-retry.
    ///
    /// Return false if auto-retry is `None`.
    pub fn remove_server(&mut self, peer: &PeerId) -> bool {
        match self.auto_probe {
            Some((ref mut retry, _)) => {
                retry.config.servers.retain(|p| p != peer);
                true
            }
            None => false,
        }
    }

    fn next_probe_id(&mut self) -> ProbeId {
        let probe_id = self.next_probe_id;
        self.next_probe_id.0 += 1;
        probe_id
    }

    fn do_probe(&mut self, probe: Probe) {
        // Check if the probe can already be resolved, e.g because there are no external addresses.
        let mut probe = match probe.evaluate() {
            Ok(status) => return self.inject_status(status),
            Err(probe) => probe,
        };
        for (peer_id, id) in probe.pending_servers.iter_mut() {
            let request_id = self.inner.send_request(
                peer_id,
                DialRequest {
                    peer_id: self.local_peer_id,
                    addrs: probe.addresses.keys().cloned().collect(),
                },
            );
            let _ = id.insert(request_id);
        }
        let _ = self.ongoing_outbound.insert(probe);
    }

    fn inject_status(&mut self, status: NatStatus) {
        self.reachability = Reachability::from(&status);
        self.pending_out_event.push_back(status);
        if let Some((ref auto_probe, ref mut delay)) = self.auto_probe {
            delay.reset(auto_probe.interval);
        }
    }

    // Handle the inbound request and collect the valid addresses to be dialed.
    fn handle_request(
        &mut self,
        sender: PeerId,
        request: DialRequest,
    ) -> Result<Vec<Multiaddr>, DialResponse> {
        let config = self
            .server_config
            .as_ref()
            .expect("Server config is present.");

        // Validate that the peer to be dialed is the request's sender.
        if request.peer_id != sender {
            let response = DialResponse {
                response: Err(ResponseError::BadRequest),
                status_text: Some("peer id mismatch".to_string()),
            };
            return Err(response);
        }
        // Check that there is no ongoing dial to the remote.
        if self.ongoing_inbound.contains_key(&sender) {
            let response = DialResponse {
                response: Err(ResponseError::DialRefused),
                status_text: Some("too many dials".to_string()),
            };
            return Err(response);
        }
        // Check if max simultaneous autonat dial-requests are reached.
        if self.ongoing_inbound.len() >= config.max_ongoing {
            let response = DialResponse {
                response: Err(ResponseError::DialRefused),
                status_text: Some("too many dials".to_string()),
            };
            return Err(response);
        }

        let observed_addr = self
            .connected
            .get(&sender)
            .expect("We are connected to the peer.");
        // Filter valid addresses.
        let mut addrs = filter_valid_addrs(
            sender,
            request.addrs,
            observed_addr,
            self.server_config.as_ref().unwrap().only_public,
        );
        addrs.truncate(config.max_addresses);

        if addrs.is_empty() {
            let response = DialResponse {
                response: Err(ResponseError::DialError),
                status_text: Some("no dialable addresses".to_string()),
            };
            return Err(response);
        }

        Ok(addrs)
    }

    // Update the ongoing outbound probe according to the result of our dial-request.
    // If the minimum confidence was reached it returns the nat status.
    fn handle_response(
        &mut self,
        sender: PeerId,
        request_id: RequestId,
        response: Result<DialResponse, OutboundFailure>,
    ) -> Option<NatStatus> {
        let mut probe = self.ongoing_outbound.take()?;

        // Ignore the response if the peer or the request is not part of the ongoing probe.
        // This could be the case if we received a late response for a previous probe that already resolved.
        if !probe
            .pending_servers
            .iter()
            .any(|(p, id)| *p == sender && id.unwrap() == request_id)
        {
            let _ = self.ongoing_outbound.insert(probe);
            return None;
        };

        probe.pending_servers.retain(|(p, _)| p != &sender);
        match response {
            Ok(DialResponse {
                response: Ok(addr), ..
            }) => {
                let score = probe.addresses.entry(addr).or_insert(0);
                *score += 1;
            }
            Ok(DialResponse {
                response: Err(error),
                ..
            }) => {
                probe.errors.push((sender, error));
            }
            Err(err) => {
                probe.outbound_failures.push((sender, err));
            }
        }
        match probe.evaluate() {
            Ok(status) => Some(status),
            Err(probe) => {
                let _ = self.ongoing_outbound.insert(probe);
                None
            }
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = NatStatus;

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
                // Check if the dialed address was among the requested addresses.
                if addrs.contains(address) {
                    // Successfully dialed one of the addresses from the remote peer.
                    let channel = self.ongoing_inbound.remove(peer).unwrap().1;
                    let response = DialResponse {
                        response: Ok(address.clone()),
                        status_text: None,
                    };
                    let _ = self.inner.send_response(channel, response);
                }
            }
        }
        self.connected
            .insert(*peer, endpoint.get_remote_address().clone());
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
            let response = DialResponse {
                response: Err(ResponseError::DialError),
                status_text: Some("dial failed".to_string()),
            };
            let _ = self.inner.send_response(channel, response);
        }
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        self.inner.inject_disconnected(peer);
        self.connected.remove(peer);
    }

    fn inject_address_change(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        self.inner.inject_address_change(peer, conn, old, new);
        // Update observed address.
        self.connected
            .insert(*peer, new.get_remote_address().clone());
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_new_external_addr(addr);
        if self.reachability.is_public() || !self.pending_probes.is_empty() {
            return;
        }
        if let Some((ref auto_probe, _)) = self.auto_probe {
            let config = auto_probe.config.clone();
            let probe_id = self.next_probe_id();
            self.pending_probes.push_back((probe_id, config));
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        if let Some((ref auto_probe, ref mut delay)) = self.auto_probe {
            if delay.poll_unpin(cx).is_ready()
                && self.ongoing_outbound.is_none()
                && self.pending_probes.is_empty()
            {
                let config = auto_probe.config.clone();
                let id = self.next_probe_id();
                self.pending_probes.push_back((id, config));
            }
        };
        loop {
            if self.ongoing_outbound.is_none() {
                if let Some((probe_id, config)) = self.pending_probes.pop_front() {
                    let mut external_addrs: Vec<Multiaddr> = params
                        .external_addresses()
                        .map(|record| record.addr)
                        .collect();
                    if external_addrs.is_empty() {
                        external_addrs.extend(params.listened_addresses());
                    }
                    let probe =
                        config.build(probe_id, external_addrs, self.connected.keys().collect());
                    self.do_probe(probe);
                }
            }
            if let Some(event) = self.pending_out_event.pop_front() {
                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
            }
            match self.inner.poll(cx, params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::Message { peer, message },
                )) => match message {
                    RequestResponseMessage::Request {
                        request_id: _,
                        request,
                        channel,
                    } => match self.handle_request(peer, request) {
                        Ok(addrs) => {
                            self.ongoing_inbound.insert(peer, (addrs.clone(), channel));
                            return Poll::Ready(NetworkBehaviourAction::Dial {
                                opts: DialOpts::peer_id(peer)
                                    .condition(PeerCondition::Always)
                                    .addresses(addrs)
                                    .build(),
                                handler: self.inner.new_handler(),
                            });
                        }
                        Err(response) => {
                            let _ = self.inner.send_response(channel, response);
                        }
                    },
                    RequestResponseMessage::Response {
                        request_id,
                        response,
                    } => {
                        let mut report_addr = None;
                        if let Ok(ref addr) = response.response {
                            // Update observed address score if it is finite.
                            let score = params
                                .external_addresses()
                                .find_map(|r| (&r.addr == addr).then(|| r.score))
                                .unwrap_or(AddressScore::Finite(0));
                            if let AddressScore::Finite(finite_score) = score {
                                report_addr = Some(NetworkBehaviourAction::ReportObservedAddr {
                                    address: addr.clone(),
                                    score: AddressScore::Finite(finite_score + 1),
                                });
                            }
                        }
                        if let Some(status) = self.handle_response(peer, request_id, Ok(response)) {
                            self.inject_status(status);
                        }
                        if let Some(action) = report_addr {
                            return Poll::Ready(action);
                        }
                    }
                },
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::ResponseSent { .. },
                )) => {}
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::OutboundFailure {
                        request_id,
                        peer,
                        error,
                    },
                )) => {
                    if let Some(status) = self.handle_response(peer, request_id, Err(error)) {
                        self.inject_status(status);
                    }
                }
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::InboundFailure { peer, .. },
                )) => {
                    self.ongoing_inbound.remove(&peer);
                }
                Poll::Ready(action) => return Poll::Ready(action.map_out(|_| unreachable!())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.inner.addresses_of_peer(peer)
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        self.inner.inject_connected(peer)
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

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        conn: ConnectionId,
        event: RequestResponseHandlerEvent<AutoNatCodec>,
    ) {
        self.inner.inject_event(peer_id, conn, event)
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

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_expired_external_addr(addr);
    }
}

// Filter demanded dial addresses for validity, to prevent abuse.
fn filter_valid_addrs(
    peer: PeerId,
    demanded: Vec<Multiaddr>,
    observed_remote_at: &Multiaddr,
    only_public: bool,
) -> Vec<Multiaddr> {
    // Skip if the observed address is a relay address.
    if observed_remote_at.iter().any(|p| p == Protocol::P2pCircuit) {
        return Vec::new();
    }
    let observed_ip = observed_remote_at.into_iter().find(|p| match p {
        Protocol::Ip4(ip) => {
            if !only_public {
                return true;
            }
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
        .filter_map(|mut addr| {
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
            if !addr.iter().any(|p| matches!(p, Protocol::P2p(_))) {
                addr.push(Protocol::P2p(peer.into()))
            }
            // Only collect distinct addresses.
            distinct.insert(addr.clone()).then(|| addr)
        })
        .collect()
}
