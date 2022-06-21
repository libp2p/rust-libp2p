// Copyright 2021 Protocol Labs.
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

mod as_client;
mod as_server;

use crate::protocol::{AutoNatCodec, AutoNatProtocol, DialRequest, DialResponse, ResponseError};
use as_client::AsClient;
pub use as_client::{OutboundProbeError, OutboundProbeEvent};
use as_server::AsServer;
pub use as_server::{InboundProbeError, InboundProbeEvent};
use futures_timer::Delay;
use instant::Instant;
use libp2p_core::{
    connection::{ConnectionId, ListenerId},
    multiaddr::Protocol,
    ConnectedPoint, Endpoint, Multiaddr, PeerId,
};
use libp2p_request_response::{
    handler::RequestResponseHandlerEvent, ProtocolSupport, RequestId, RequestResponse,
    RequestResponseConfig, RequestResponseEvent, RequestResponseMessage, ResponseChannel,
};
use libp2p_swarm::{
    DialError, IntoConnectionHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
};
use std::{
    collections::{HashMap, VecDeque},
    iter,
    task::{Context, Poll},
    time::Duration,
};

/// Config for the [`Behaviour`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Timeout for requests.
    pub timeout: Duration,

    // Client Config
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

    // Server Config
    /// Max addresses that are tried per peer.
    pub max_peer_addresses: usize,
    /// Max total dial requests done in `[Config::throttle_clients_period`].
    pub throttle_clients_global_max: usize,
    /// Max dial requests done in `[Config::throttle_clients_period`] for a peer.
    pub throttle_clients_peer_max: usize,
    /// Period for throttling clients requests.
    pub throttle_clients_period: Duration,
    /// As a server reject probes for clients that are observed at a non-global ip address.
    /// Correspondingly as a client only pick peers as server that are not observed at a
    /// private ip address. Note that this does not apply for servers that are added via
    /// [`Behaviour::add_server`].
    pub only_global_ips: bool,
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
            only_global_ips: true,
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
    ongoing_inbound: HashMap<
        PeerId,
        (
            ProbeId,
            RequestId,
            Vec<Multiaddr>,
            ResponseChannel<DialResponse>,
        ),
    >,

    // Ongoing outbound probes and mapped to the inner request id.
    ongoing_outbound: HashMap<RequestId, ProbeId>,

    // Connected peers with the observed address of each connection.
    // If the endpoint of a connection is relayed or not global (in case of Config::only_global_ips),
    // the observed address is `None`.
    connected: HashMap<PeerId, HashMap<ConnectionId, Option<Multiaddr>>>,

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

    fn as_client(&mut self) -> AsClient {
        AsClient {
            inner: &mut self.inner,
            local_peer_id: self.local_peer_id,
            config: &self.config,
            connected: &self.connected,
            probe_id: &mut self.probe_id,
            servers: &self.servers,
            throttled_servers: &mut self.throttled_servers,
            nat_status: &mut self.nat_status,
            confidence: &mut self.confidence,
            ongoing_outbound: &mut self.ongoing_outbound,
            last_probe: &mut self.last_probe,
            schedule_probe: &mut self.schedule_probe,
        }
    }

    fn as_server(&mut self) -> AsServer {
        AsServer {
            inner: &mut self.inner,
            config: &self.config,
            connected: &self.connected,
            probe_id: &mut self.probe_id,
            throttled_clients: &mut self.throttled_clients,
            ongoing_inbound: &mut self.ongoing_inbound,
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ConnectionHandler;
    type OutEvent = Event;

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
        failed_addresses: Option<&Vec<Multiaddr>>,
        other_established: usize,
    ) {
        self.inner.inject_connection_established(
            peer,
            conn,
            endpoint,
            failed_addresses,
            other_established,
        );
        let connections = self.connected.entry(*peer).or_default();
        let addr = endpoint.get_remote_address();
        let observed_addr =
            if !endpoint.is_relayed() && (!self.config.only_global_ips || addr.is_global_ip()) {
                Some(addr.clone())
            } else {
                None
            };
        connections.insert(*conn, observed_addr);

        match endpoint {
            ConnectedPoint::Dialer {
                address,
                role_override: Endpoint::Dialer,
            } => {
                if let Some(event) = self.as_server().on_outbound_connection(peer, address) {
                    self.pending_out_events
                        .push_back(Event::InboundProbe(event));
                }
            }
            ConnectedPoint::Dialer {
                address: _,
                role_override: Endpoint::Listener,
            } => {
                // Outgoing connection was dialed as a listener. In other words outgoing connection
                // was dialed as part of a hole punch. `libp2p-autonat` never attempts to hole
                // punch, thus this connection has not been requested by this [`NetworkBehaviour`].
            }
            ConnectedPoint::Listener { .. } => self.as_client().on_inbound_connection(),
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
        handler: <Self::ConnectionHandler as IntoConnectionHandler>::Handler,
        remaining_established: usize,
    ) {
        self.inner
            .inject_connection_closed(peer, conn, endpoint, handler, remaining_established);
        if remaining_established == 0 {
            self.connected.remove(peer);
        } else {
            let connections = self.connected.get_mut(peer).expect("Peer is connected.");
            connections.remove(conn);
        }
    }

    fn inject_dial_failure(
        &mut self,
        peer: Option<PeerId>,
        handler: Self::ConnectionHandler,
        error: &DialError,
    ) {
        self.inner.inject_dial_failure(peer, handler, error);
        if let Some(event) = self.as_server().on_outbound_dial_error(peer, error) {
            self.pending_out_events
                .push_back(Event::InboundProbe(event));
        }
    }

    fn inject_address_change(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        self.inner.inject_address_change(peer, conn, old, new);

        if old.is_relayed() && new.is_relayed() {
            return;
        }
        let connections = self.connected.get_mut(peer).expect("Peer is connected.");
        let addr = new.get_remote_address();
        let observed_addr =
            if !new.is_relayed() && (!self.config.only_global_ips || addr.is_global_ip()) {
                Some(addr.clone())
            } else {
                None
            };
        connections.insert(*conn, observed_addr);
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_new_listen_addr(id, addr);
        self.as_client().on_new_address();
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_expired_listen_addr(id, addr);
        self.as_client().on_expired_address(addr);
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_new_external_addr(addr);
        self.as_client().on_new_address();
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_expired_external_addr(addr);
        self.as_client().on_expired_address(addr);
    }

    fn poll(&mut self, cx: &mut Context<'_>, params: &mut impl PollParameters) -> Poll<Action> {
        loop {
            if let Some(event) = self.pending_out_events.pop_front() {
                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
            }

            let mut is_inner_pending = false;
            match self.inner.poll(cx, params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(event)) => {
                    let (mut events, action) = match event {
                        RequestResponseEvent::Message {
                            message: RequestResponseMessage::Response { .. },
                            ..
                        }
                        | RequestResponseEvent::OutboundFailure { .. } => {
                            self.as_client().handle_event(params, event)
                        }
                        RequestResponseEvent::Message {
                            message: RequestResponseMessage::Request { .. },
                            ..
                        }
                        | RequestResponseEvent::InboundFailure { .. } => {
                            self.as_server().handle_event(params, event)
                        }
                        RequestResponseEvent::ResponseSent { .. } => (VecDeque::new(), None),
                    };
                    self.pending_out_events.append(&mut events);
                    if let Some(action) = action {
                        return Poll::Ready(action);
                    }
                }
                Poll::Ready(action) => return Poll::Ready(action.map_out(|_| unreachable!())),
                Poll::Pending => is_inner_pending = true,
            }

            match self.as_client().poll_auto_probe(params, cx) {
                Poll::Ready(event) => self
                    .pending_out_events
                    .push_back(Event::OutboundProbe(event)),
                Poll::Pending if is_inner_pending => return Poll::Pending,
                Poll::Pending => {}
            }
        }
    }

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.inner.addresses_of_peer(peer)
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
        handler: Self::ConnectionHandler,
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

type Action = NetworkBehaviourAction<
    <Behaviour as NetworkBehaviour>::OutEvent,
    <Behaviour as NetworkBehaviour>::ConnectionHandler,
>;

// Trait implemented for `AsClient` as `AsServer` to handle events from the inner [`RequestResponse`] Protocol.
trait HandleInnerEvent {
    fn handle_event(
        &mut self,
        params: &mut impl PollParameters,
        event: RequestResponseEvent<DialRequest, DialResponse>,
    ) -> (VecDeque<Event>, Option<Action>);
}

trait GlobalIp {
    fn is_global_ip(&self) -> bool;
}

impl GlobalIp for Multiaddr {
    fn is_global_ip(&self) -> bool {
        match self.iter().next() {
            Some(Protocol::Ip4(a)) => a.is_global_ip(),
            Some(Protocol::Ip6(a)) => a.is_global_ip(),
            _ => false,
        }
    }
}

impl GlobalIp for std::net::Ipv4Addr {
    // NOTE: The below logic is copied from `std::net::Ipv4Addr::is_global`, which is at the time of
    // writing behind the unstable `ip` feature.
    // See https://github.com/rust-lang/rust/issues/27709 for more info.
    fn is_global_ip(&self) -> bool {
        // Check if this address is 192.0.0.9 or 192.0.0.10. These addresses are the only two
        // globally routable addresses in the 192.0.0.0/24 range.
        if u32::from_be_bytes(self.octets()) == 0xc0000009
            || u32::from_be_bytes(self.octets()) == 0xc000000a
        {
            return true;
        }

        // Copied from the unstable method `std::net::Ipv4Addr::is_shared`.
        fn is_shared(addr: &std::net::Ipv4Addr) -> bool {
            addr.octets()[0] == 100 && (addr.octets()[1] & 0b1100_0000 == 0b0100_0000)
        }

        // Copied from the unstable method `std::net::Ipv4Addr::is_reserved`.
        //
        // **Warning**: As IANA assigns new addresses, this logic will be
        // updated. This may result in non-reserved addresses being
        // treated as reserved in code that relies on an outdated version
        // of this method.
        fn is_reserved(addr: &std::net::Ipv4Addr) -> bool {
            addr.octets()[0] & 240 == 240 && !addr.is_broadcast()
        }

        // Copied from the unstable method `std::net::Ipv4Addr::is_benchmarking`.
        fn is_benchmarking(addr: &std::net::Ipv4Addr) -> bool {
            addr.octets()[0] == 198 && (addr.octets()[1] & 0xfe) == 18
        }

        !self.is_private()
            && !self.is_loopback()
            && !self.is_link_local()
            && !self.is_broadcast()
            && !self.is_documentation()
            && !is_shared(self)
            // addresses reserved for future protocols (`192.0.0.0/24`)
            && !(self.octets()[0] == 192 && self.octets()[1] == 0 && self.octets()[2] == 0)
            && !is_reserved(self)
            && !is_benchmarking(self)
            // Make sure the address is not in 0.0.0.0/8
            && self.octets()[0] != 0
    }
}

impl GlobalIp for std::net::Ipv6Addr {
    // NOTE: The below logic is copied from `std::net::Ipv6Addr::is_global`, which is at the time of
    // writing behind the unstable `ip` feature.
    // See https://github.com/rust-lang/rust/issues/27709 for more info.
    //
    // Note that contrary to `Ipv4Addr::is_global_ip` this currently checks for global scope
    // rather than global reachability.
    fn is_global_ip(&self) -> bool {
        // Copied from the unstable method `std::net::Ipv6Addr::is_unicast`.
        fn is_unicast(addr: &std::net::Ipv6Addr) -> bool {
            !addr.is_multicast()
        }
        // Copied from the unstable method `std::net::Ipv6Addr::is_unicast_link_local`.
        fn is_unicast_link_local(addr: &std::net::Ipv6Addr) -> bool {
            (addr.segments()[0] & 0xffc0) == 0xfe80
        }
        // Copied from the unstable method `std::net::Ipv6Addr::is_unique_local`.
        fn is_unique_local(addr: &std::net::Ipv6Addr) -> bool {
            (addr.segments()[0] & 0xfe00) == 0xfc00
        }
        // Copied from the unstable method `std::net::Ipv6Addr::is_documentation`.
        fn is_documentation(addr: &std::net::Ipv6Addr) -> bool {
            (addr.segments()[0] == 0x2001) && (addr.segments()[1] == 0xdb8)
        }

        // Copied from the unstable method `std::net::Ipv6Addr::is_unicast_global`.
        fn is_unicast_global(addr: &std::net::Ipv6Addr) -> bool {
            is_unicast(addr)
                && !addr.is_loopback()
                && !is_unicast_link_local(addr)
                && !is_unique_local(addr)
                && !addr.is_unspecified()
                && !is_documentation(addr)
        }

        // Variation of unstable method [`std::net::Ipv6Addr::multicast_scope`] that instead of the
        // `Ipv6MulticastScope` just returns if the scope is global or not.
        // Equivalent to `Ipv6Addr::multicast_scope(..).map(|scope| matches!(scope, Ipv6MulticastScope::Global))`.
        fn is_multicast_scope_global(addr: &std::net::Ipv6Addr) -> Option<bool> {
            match addr.segments()[0] & 0x000f {
                14 => Some(true),         // Global multicast scope.
                1..=5 | 8 => Some(false), // Local multicast scope.
                _ => None,                // Unknown multicast scope.
            }
        }

        match is_multicast_scope_global(self) {
            Some(true) => true,
            None => is_unicast_global(self),
            _ => false,
        }
    }
}
