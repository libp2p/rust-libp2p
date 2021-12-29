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
    handler::RequestResponseHandlerEvent, InboundFailure, OutboundFailure, ProtocolSupport,
    RequestId, RequestResponse, RequestResponseConfig, RequestResponseEvent,
    RequestResponseMessage, ResponseChannel,
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
    /// Delay on init before starting the fist probe.
    pub boot_delay: Duration,
    /// Interval in which the NAT should be tested again if max confidence was reached in a status.
    pub refresh_interval: Duration,
    /// Interval in which the NAT status should be re-tried if it is currently unknown
    /// or max confidence was not reached yet.
    pub retry_interval: Duration,

    /// Throttle period for re-using a peer as server for a dial-request.
    pub throttle_server_period: Duration,
    /// Use connected peers as servers for probes.
    pub use_connected: bool,
    /// Max confidence that can be reached in a public / private NAT status.
    /// Note: for [`NatStatus::Unknown`] the confidence is always 0.
    pub confidence_max: usize,

    //== Server Config
    /// Max addresses that are tried per peer.
    pub max_peer_addresses: usize,
    /// Max total dial requests done in `[Config::throttle_clients_period`].
    pub throttle_clients_global_max: usize,
    /// Max dial requests done in `[Config::throttle_clients_period`] for a peer.
    pub throttle_clients_peer_max: usize,
    /// Period for throttling clients requests.
    pub throttle_clients_period: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timeout: Duration::from_secs(30),
            boot_delay: Duration::from_secs(15),
            retry_interval: Duration::from_secs(90),
            refresh_interval: Duration::from_secs(15 * 60),
            throttle_server_period: Duration::from_secs(90),
            use_connected: true,
            confidence_max: 3,
            max_peer_addresses: 16,
            throttle_clients_global_max: 30,
            throttle_clients_peer_max: 3,
            throttle_clients_period: Duration::from_secs(1),
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

/// Unique identifier for a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeId(usize);

impl ProbeId {
    fn next(&mut self) -> ProbeId {
        let current = *self;
        self.0 += 1;
        current
    }
}

/// Outbound probe failed or was aborted.
#[derive(Debug, Clone, PartialEq)]
pub enum OutboundProbeError {
    /// Probe was aborted because no server is known, or all servers
    /// are throttled through [`Config::throttle_server_period`].
    NoServer,
    ///  Probe was aborted because the local peer has no listening or
    /// external addresses.
    NoAddresses,
    /// Sending the dial-back request or receiving a response failed.
    OutboundRequest(OutboundFailure),
    /// The server refused or failed to dial us.
    Response(ResponseError),
}

/// Inbound probe failed.
#[derive(Debug, Clone, PartialEq)]
pub enum InboundProbeError {
    /// Receiving the dial-back request or sending a response failed.
    InboundRequest(InboundFailure),
    /// We refused or failed to dial the client.
    Response(ResponseError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InboundProbeEvent {
    /// A dial-back request was received from a remote peer.
    Request {
        probe_id: ProbeId,
        /// Peer that sent the request.
        peer: PeerId,
        /// The addresses that will be attempted to dial.
        addresses: Vec<Multiaddr>,
    },
    /// A dial request to the remote was successful.
    Response {
        probe_id: ProbeId,
        /// Peer to which the response is sent.
        peer: PeerId,
        address: Multiaddr,
    },
    /// The inbound request failed, was rejected, or none of the remote's
    /// addresses could be dialed.
    Error {
        probe_id: ProbeId,
        /// Peer that sent the dial-back request.
        peer: PeerId,
        error: InboundProbeError,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutboundProbeEvent {
    /// A dial-back request was sent to a remote peer.
    Request {
        probe_id: ProbeId,
        /// Peer to which the request is sent.
        peer: PeerId,
    },
    /// The remote successfully dialed one of our addresses.
    Response {
        probe_id: ProbeId,
        /// Id of the peer that sent the response.
        peer: PeerId,
        /// The address at which the remote succeeded to dial us.
        address: Multiaddr,
    },
    /// The outbound request failed, was rejected, or the remote could dial
    /// none of our addresses.
    Error {
        probe_id: ProbeId,
        /// Id of the peer used for the probe.
        /// `None` if the probe was aborted due to no addresses or no qualified server.
        peer: Option<PeerId>,
        error: OutboundProbeError,
    },
}

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Event on an inbound probe.
    InboundProbe(InboundProbeEvent),
    /// Event on an outbound probe.
    OutboundProbe(OutboundProbeEvent),
    /// The assumed NAT changed.
    StatusChanged {
        /// Former status.
        old: NatStatus,
        /// New status.
        new: NatStatus,
    },
}

/// [`NetworkBehaviour`] for AutoNAT.
///
/// The behaviour frequently runs probes to determine whether the local peer is behind NAT and/ or a firewall, or
/// publicly reachable.
/// In a probe, a dial-back request is sent to a peer that is randomly selected from the list of fixed servers and
/// connected peers. Upon receiving a dial-back request, the remote tries to dial the included addresses. When a
/// first address was successfully dialed, a status Ok will be send back together with the dialed address. If no address
/// can be reached a dial-error is send back.
/// Based on the received response, the sender assumes themselves to be public or private.
/// The status is retried in a frequency of [`Config::retry_interval`] or [`Config::retry_interval`], depending on whether
/// enough confidence in the assumed NAT status was reached or not.
/// The confidence increases each time a probe confirms the assumed status, and decreases if a different status is reported.
/// If the confidence is 0, the status is flipped and the Behaviour will report the new status in an `OutEvent`.
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
    ongoing_inbound: HashMap<PeerId, (ProbeId, Vec<Multiaddr>, ResponseChannel<DialResponse>)>,

    // Ongoing outbound probes and mapped to the inner request id.
    ongoing_outbound: HashMap<RequestId, ProbeId>,

    // Connected peers with their observed address.
    // These peers may be used as servers for dial-requests.
    connected: HashMap<PeerId, Multiaddr>,

    // Used servers in recent outbound probes that are throttled through Config::throttle_server_period.
    throttled_servers: Vec<(PeerId, Instant)>,

    // Recent probes done for clients
    throttled_clients: Vec<(PeerId, Instant)>,

    last_probe: Option<Instant>,

    pending_out_events: VecDeque<<Self as NetworkBehaviour>::OutEvent>,

    probe_id: ProbeId,
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
            ongoing_outbound: HashMap::default(),
            connected: HashMap::default(),
            nat_status: NatStatus::Unknown,
            confidence: 0,
            throttled_servers: Vec::new(),
            throttled_clients: Vec::new(),
            last_probe: None,
            pending_out_events: VecDeque::new(),
            probe_id: ProbeId(0),
        }
    }

    /// Assumed public address of the local peer.
    /// Returns `None` in case of status [`NatStatus::Private`] or [`NatStatus::Unknown`].
    pub fn public_address(&self) -> Option<&Multiaddr> {
        match &self.nat_status {
            NatStatus::Public(address) => Some(address),
            _ => None,
        }
    }

    /// Assumed NAT status.
    pub fn nat_status(&self) -> NatStatus {
        self.nat_status.clone()
    }

    /// Confidence in the assumed NAT status.
    pub fn confidence(&self) -> usize {
        self.confidence
    }

    /// Add a peer to the list over servers that may be used for probes.
    /// These peers are used for dial-request even if they are currently not connection, in which case a connection will be
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
        let i = self.throttled_servers.partition_point(|(_, time)| {
            *time + self.config.throttle_server_period < Instant::now()
        });
        self.throttled_servers.drain(..i);

        let mut servers: Vec<&PeerId> = self.servers.iter().collect();

        if self.config.use_connected {
            servers.extend(self.connected.iter().map(|(id, _)| id));
        }

        servers.retain(|s| !self.throttled_servers.iter().any(|(id, _)| s == &id));

        if servers.is_empty() {
            return None;
        }
        let server = servers[rand::random::<usize>() % servers.len()];
        Some(*server)
    }

    // Send a dial-request to a randomly selected server.
    // Returns the server that is used in this probe.
    // `Err` if there are no qualified servers or no dial-back addresses.
    fn do_probe(
        &mut self,
        probe_id: ProbeId,
        addresses: Vec<Multiaddr>,
    ) -> Result<PeerId, OutboundProbeError> {
        self.last_probe = Some(Instant::now());
        if addresses.is_empty() {
            log::debug!("Outbound dial-back request aborted: No dial-back addresses.");
            return Err(OutboundProbeError::NoAddresses);
        }
        let server = match self.random_server() {
            Some(s) => s,
            None => {
                log::debug!("Outbound dial-back request aborted: No qualified server.");
                return Err(OutboundProbeError::NoServer);
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
        self.ongoing_outbound.insert(request_id, probe_id);
        Ok(server)
    }

    // Validate the inbound request and collect the addresses to be dialed.
    fn resolve_inbound_request(
        &mut self,
        sender: PeerId,
        request: DialRequest,
    ) -> Result<Vec<Multiaddr>, (String, ResponseError)> {
        // Update list of throttled clients.
        let i = self.throttled_clients.partition_point(|(_, time)| {
            *time + self.config.throttle_clients_period < Instant::now()
        });
        self.throttled_clients.drain(..i);

        if request.peer_id != sender {
            let status_text = "peer id mismatch".to_string();
            return Err((status_text, ResponseError::BadRequest));
        }

        if self.ongoing_inbound.contains_key(&sender) {
            let status_text = "dial-back already ongoing".to_string();
            return Err((status_text, ResponseError::DialRefused));
        }

        if self.throttled_clients.len() >= self.config.throttle_clients_global_max {
            let status_text = "too many total dials".to_string();
            return Err((status_text, ResponseError::DialRefused));
        }

        let ongoing_for_client = self
            .throttled_clients
            .iter()
            .filter(|(p, _)| p == &sender)
            .count();

        if ongoing_for_client >= self.config.throttle_clients_peer_max {
            let status_text = "too many dials for peer".to_string();
            return Err((status_text, ResponseError::DialRefused));
        }

        let observed_addr = self
            .connected
            .get(&sender)
            .expect("We are connected to the peer.");

        let mut addrs = filter_valid_addrs(sender, request.addresses, observed_addr);
        addrs.truncate(self.config.max_peer_addresses);

        if addrs.is_empty() {
            let status_text = "no dialable addresses".to_string();
            return Err((status_text, ResponseError::DialError));
        }

        Ok(addrs)
    }

    // Set the delay to the next probe based on the time of our last probe
    // and the specified delay.
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
    fn handle_reported_status(
        &mut self,
        probe_id: ProbeId,
        peer: Option<PeerId>,
        result: Result<Multiaddr, OutboundProbeError>,
    ) {
        self.schedule_next_probe(self.config.retry_interval);

        let reported_status = match result {
            Ok(ref addr) => NatStatus::Public(addr.clone()),
            Err(OutboundProbeError::Response(ResponseError::DialError)) => NatStatus::Private,
            _ => NatStatus::Unknown,
        };
        let event = match result {
            Ok(address) => OutboundProbeEvent::Response {
                probe_id,
                peer: peer.unwrap(),
                address,
            },
            Err(error) => OutboundProbeEvent::Error {
                probe_id,
                peer,
                error,
            },
        };
        self.pending_out_events
            .push_back(Event::OutboundProbe(event));

        if matches!(reported_status, NatStatus::Unknown) {
        } else if reported_status == self.nat_status {
            if self.confidence < self.config.confidence_max {
                self.confidence += 1;
            }
            // Delay with (usually longer) refresh-interval.
            if self.confidence >= self.config.confidence_max {
                self.schedule_next_probe(self.config.refresh_interval);
            }
        } else if reported_status.is_public() && self.nat_status.is_public() {
            // Different address than the currently assumed public address was reported.
            // Switch address, but don't report as flipped.
            self.nat_status = reported_status;
        } else if self.confidence > 0 {
            // Reduce confidence but keep old status.
            self.confidence -= 1;
        } else {
            log::debug!(
                "Flipped assumed NAT status from {:?} to {:?}",
                self.nat_status,
                reported_status
            );
            let old_status = self.nat_status.clone();
            self.nat_status = reported_status;
            let event = Event::StatusChanged {
                old: old_status,
                new: self.nat_status.clone(),
            };
            self.pending_out_events.push_back(event);
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = Event;

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
                if let Some((_, addrs, _)) = self.ongoing_inbound.get(peer) {
                    // Check if the dialed address was among the requested addresses.
                    if addrs.contains(address) {
                        log::debug!(
                            "Dial-back to peer {} succeeded at addr {:?}.",
                            peer,
                            address
                        );

                        let (probe_id, _, channel) = self.ongoing_inbound.remove(peer).unwrap();
                        let response = DialResponse {
                            result: Ok(address.clone()),
                            status_text: None,
                        };
                        let _ = self.inner.send_response(channel, response);
                        let event = InboundProbeEvent::Response {
                            probe_id,
                            peer: *peer,
                            address: address.clone(),
                        };
                        self.pending_out_events
                            .push_back(Event::InboundProbe(event));
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
        peer: Option<PeerId>,
        handler: Self::ProtocolsHandler,
        error: &DialError,
    ) {
        self.inner.inject_dial_failure(peer, handler, error);
        if let Some((probe_id, _, channel)) = peer.and_then(|p| self.ongoing_inbound.remove(&p)) {
            log::debug!(
                "Dial-back to peer {} failed with error {:?}.",
                peer.unwrap(),
                error
            );
            let response_error = ResponseError::DialError;
            let response = DialResponse {
                result: Err(response_error.clone()),
                status_text: Some("dial failed".to_string()),
            };
            let _ = self.inner.send_response(channel, response);

            let event = InboundProbeEvent::Error {
                probe_id,
                peer: peer.expect("PeerId is present."),
                error: InboundProbeError::Response(response_error),
            };
            self.pending_out_events
                .push_back(Event::InboundProbe(event));
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
                let mut addresses: Vec<_> = params.external_addresses().map(|r| r.addr).collect();
                addresses.extend(params.listened_addresses());

                let probe_id = self.probe_id.next();
                match self.do_probe(probe_id, addresses) {
                    Ok(peer) => {
                        let event = OutboundProbeEvent::Request { probe_id, peer };
                        self.pending_out_events
                            .push_back(Event::OutboundProbe(event));
                    }
                    Err(e) => {
                        self.handle_reported_status(probe_id, None, Err(e));
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
                        let probe_id = self.probe_id.next();
                        match self.resolve_inbound_request(peer, request) {
                            Ok(addrs) => {
                                log::debug!("Inbound dial request from Peer {} with dial-back addresses {:?}.", peer, addrs);
                                self.ongoing_inbound
                                    .insert(peer, (probe_id, addrs.clone(), channel));

                                let event = InboundProbeEvent::Request {
                                    probe_id,
                                    peer,
                                    addresses: addrs.clone(),
                                };
                                self.pending_out_events
                                    .push_back(Event::InboundProbe(event));

                                return Poll::Ready(NetworkBehaviourAction::Dial {
                                    opts: DialOpts::peer_id(peer)
                                        .condition(PeerCondition::Always)
                                        .addresses(addrs)
                                        .build(),
                                    handler: self.inner.new_handler(),
                                });
                            }
                            Err((status_text, error)) => {
                                log::debug!(
                                    "Reject inbound dial request from peer {}: {}.",
                                    peer,
                                    status_text
                                );

                                let response = DialResponse {
                                    result: Err(error.clone()),
                                    status_text: Some(status_text),
                                };
                                let _ = self.inner.send_response(channel, response.clone());

                                let event = InboundProbeEvent::Error {
                                    probe_id,
                                    peer,
                                    error: InboundProbeError::Response(error),
                                };
                                self.pending_out_events
                                    .push_back(Event::InboundProbe(event));
                            }
                        }
                    }
                    RequestResponseMessage::Response {
                        response,
                        request_id,
                    } => {
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
                        let probe_id = self
                            .ongoing_outbound
                            .remove(&request_id)
                            .expect("RequestId exists.");
                        let result = response.result.map_err(OutboundProbeError::Response);
                        self.handle_reported_status(probe_id, Some(peer), result);
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
