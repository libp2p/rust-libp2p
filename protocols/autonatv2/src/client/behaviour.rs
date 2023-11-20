use std::{
    collections::{HashMap, HashSet, VecDeque},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use either::Either;
use libp2p_core::{multiaddr::Protocol, transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionEstablished, ExternalAddrConfirmed},
    dial_opts::{DialOpts, PeerCondition},
    ConnectionClosed, ConnectionDenied, ConnectionHandler, ConnectionId, DialFailure, FromSwarm,
    NetworkBehaviour, NewExternalAddrCandidate, NotifyHandler, ToSwarm,
};
use rand::{distributions::Standard, seq::SliceRandom, Rng};
use rand_core::RngCore;

use crate::{global_only::IpExt, request_response::DialRequest};

use super::handler::{dial_back, dial_request, Handler, TestEnd};

struct IntervalTicker {
    interval: Duration,
    last_tick: Instant,
}

impl IntervalTicker {
    fn ready(&mut self) -> bool {
        if self.last_tick.elapsed() >= self.interval {
            self.last_tick = Instant::now();
            true
        } else {
            false
        }
    }
}

pub(crate) struct Config {
    pub(crate) test_server_count: usize,
    pub(crate) max_addrs_count: usize,
}

pub(crate) struct Behaviour<R>
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
    peers_to_handlers: HashMap<PeerId, ConnectionId>,
    ticker: IntervalTicker,
}

impl<R> NetworkBehaviour for Behaviour<R>
where
    R: RngCore + 'static,
{
    type ConnectionHandler = Handler;

    type ToSwarm = ();

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
        Ok(Either::Left(dial_request::Handler::new()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _peer: PeerId,
        addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        if addr_is_local(addr) {
            self.local_peers.insert(connection_id);
        }
        Ok(Either::Right(dial_back::Handler::new()))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::NewExternalAddrCandidate(NewExternalAddrCandidate { addr }) => {
                *self.address_candidates.entry(addr.clone()).or_default() += 1;
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
                tracing::trace!("connection with {peer_id:?} closed");
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
        event: <Handler as ConnectionHandler>::ToBehaviour,
    ) {
        self.peers_to_handlers
            .entry(peer_id)
            .or_insert(connection_id);
        match event {
            Either::Right(Ok(nonce)) => {
                if self.pending_nonces.remove(&nonce) {
                    tracing::trace!("Received pending nonce from {peer_id:?}");
                } else {
                    tracing::warn!("Received unexpected nonce from {peer_id:?}, this means that another node tried to be reachable on an address this node is reachable on.");
                }
            }
            Either::Right(Err(err)) => {
                tracing::debug!("Dial back failed: {:?}", err);
            }
            Either::Left(dial_request::ToBehaviour::PeerHasServerSupport) => {
                if !self.known_servers.contains(&peer_id) {
                    self.known_servers.push(peer_id);
                }
            }
            Either::Left(dial_request::ToBehaviour::TestCompleted(Ok(TestEnd {
                dial_request: DialRequest { nonce, addrs },
                suspicious_addr,
                reachable_addr,
            }))) => {
                if self.pending_nonces.remove(&nonce) {
                    tracing::debug!(
                        "server reported reachbility, but didn't actually reached this node."
                    );
                    return;
                }
                if !suspicious_addr.is_empty() {
                    tracing::trace!(
                        "server reported suspicious addresses: {:?}",
                        suspicious_addr
                    );
                }
                self.pending_events.extend(
                    addrs
                        .into_iter()
                        .take_while(|addr| addr != &reachable_addr)
                        .map(ToSwarm::ExternalAddrExpired),
                );
                self.pending_events
                    .push_back(ToSwarm::ExternalAddrConfirmed(reachable_addr));
            }
            Either::Left(dial_request::ToBehaviour::TestCompleted(
                Err(dial_request::Error::UnableToConnectOnSelectedAddress { addr: Some(addr) })
            ))
            | Either::Left(dial_request::ToBehaviour::TestCompleted(
                Err(dial_request::Error::FailureDuringDialBack { addr: Some(addr) })
            )) => {
                self.pending_events
                    .push_back(ToSwarm::ExternalAddrExpired(addr));
            }
            Either::Left(dial_request::ToBehaviour::TestCompleted(Err(err))) => {
                tracing::debug!("Test failed: {:?}", err);
            }
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, <Handler as ConnectionHandler>::FromBehaviour>> {
        let pending_event = self.poll_pending_events();
        if pending_event.is_ready() {
            return pending_event;
        }
        if self.ticker.ready() && !self.known_servers.is_empty() {
            let mut entries = self.address_candidates.drain().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(_, count)| *count);
            let addrs = entries
                .into_iter()
                .rev()
                .map(|(addr, _)| addr)
                .take(self.config.max_addrs_count)
                .collect::<Vec<_>>();
            let peers = if self.known_servers.len() < self.config.test_server_count {
                self.known_servers.clone()
            } else {
                self.known_servers
                    .choose_multiple(&mut self.rng, self.config.test_server_count)
                    .copied()
                    .collect()
            };
            for peer in peers {
                let nonce = self.rng.gen();
                let req = DialRequest {
                    nonce,
                    addrs: addrs.clone(),
                };
                self.pending_nonces.insert(nonce);
                self.submit_req_for_peer(peer, req);
            }
            let pending_event = self.poll_pending_events();
            if pending_event.is_ready() {
                return pending_event;
            }
        }
        Poll::Pending
    }
}

impl<R> Behaviour<R>
where
    R: RngCore + 'static,
{
    fn submit_req_for_peer(&mut self, peer: PeerId, req: DialRequest) {
        if let Some(conn_id) = self.peers_to_handlers.get(&peer) {
            self.pending_events.push_back(ToSwarm::NotifyHandler {
                peer_id: peer,
                handler: NotifyHandler::One(*conn_id),
                event: Either::Left(dial_request::FromBehaviour::PerformRequest(req)),
            });
        } else {
            tracing::debug!(
                "There should be a connection to {:?}, but there isn't",
                peer
            );
        }
    }

    fn handle_no_connection(&mut self, peer_id: PeerId, connection_id: ConnectionId) {
        if matches!(self.peers_to_handlers.get(&peer_id), Some(conn_id) if *conn_id == connection_id)
        {
            self.peers_to_handlers.remove(&peer_id);
        }
        self.known_servers.retain(|p| p != &peer_id);
    }

    fn poll_pending_events(
        &mut self,
    ) -> Poll<
        ToSwarm<<Self as NetworkBehaviour>::ToSwarm, <Handler as ConnectionHandler>::FromBehaviour>,
    > {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        Poll::Pending
    }
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
