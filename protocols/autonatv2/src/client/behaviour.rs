use std::{
    collections::{HashMap, HashSet, VecDeque},
    task::{Context, Poll},
};

use either::Either;
use libp2p_core::{multiaddr::Protocol, transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::ConnectionEstablished,
    dial_opts::{DialOpts, PeerCondition},
    ConnectionClosed, ConnectionDenied, ConnectionHandler, ConnectionId, DialFailure, FromSwarm,
    NetworkBehaviour, NewExternalAddrCandidate, NotifyHandler, ToSwarm,
};
use rand::{seq::SliceRandom, Rng};
use rand_core::RngCore;

use crate::{global_only::IpExt, request_response::DialRequest};

use super::handler::{
    new_handler, Handler, RequestError, RequestFromBehaviour, RequestToBehaviour, TestEnd,
};

pub(crate) struct Config {
    pub(crate) test_server_count: usize,
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
    peers_to_handlers: HashMap<PeerId, ConnectionId>,
    pending_req_for_peer: HashMap<PeerId, VecDeque<DialRequest>>,
    pending_requests: VecDeque<DialRequest>,
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
        Ok(new_handler())
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
        Ok(new_handler())
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::NewExternalAddrCandidate(NewExternalAddrCandidate { addr }) => {
                for _ in 0..self.config.test_server_count {
                    let nonce = self.rng.gen();
                    self.pending_requests.push_back(DialRequest {
                        addrs: vec![addr.clone()],
                        nonce,
                    });
                    self.pending_nonces.insert(nonce);
                }
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
            Either::Left(RequestToBehaviour::PeerHasServerSupport) => {
                if !self.known_servers.contains(&peer_id) {
                    self.known_servers.push(peer_id);
                }
            }
            Either::Left(RequestToBehaviour::TestCompleted(Ok(TestEnd {
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
            Either::Left(RequestToBehaviour::TestCompleted(Err(
                RequestError::UnableToConnectOnSelectedAddress { addr: Some(addr) },
            )))
            | Either::Left(RequestToBehaviour::TestCompleted(Err(
                RequestError::FailureDuringDialBack { addr: Some(addr) },
            ))) => {
                self.pending_events
                    .push_back(ToSwarm::ExternalAddrExpired(addr));
            }
            Either::Left(RequestToBehaviour::TestCompleted(Err(err))) => {
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
        self.pending_req_for_peer.retain(|_, reqs| !reqs.is_empty());
        for (peer, dial_requests) in &mut self.pending_req_for_peer {
            if let Some(conn_id) = self.peers_to_handlers.get(peer) {
                let dial_request = dial_requests.pop_front().unwrap();
                return Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id: *peer,
                    handler: NotifyHandler::One(*conn_id),
                    event: Either::Left(RequestFromBehaviour::PerformRequest(dial_request)),
                });
            }
        }
        if let Some(dial_request) = self.pending_requests.pop_front() {
            if self.known_servers.is_empty() {
                self.pending_requests.push_front(dial_request);
            } else {
                let peer = self.known_servers.choose(&mut self.rng).unwrap();
                self.submit_req_for_peer(*peer, dial_request);
                return self.poll_pending_events();
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
                event: Either::Left(RequestFromBehaviour::PerformRequest(req)),
            });
        } else {
            self.pending_events.push_back(ToSwarm::Dial {
                opts: DialOpts::peer_id(peer)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .build(),
            });
            self.pending_req_for_peer
                .entry(peer)
                .or_default()
                .push_back(req);
        }
    }

    fn handle_no_connection(&mut self, peer_id: PeerId, connection_id: ConnectionId) {
        if matches!(self.peers_to_handlers.get(&peer_id), Some(conn_id) if *conn_id == connection_id)
        {
            self.peers_to_handlers.remove(&peer_id);
        }
        for dial_request in self
            .pending_req_for_peer
            .remove(&peer_id)
            .unwrap_or_default()
        {
            if let Some(new_peer) = self.known_servers.choose(&mut self.rng) {
                self.submit_req_for_peer(*new_peer, dial_request);
            } else {
                self.pending_requests.push_front(dial_request);
            }
        }
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
