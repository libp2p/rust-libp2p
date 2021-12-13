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
use instant::Instant;
use libp2p_core::{
    connection::{ConnectionId, ListenerId},
    multiaddr::Protocol,
    ConnectedPoint, Multiaddr, PeerId,
};
use libp2p_request_response::{
    handler::RequestResponseHandlerEvent, ProtocolSupport, RequestId, RequestResponse,
    RequestResponseConfig, RequestResponseEvent, RequestResponseMessage, ResponseChannel,
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

    // == Client Config
    /// Delay on init before starting the fist probe
    pub boot_delay: Duration,
    /// Interval in which the NAT should be tested again if reached max confidence.
    pub refresh_interval: Duration,
    /// Interval in which the NAT status should be re-tried if it is currently is unknown
    /// or max confidence was not reached yet.
    pub retry_interval: Duration,

    /// Throttle period for re-using a peer as server for a dial-request.
    pub throttle_peer_period: Duration,
    /// Wether connected peers may be used as server for a probe, in addition to the predefined ones.
    pub may_use_connected: bool,
    /// Max confidence that can be reached in a public / private NAT status.
    /// The confidence is increased each time a probe confirms the assumed status, and
    /// reduced each time a different status is reported. On confidence 0 the status
    /// is flipped if a different one is reported.
    /// Note: for [`NatStatus::Unknown`] the confidence is always 0.
    pub confidence_max: usize,

    //== Server Config
    /// Max addresses that are tried per peer.
    pub max_peer_addresses: usize,
    /// Max total simultaneous dial-attempts.
    pub throttle_global_max: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timeout: Duration::from_secs(30),
            boot_delay: Duration::from_secs(15),
            retry_interval: Duration::from_secs(90),
            refresh_interval: Duration::from_secs(15 * 60),
            throttle_peer_period: Duration::from_secs(90),
            may_use_connected: true,
            confidence_max: 3,
            max_peer_addresses: 16,
            throttle_global_max: 30,
        }
    }
}

/// Assumed NAT status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NatStatus {
    Public(Multiaddr),
    Private,
    Unknown,
}

impl NatStatus {
    pub fn is_public(&self) -> bool {
        matches!(self, NatStatus::Public(..))
    }
}

impl From<Result<Multiaddr, ResponseError>> for NatStatus {
    fn from(result: Result<Multiaddr, ResponseError>) -> Self {
        match result {
            Ok(addr) => NatStatus::Public(addr),
            Err(ResponseError::DialError) => NatStatus::Private,
            _ => NatStatus::Unknown,
        }
    }
}

/// Network Behaviour for AutoNAT.
pub struct Behaviour {
    // Local peer id
    local_peer_id: PeerId,

    // Inner behaviour for sending requests and receiving the response.
    inner: RequestResponse<AutoNatCodec>,

    config: Config,

    // Additional peers apart from the currently connected ones, that may be used for probes.
    servers: Vec<PeerId>,

    // Assumed NAT status.
    nat_status: NatStatus,

    // Confidence in the assumed NAT status.
    confidence: usize,

    // Timer for the next probe.
    schedule_probe: Delay,

    // Ongoing inbound requests, where no response has been sent back to the remote yet.
    ongoing_inbound: HashMap<PeerId, (Vec<Multiaddr>, ResponseChannel<DialResponse>)>,

    // Connected peers with their observed address.
    // These peers may be used as servers for dial-requests.
    connected: HashMap<PeerId, Multiaddr>,

    // Used servers in recent outbound probes that are throttled through Config::throttle_peer_period.
    throttled_servers: Vec<(PeerId, Instant)>,

    last_probe: Option<Instant>,

    pending_out_events: VecDeque<<Self as NetworkBehaviour>::OutEvent>,
}

impl Behaviour {
    pub fn new(local_peer_id: PeerId, config: Config) -> Self {
        let protocols = iter::once((AutoNatProtocol, ProtocolSupport::Full));
        let mut cfg = RequestResponseConfig::default();
        cfg.set_request_timeout(config.timeout);
        let inner = RequestResponse::new(AutoNatCodec, protocols, cfg);
        Self {
            local_peer_id,
            inner,
            schedule_probe: Delay::new(config.boot_delay),
            config,
            servers: Vec::new(),
            ongoing_inbound: HashMap::default(),
            connected: HashMap::default(),
            nat_status: NatStatus::Unknown,
            confidence: 0,
            throttled_servers: Vec::new(),
            last_probe: None,
            pending_out_events: VecDeque::new(),
        }
    }

    /// Address if we are public.
    pub fn public_address(&self) -> Option<&Multiaddr> {
        match &self.nat_status {
            NatStatus::Public(address) => Some(address),
            _ => None,
        }
    }

    /// Currently assumed NAT status.
    pub fn nat_status(&self) -> NatStatus {
        self.nat_status.clone()
    }

    /// Confidence in the assumed NAT status.
    pub fn confidence(&self) -> usize {
        self.confidence
    }

    /// Add a peer to the list over servers that may be used for probes.
    /// While probes normally use one of the connected peers as server, this allows to add trusted
    /// peers that can be used even if they are currently not connected, in which case a connection will be
    /// establish before sending the dial-request.
    pub fn add_server(&mut self, peer: PeerId, address: Option<Multiaddr>) {
        self.servers.push(peer);
        if let Some(addr) = address {
            self.inner.add_address(&peer, addr);
        }
    }

    /// Remove a peer from the list of servers.
    /// See [`Behaviour::add_server`] for more info.
    pub fn remove_server(&mut self, peer: &PeerId) {
        self.servers.retain(|p| p != peer);
    }

    // Select a random server for the probe.
    fn random_server(&mut self) -> Option<PeerId> {
        // Update list of throttled servers.
        let i = self
            .throttled_servers
            .partition_point(|(_, time)| *time + self.config.throttle_peer_period < Instant::now());
        self.throttled_servers.drain(..i);

        let mut servers: Vec<&PeerId> = self.servers.iter().collect();

        if self.config.may_use_connected {
            servers.extend(self.connected.iter().map(|(id, _)| id));
        }

        servers.retain(|s| !self.throttled_servers.iter().any(|(id, _)| s == &id));

        if servers.is_empty() {
            return None;
        }
        let server = servers[rand::random::<usize>() % servers.len()];
        Some(*server)
    }

    // Send a dial request to the set server or a randomly selected one.
    // Return `None` if there are no qualified servers or no dial-back addresses.
    fn do_probe(&mut self, addresses: Vec<Multiaddr>) -> Option<RequestId> {
        self.last_probe = Some(Instant::now());
        if addresses.is_empty() {
            log::debug!("Outbound dial-back request aborted: No dial-back addresses.");
            return None;
        }
        let server = match self.random_server() {
            Some(s) => s,
            None => {
                log::debug!("Outbound dial-back request aborted: No qualified server.");
                return None;
            }
        };
        let request_id = self.inner.send_request(
            &server,
            DialRequest {
                peer_id: self.local_peer_id,
                addresses,
            },
        );
        self.throttled_servers.push((server, Instant::now()));
        log::debug!("Send dial-back request to peer {}.", server);
        Some(request_id)
    }

    // Validate the inbound request and collect the addresses to be dialed.
    fn resolve_inbound_request(
        &mut self,
        sender: PeerId,
        request: DialRequest,
    ) -> Result<Vec<Multiaddr>, DialResponse> {
        if request.peer_id != sender {
            let status_text = "peer id mismatch".to_string();
            log::debug!(
                "Reject inbound dial request from peer {}: {}.",
                sender,
                status_text
            );
            let response = DialResponse {
                result: Err(ResponseError::BadRequest),
                status_text: Some(status_text),
            };
            return Err(response);
        }

        if self.ongoing_inbound.contains_key(&sender) {
            let status_text = "too many dials".to_string();
            log::debug!(
                "Reject inbound dial request from peer {}: {}.",
                sender,
                status_text
            );
            let response = DialResponse {
                result: Err(ResponseError::DialRefused),
                status_text: Some(status_text),
            };
            return Err(response);
        }

        if self.ongoing_inbound.len() >= self.config.throttle_global_max {
            let status_text = "too many total dials".to_string();
            log::debug!(
                "Reject inbound dial request from peer {}: {}.",
                sender,
                status_text
            );
            let response = DialResponse {
                result: Err(ResponseError::DialRefused),
                status_text: Some(status_text),
            };
            return Err(response);
        }

        let observed_addr = self
            .connected
            .get(&sender)
            .expect("We are connected to the peer.");

        let mut addrs = filter_valid_addrs(sender, request.addresses, observed_addr);
        addrs.truncate(self.config.max_peer_addresses);

        if addrs.is_empty() {
            let status_text = "no dialable addresses".to_string();
            log::debug!(
                "Reject inbound dial request from peer {}: {}.",
                sender,
                status_text
            );
            let response = DialResponse {
                result: Err(ResponseError::DialError),
                status_text: Some(status_text),
            };
            return Err(response);
        }

        Ok(addrs)
    }

    fn schedule_next_probe(&mut self, delay: Duration) {
        let last_probe_instant = match self.last_probe {
            Some(instant) => instant,
            None => {
                return;
            }
        };
        let schedule_next = last_probe_instant + delay;
        self.schedule_probe
            .reset(schedule_next.saturating_duration_since(Instant::now()));
    }

    // Adapt current confidence and NAT status to the status reported by the latest probe.
    // Return whether the currently assumed status was flipped.
    fn handle_reported_status(&mut self, reported: NatStatus) -> bool {
        self.schedule_next_probe(self.config.retry_interval);

        if reported == self.nat_status {
            if !matches!(self.nat_status, NatStatus::Unknown) {
                if self.confidence < self.config.confidence_max {
                    self.confidence += 1;
                }
                // Delay with (usually longer) refresh-interval.
                if self.confidence >= self.config.confidence_max {
                    self.schedule_next_probe(self.config.refresh_interval);
                }
            }
            false
        } else if self.confidence > 0 {
            // Reduce confidence but keep old status.
            self.confidence -= 1;
            false
        } else {
            log::debug!(
                "Flipped assumed NAT status from {:?} to {:?}",
                self.nat_status,
                reported
            );
            self.nat_status = reported;
            true
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
        self.connected
            .insert(*peer, endpoint.get_remote_address().clone());

        match endpoint {
            ConnectedPoint::Dialer { address } => {
                if let Some((addrs, _)) = self.ongoing_inbound.get(peer) {
                    // Check if the dialed address was among the requested addresses.
                    if addrs.contains(address) {
                        log::debug!(
                            "Dial-back to peer {} succeeded at addr {:?}.",
                            peer,
                            address
                        );

                        let channel = self.ongoing_inbound.remove(peer).unwrap().1;
                        let response = DialResponse {
                            result: Ok(address.clone()),
                            status_text: None,
                        };
                        let _ = self.inner.send_response(channel, response);
                    }
                }
            }
            // An inbound connection can indicate that we are public; adjust the delay to the next probe.
            ConnectedPoint::Listener { .. } => {
                if self.confidence == self.config.confidence_max {
                    if self.nat_status.is_public() {
                        self.schedule_next_probe(self.config.refresh_interval * 2);
                    } else {
                        self.schedule_next_probe(self.config.refresh_interval / 5);
                    }
                }
            }
        }
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ProtocolsHandler,
        error: &DialError,
    ) {
        self.inner.inject_dial_failure(peer_id, handler, error);
        if let Some((_, channel)) = peer_id.and_then(|p| self.ongoing_inbound.remove(&p)) {
            log::debug!(
                "Dial-back to peer {} failed with error {:?}.",
                peer_id.unwrap(),
                error
            );

            let response = DialResponse {
                result: Err(ResponseError::DialError),
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

        self.connected
            .insert(*peer, new.get_remote_address().clone());
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_new_listen_addr(id, addr);

        // New address could be publicly reachable, trigger retry.
        if !self.nat_status.is_public() {
            if self.confidence > 0 {
                self.confidence -= 1;
            }
            self.schedule_next_probe(self.config.retry_interval);
        }
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_expired_listen_addr(id, addr);
        if let Some(public_address) = self.public_address() {
            if public_address == addr {
                self.confidence = 0;
                self.nat_status = NatStatus::Unknown;
                self.schedule_probe.reset(Duration::ZERO);
            }
        }
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_new_external_addr(addr);

        // New address could be publicly reachable, trigger retry.
        if !self.nat_status.is_public() {
            if self.confidence > 0 {
                self.confidence -= 1;
            }
            self.schedule_next_probe(self.config.retry_interval);
        }
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_expired_external_addr(addr);
        if let Some(public_address) = self.public_address() {
            if public_address == addr {
                self.confidence = 0;
                self.nat_status = NatStatus::Unknown;
                self.schedule_probe.reset(Duration::ZERO);
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        let mut is_probe_ready = false;
        loop {
            if self.schedule_probe.poll_unpin(cx).is_ready() {
                is_probe_ready = true;
                self.schedule_probe.reset(self.config.retry_interval);
                continue;
            }
            if is_probe_ready {
                let mut addresses = match self.public_address() {
                    Some(a) => vec![a.clone()], // Remote should try our assumed public address first.
                    None => Vec::new(),
                };
                addresses.extend(params.external_addresses().map(|r| r.addr));
                addresses.extend(params.listened_addresses());

                match self.do_probe(addresses) {
                    Some(_) => {}
                    None => {
                        let nat_status = NatStatus::Unknown;
                        let has_flipped = self.handle_reported_status(nat_status);
                        if has_flipped {
                            self.pending_out_events.push_back(self.nat_status.clone());
                        }
                    }
                }
            }
            if let Some(event) = self.pending_out_events.pop_front() {
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
                    } => {
                        match self.resolve_inbound_request(peer, request) {
                            Ok(addrs) => {
                                log::debug!("Inbound dial request from Peer {} with dial-back addresses {:?}.", peer, addrs);
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
                        }
                    }
                    RequestResponseMessage::Response { response, .. } => {
                        log::debug!("Outbound dial-back request returned {:?}.", response);
                        let mut report_addr = None;
                        if let Ok(ref addr) = response.result {
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
                        let reported = response.result.into();
                        let has_flipped = self.handle_reported_status(reported);
                        if has_flipped {
                            self.pending_out_events.push_back(self.nat_status.clone());
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
                    RequestResponseEvent::OutboundFailure { error, peer, .. },
                )) => {
                    log::debug!(
                        "Outbound Failure {} when sending dial-back request to server {}.",
                        error,
                        peer
                    );
                    is_probe_ready = true;
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

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.inner.inject_listener_error(id, err)
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        self.inner.inject_listener_closed(id, reason)
    }
}

// Filter dial addresses and replace demanded ip with the observed one.
fn filter_valid_addrs(
    peer: PeerId,
    demanded: Vec<Multiaddr>,
    observed_remote_at: &Multiaddr,
) -> Vec<Multiaddr> {
    // Skip if the observed address is a relay address.
    if observed_remote_at.iter().any(|p| p == Protocol::P2pCircuit) {
        return Vec::new();
    }
    let observed_ip = match observed_remote_at
        .into_iter()
        .find(|p| matches!(p, Protocol::Ip4(_) | Protocol::Ip6(_)))
    {
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
            let mut addr = addr.replace(i, |_| Some(observed_ip.clone()))?;

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

#[cfg(test)]
mod test {
    use super::*;

    use std::net::Ipv4Addr;

    fn random_ip<'a>() -> Protocol<'a> {
        Protocol::Ip4(Ipv4Addr::new(
            rand::random(),
            rand::random(),
            rand::random(),
            rand::random(),
        ))
    }
    fn random_port<'a>() -> Protocol<'a> {
        Protocol::Tcp(rand::random())
    }

    #[test]
    fn filter_addresses() {
        let peer_id = PeerId::random();
        let observed_ip = random_ip();
        let observed_addr = Multiaddr::empty()
            .with(observed_ip.clone())
            .with(random_port())
            .with(Protocol::P2p(peer_id.into()));
        // Valid address with matching peer-id
        let demanded_1 = Multiaddr::empty()
            .with(random_ip())
            .with(random_port())
            .with(Protocol::P2p(peer_id.into()));
        // Invalid because peer_id does not match
        let demanded_2 = Multiaddr::empty()
            .with(random_ip())
            .with(random_port())
            .with(Protocol::P2p(PeerId::random().into()));
        // Valid address without peer-id
        let demanded_3 = Multiaddr::empty().with(random_ip()).with(random_port());
        // Invalid because relayed
        let demanded_4 = Multiaddr::empty()
            .with(random_ip())
            .with(random_port())
            .with(Protocol::P2p(PeerId::random().into()))
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(peer_id.into()));
        let demanded = vec![
            demanded_1.clone(),
            demanded_2,
            demanded_3.clone(),
            demanded_4,
        ];
        let filtered = filter_valid_addrs(peer_id, demanded, &observed_addr);
        let expected_1 = demanded_1
            .replace(0, |_| Some(observed_ip.clone()))
            .unwrap();
        let expected_2 = demanded_3
            .replace(0, |_| Some(observed_ip))
            .unwrap()
            .with(Protocol::P2p(peer_id.into()));
        assert_eq!(filtered, vec![expected_1, expected_2]);
    }

    #[test]
    fn skip_relayed_addr() {
        let peer_id = PeerId::random();
        let observed_ip = random_ip();
        // Observed address is relayed.
        let observed_addr = Multiaddr::empty()
            .with(observed_ip.clone())
            .with(random_port())
            .with(Protocol::P2p(PeerId::random().into()))
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(peer_id.into()));
        let demanded = Multiaddr::empty()
            .with(random_ip())
            .with(random_port())
            .with(Protocol::P2p(peer_id.into()));
        let filtered = filter_valid_addrs(peer_id, vec![demanded], &observed_addr);
        assert!(filtered.is_empty());
    }
}
