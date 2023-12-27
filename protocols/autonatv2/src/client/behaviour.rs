use std::{
    collections::{HashMap, HashSet, VecDeque},
    task::{Context, Poll},
    time::Duration,
};

use either::Either;
use futures::FutureExt;
use futures_timer::Delay;
use libp2p_core::{multiaddr::Protocol, transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionEstablished, ExternalAddrConfirmed},
    ConnectionClosed, ConnectionDenied, ConnectionHandler, ConnectionId, DialFailure, FromSwarm,
    NetworkBehaviour, NewExternalAddrCandidate, NotifyHandler, ToSwarm,
};
use rand::{seq::SliceRandom, Rng};
use rand_core::{OsRng, RngCore};
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

use crate::{client::handler::dial_request::InternalError, Nonce};
use crate::{global_only::IpExt, protocol::DialRequest};

use super::handler::{
    dial_back,
    dial_request::{self},
    TestEnd,
};

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub(crate) test_server_count: usize,
    pub(crate) max_addrs_count: usize,
    pub(crate) recheck_interval: Duration,
}

impl Config {
    pub fn with_test_server_count(self, test_server_count: usize) -> Self {
        Self {
            test_server_count,
            ..self
        }
    }

    pub fn with_max_addrs_count(self, max_addrs_count: usize) -> Self {
        Self {
            max_addrs_count,
            ..self
        }
    }

    pub fn with_recheck_interval(self, recheck_interval: Duration) -> Self {
        Self {
            recheck_interval,
            ..self
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            test_server_count: 3,
            max_addrs_count: 10,
            recheck_interval: Duration::from_secs(5),
        }
    }
}
pub struct Behaviour<R = OsRng>
where
    R: RngCore + 'static,
{
    local_peers: HashSet<ConnectionId>,
    pending_nonces: HashSet<u64>,
    known_servers: Vec<PeerId>,
    rng: R,
    config: Config,
    pending_events: VecDeque<
        ToSwarm<
            <Self as NetworkBehaviour>::ToSwarm,
            <<Self as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::FromBehaviour,
        >,
    >,
    address_candidates: HashMap<Multiaddr, usize>,
    already_tested: HashSet<Multiaddr>,
    peers_to_handlers: HashMap<PeerId, ConnectionId>,
    next_tick: Delay,
}

impl<R> NetworkBehaviour for Behaviour<R>
where
    R: RngCore + 'static,
{
    type ConnectionHandler = Either<dial_request::Handler, dial_back::Handler>;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        if addr_is_local(remote_addr) {
            self.local_peers.insert(connection_id);
        }
        Ok(Either::Right(dial_back::Handler::new()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        if addr_is_local(addr) {
            self.local_peers.insert(connection_id);
        }
        Ok(Either::Left(dial_request::Handler::new(peer)))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::NewExternalAddrCandidate(NewExternalAddrCandidate { addr }) => {
                self.inject_address_candiate(addr.clone())
            }
            FromSwarm::ExternalAddrConfirmed(ExternalAddrConfirmed { addr }) => {
                self.address_candidates.remove(addr);
            }
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                ..
            }) => {
                self.peers_to_handlers
                    .entry(peer_id)
                    .or_insert(connection_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                self.handle_no_connection(peer_id, connection_id);
            }
            FromSwarm::DialFailure(DialFailure {
                peer_id: Some(peer_id),
                error,
                connection_id,
            }) => {
                tracing::trace!("dialing {peer_id:?} failed: {error:?}");
                self.handle_no_connection(peer_id, connection_id);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: <Self::ConnectionHandler as ConnectionHandler>::ToBehaviour,
    ) {
        if matches!(event, Either::Left(_)) {
            self.peers_to_handlers
                .entry(peer_id)
                .or_insert(connection_id);
        }
        match event {
            Either::Right(nonce) => {
                if self.pending_nonces.remove(&nonce) {
                    tracing::trace!("Received pending nonce from {peer_id:?}");
                } else {
                    tracing::warn!("Received unexpected nonce from {peer_id:?}, this means that another node tried to be reachable on an address this node is reachable on.");
                }
            }
            Either::Left(dial_request::ToBehaviour::PeerHasServerSupport) => {
                self.inject_kown_server(peer_id);
            }
            Either::Left(dial_request::ToBehaviour::TestCompleted(Ok(TestEnd {
                dial_request: DialRequest { nonce, .. },
                reachable_addr,
            }))) => {
                if self.pending_nonces.remove(&nonce) {
                    tracing::debug!(
                        "server reported reachbility, but didn't actually reached this node."
                    );
                    return;
                }
                self.pending_events
                    .push_back(ToSwarm::ExternalAddrConfirmed(reachable_addr));
            }
            Either::Left(dial_request::ToBehaviour::TestCompleted(Err(err))) => {
                match err.internal.as_ref() {
                    dial_request::InternalError::FailureDuringDialBack { addr: Some(addr) }
                    | dial_request::InternalError::UnableToConnectOnSelectedAddress {
                        addr: Some(addr),
                    } => {
                        self.pending_events
                            .push_back(ToSwarm::ExternalAddrExpired(addr.clone()));
                    }
                    dial_request::InternalError::InternalServer
                    | dial_request::InternalError::DataRequestTooLarge { .. }
                    | dial_request::InternalError::DataRequestTooSmall { .. }
                    | dial_request::InternalError::InvalidResponse
                    | dial_request::InternalError::ServerRejectedDialRequest
                    | dial_request::InternalError::InvalidReferencedAddress { .. }
                    | dial_request::InternalError::ServerChoseNotToDialAnyAddress => {
                        self.handle_no_connection(peer_id, connection_id);
                    }
                    _ => {
                        tracing::debug!("Test failed: {:?}", err);
                    }
                }
            }
            Either::Left(dial_request::ToBehaviour::StatusUpdate(update)) => self
                .pending_events
                .push_back(ToSwarm::GenerateEvent(update)),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, <Self::ConnectionHandler as ConnectionHandler>::FromBehaviour>>
    {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        if self.next_tick.poll_unpin(cx).is_ready() {
            self.inject_address_candiate_test();
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(event);
            }
        }
        Poll::Pending
    }
}

impl<R> Behaviour<R>
where
    R: RngCore + 'static,
{
    pub fn new(rng: R, config: Config) -> Self {
        Self {
            local_peers: HashSet::new(),
            pending_nonces: HashSet::new(),
            known_servers: Vec::new(),
            rng,
            next_tick: Delay::new(config.recheck_interval),
            config,
            pending_events: VecDeque::new(),
            address_candidates: HashMap::new(),
            peers_to_handlers: HashMap::new(),
            already_tested: HashSet::new(),
        }
    }

    /// Injects a known server into the behaviour. It's mostly useful if you are not using identify
    /// or for testing purposes.
    pub fn inject_kown_server(&mut self, peer: PeerId) {
        if !self.known_servers.contains(&peer) {
            self.known_servers.push(peer);
        }
    }

    /// Inject a new address candidate into the behaviour.
    pub fn inject_address_candiate(&mut self, addr: Multiaddr) {
        *self.address_candidates.entry(addr).or_default() += 1;
    }

    /// Inject an immediate test for all pending address candidates.
    pub fn inject_address_candiate_test(&mut self) {
        if self.known_servers.is_empty() || self.address_candidates.is_empty() {
            return;
        }
        let mut entries = self
            .address_candidates
            .iter()
            .filter(|(addr, _)| !self.already_tested.contains(addr))
            .map(|(addr, count)| (addr.clone(), *count))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return;
        }
        entries.sort_unstable_by_key(|(_, count)| *count);
        let addrs = entries
            .iter()
            .rev()
            .map(|(addr, _)| addr)
            .take(self.config.max_addrs_count)
            .cloned()
            .collect::<Vec<_>>();
        self.already_tested.extend(addrs.iter().cloned());
        let peers = if self.known_servers.len() < self.config.test_server_count {
            self.known_servers.clone()
        } else {
            self.known_servers
                .choose_multiple(&mut self.rng, self.config.test_server_count)
                .copied()
                .collect()
        };
        for peer in peers {
            let mut remaining_entries = entries.clone();
            let mut addrs = Vec::with_capacity(self.config.max_addrs_count);
            while !remaining_entries.is_empty() && addrs.len() < self.config.max_addrs_count {
                let addr = remaining_entries
                    .choose_weighted(&mut self.rng, |item| item.1)
                    .unwrap()
                    .0
                    .clone();
                remaining_entries.retain(|(a, _)| a != &addr);
                addrs.push(addr);
            }
            let nonce = self.rng.gen();
            let req = DialRequest { nonce, addrs };
            self.submit_req_for_peer(peer, req, nonce);
        }
        self.next_tick.reset(self.config.recheck_interval);
    }

    fn submit_req_for_peer(&mut self, peer: PeerId, req: DialRequest, nonce: Nonce) {
        self.pending_nonces.insert(nonce);
        if let Some(conn_id) = self.peers_to_handlers.get(&peer) {
            self.pending_events.push_back(ToSwarm::NotifyHandler {
                peer_id: peer,
                handler: NotifyHandler::One(*conn_id),
                event: Either::Left(req),
            });
        }
    }

    fn handle_no_connection(&mut self, peer_id: PeerId, connection_id: ConnectionId) {
        if matches!(self.peers_to_handlers.get(&peer_id), Some(conn_id) if *conn_id == connection_id)
        {
            self.peers_to_handlers.remove(&peer_id);
        }
        self.known_servers.retain(|p| p != &peer_id);
    }
}

impl Default for Behaviour<OsRng> {
    fn default() -> Self {
        Self::new(OsRng, Config::default())
    }
}

pub struct Error {
    pub(crate) internal: Arc<InternalError>,
}

impl Error {
    pub(crate) fn duplicate(&self) -> Self {
        Self {
            internal: Arc::clone(&self.internal),
        }
    }
}

impl From<InternalError> for Error {
    fn from(value: InternalError) -> Self {
        Self {
            internal: Arc::new(value),
        }
    }
}

impl From<Arc<InternalError>> for Error {
    fn from(value: Arc<InternalError>) -> Self {
        Self { internal: value }
    }
}

impl From<&Arc<InternalError>> for Error {
    fn from(value: &Arc<InternalError>) -> Self {
        Self {
            internal: Arc::clone(value),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.internal, f)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.internal, f)
    }
}

#[derive(Debug)]
pub struct Event {
    /// The address that was selected for testing.
    /// Is `None` in the case that the server respond with something unexpected.
    pub tested_addr: Option<Multiaddr>,
    /// The amount of data that was sent to the server.
    /// Is 0 if it wasn't necessary to send any data.
    /// Otherwise it's a number between 30.000 and 100.000.
    pub data_amount: usize,
    /// The peer id of the server that was selected for testing.
    pub server: PeerId,
    /// The result of the test. If the test was successful, this is `Ok(())`.
    /// Otherwise it's an error.
    pub result: Result<(), Error>,
}

fn addr_is_local(addr: &Multiaddr) -> bool {
    addr.iter().any(|c| match c {
        Protocol::Dns(addr)
        | Protocol::Dns4(addr)
        | Protocol::Dns6(addr)
        | Protocol::Dnsaddr(addr) => addr.ends_with(".local"),
        Protocol::Ip4(ip) => !IpExt::is_global(&ip),
        Protocol::Ip6(ip) => !IpExt::is_global(&ip),
        _ => false,
    })
}
