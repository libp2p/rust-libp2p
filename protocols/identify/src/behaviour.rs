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

use crate::handler::{self, Handler, InEvent};
use crate::protocol::{Info, Protocol, UpgradeError};
use libp2p_core::{multiaddr, ConnectedPoint, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_identity::PublicKey;
use libp2p_swarm::behaviour::{ConnectionClosed, ConnectionEstablished, DialFailure, FromSwarm};
use libp2p_swarm::{
    AddressScore, ConnectionDenied, ConnectionHandlerUpgrErr, DialError, ExternalAddresses,
    ListenAddresses, NetworkBehaviour, NotifyHandler, PollParameters, StreamProtocol,
    THandlerInEvent, ToSwarm,
};
use libp2p_swarm::{ConnectionId, THandler, THandlerOutEvent};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter::FromIterator,
    task::Context,
    task::Poll,
    time::Duration,
};

/// Network behaviour that automatically identifies nodes periodically, returns information
/// about them, and answers identify queries from other nodes.
///
/// All external addresses of the local node supposedly observed by remotes
/// are reported via [`ToSwarm::ReportObservedAddr`] with a
/// [score](AddressScore) of `1`.
pub struct Behaviour {
    config: Config,
    /// For each peer we're connected to, the observed address to send back to it.
    connected: HashMap<PeerId, HashMap<ConnectionId, Multiaddr>>,
    /// Pending requests to be fulfilled, either `Handler` requests for `Behaviour` info
    /// to address identification requests, or push requests to peers
    /// with current information about the local peer.
    requests: Vec<Request>,
    /// Pending events to be emitted when polled.
    events: VecDeque<ToSwarm<Event, InEvent>>,
    /// The addresses of all peers that we have discovered.
    discovered_peers: PeerCache,

    listen_addresses: ListenAddresses,
    external_addresses: ExternalAddresses,
}

/// A `Behaviour` request to be fulfilled, either `Handler` requests for `Behaviour` info
/// to address identification requests, or push requests to peers
/// with current information about the local peer.
#[derive(Debug, PartialEq, Eq)]
struct Request {
    peer_id: PeerId,
    protocol: Protocol,
}

/// Configuration for the [`identify::Behaviour`](Behaviour).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Config {
    /// Application-specific version of the protocol family used by the peer,
    /// e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// The public key of the local node. To report on the wire.
    pub local_public_key: PublicKey,
    /// Name and version of the local peer implementation, similar to the
    /// `User-Agent` header in the HTTP protocol.
    ///
    /// Defaults to `rust-libp2p/<libp2p-identify-version>`.
    pub agent_version: String,
    /// The initial delay before the first identification request
    /// is sent to a remote on a newly established connection.
    ///
    /// Defaults to 0ms.
    #[deprecated(note = "The `initial_delay` is no longer necessary and will be
                completely removed since a remote should be able to instantly
                answer to an identify request")]
    pub initial_delay: Duration,
    /// The interval at which identification requests are sent to
    /// the remote on established connections after the first request,
    /// i.e. the delay between identification requests.
    ///
    /// Defaults to 5 minutes.
    pub interval: Duration,

    /// Whether new or expired listen addresses of the local node should
    /// trigger an active push of an identify message to all connected peers.
    ///
    /// Enabling this option can result in connected peers being informed
    /// earlier about new or expired listen addresses of the local node,
    /// i.e. before the next periodic identify request with each peer.
    ///
    /// Disabled by default.
    pub push_listen_addr_updates: bool,

    /// How many entries of discovered peers to keep before we discard
    /// the least-recently used one.
    ///
    /// Disabled by default.
    pub cache_size: usize,
}

impl Config {
    /// Creates a new configuration for the identify [`Behaviour`] that
    /// advertises the given protocol version and public key.
    #[allow(deprecated)]
    pub fn new(protocol_version: String, local_public_key: PublicKey) -> Self {
        Self {
            protocol_version,
            agent_version: format!("rust-libp2p/{}", env!("CARGO_PKG_VERSION")),
            local_public_key,
            initial_delay: Duration::from_millis(0),
            interval: Duration::from_secs(5 * 60),
            push_listen_addr_updates: false,
            cache_size: 100,
        }
    }

    /// Configures the agent version sent to peers.
    pub fn with_agent_version(mut self, v: String) -> Self {
        self.agent_version = v;
        self
    }

    /// Configures the initial delay before the first identification
    /// request is sent on a newly established connection to a peer.
    #[deprecated(note = "The `initial_delay` is no longer necessary and will be
                completely removed since a remote should be able to instantly
                answer to an identify request thus also this setter will be removed")]
    #[allow(deprecated)]
    pub fn with_initial_delay(mut self, d: Duration) -> Self {
        self.initial_delay = d;
        self
    }

    /// Configures the interval at which identification requests are
    /// sent to peers after the initial request.
    pub fn with_interval(mut self, d: Duration) -> Self {
        self.interval = d;
        self
    }

    /// Configures whether new or expired listen addresses of the local
    /// node should trigger an active push of an identify message to all
    /// connected peers.
    pub fn with_push_listen_addr_updates(mut self, b: bool) -> Self {
        self.push_listen_addr_updates = b;
        self
    }

    /// Configures the size of the LRU cache, caching addresses of discovered peers.
    pub fn with_cache_size(mut self, cache_size: usize) -> Self {
        self.cache_size = cache_size;
        self
    }
}

impl Behaviour {
    /// Creates a new identify [`Behaviour`].
    pub fn new(config: Config) -> Self {
        let discovered_peers = match NonZeroUsize::new(config.cache_size) {
            None => PeerCache::disabled(),
            Some(size) => PeerCache::enabled(size),
        };

        Self {
            config,
            connected: HashMap::new(),
            requests: Vec::new(),
            events: VecDeque::new(),
            discovered_peers,
            listen_addresses: Default::default(),
            external_addresses: Default::default(),
        }
    }

    /// Initiates an active push of the local peer information to the given peers.
    pub fn push<I>(&mut self, peers: I)
    where
        I: IntoIterator<Item = PeerId>,
    {
        for p in peers {
            if !self.connected.contains_key(&p) {
                log::debug!("Not pushing to {p} because we are not connected");
                continue;
            }

            let request = Request {
                peer_id: p,
                protocol: Protocol::Push,
            };
            if !self.requests.contains(&request) {
                self.requests.push(request);
            }
        }
    }

    fn on_connection_established(
        &mut self,
        ConnectionEstablished {
            peer_id,
            connection_id: conn,
            endpoint,
            failed_addresses,
            ..
        }: ConnectionEstablished,
    ) {
        let addr = match endpoint {
            ConnectedPoint::Dialer { address, .. } => address.clone(),
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr.clone(),
        };

        self.connected
            .entry(peer_id)
            .or_default()
            .insert(conn, addr);

        if let Some(entry) = self.discovered_peers.get_mut(&peer_id) {
            for addr in failed_addresses {
                entry.remove(addr);
            }
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type OutEvent = Event;

    #[allow(deprecated)]
    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(
            self.config.initial_delay,
            self.config.interval,
            peer,
            self.config.local_public_key.clone(),
            self.config.protocol_version.clone(),
            self.config.agent_version.clone(),
            remote_addr.clone(),
        ))
    }

    #[allow(deprecated)]
    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(
            self.config.initial_delay,
            self.config.interval,
            peer,
            self.config.local_public_key.clone(),
            self.config.protocol_version.clone(),
            self.config.agent_version.clone(),
            addr.clone(), // TODO: This is weird? That is the public address we dialed, shouldn't need to tell the other party?
        ))
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {
            handler::Event::Identified(mut info) => {
                // Remove invalid multiaddrs.
                info.listen_addrs
                    .retain(|addr| multiaddr_matches_peer_id(addr, &peer_id));

                // Replace existing addresses to prevent other peer from filling up our memory.
                self.discovered_peers
                    .put(peer_id, info.listen_addrs.iter().cloned());

                let observed = info.observed_addr.clone();
                self.events
                    .push_back(ToSwarm::GenerateEvent(Event::Received { peer_id, info }));
                self.events.push_back(ToSwarm::ReportObservedAddr {
                    address: observed,
                    score: AddressScore::Finite(1),
                });
            }
            handler::Event::Identification(peer) => {
                self.events
                    .push_back(ToSwarm::GenerateEvent(Event::Sent { peer_id: peer }));
            }
            handler::Event::IdentificationPushed => {
                self.events
                    .push_back(ToSwarm::GenerateEvent(Event::Pushed { peer_id }));
            }
            handler::Event::Identify => {
                self.requests.push(Request {
                    peer_id,
                    protocol: Protocol::Identify(connection_id),
                });
            }
            handler::Event::IdentificationError(error) => {
                self.events
                    .push_back(ToSwarm::GenerateEvent(Event::Error { peer_id, error }));
            }
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        // Check for pending requests.
        match self.requests.pop() {
            Some(Request {
                peer_id,
                protocol: Protocol::Push,
            }) => Poll::Ready(ToSwarm::NotifyHandler {
                peer_id,
                handler: NotifyHandler::Any,
                event: InEvent {
                    listen_addrs: self
                        .listen_addresses
                        .iter()
                        .chain(self.external_addresses.iter())
                        .cloned()
                        .collect(),
                    supported_protocols: supported_protocols(params),
                    protocol: Protocol::Push,
                },
            }),
            Some(Request {
                peer_id,
                protocol: Protocol::Identify(connection_id),
            }) => Poll::Ready(ToSwarm::NotifyHandler {
                peer_id,
                handler: NotifyHandler::One(connection_id),
                event: InEvent {
                    listen_addrs: self
                        .listen_addresses
                        .iter()
                        .chain(self.external_addresses.iter())
                        .cloned()
                        .collect(),
                    supported_protocols: supported_protocols(params),
                    protocol: Protocol::Identify(connection_id),
                },
            }),
            None => Poll::Pending,
        }
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        let peer = match maybe_peer {
            None => return Ok(vec![]),
            Some(peer) => peer,
        };

        Ok(self.discovered_peers.get(&peer))
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        self.listen_addresses.on_swarm_event(&event);
        self.external_addresses.on_swarm_event(&event);

        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                self.on_connection_established(connection_established)
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                remaining_established,
                ..
            }) => {
                if remaining_established == 0 {
                    self.connected.remove(&peer_id);
                    self.requests.retain(|request| {
                        request
                            != &Request {
                                peer_id,
                                protocol: Protocol::Push,
                            }
                    });
                } else if let Some(addrs) = self.connected.get_mut(&peer_id) {
                    addrs.remove(&connection_id);
                }
            }
            FromSwarm::DialFailure(DialFailure { peer_id, error, .. }) => {
                if let Some(peer_id) = peer_id {
                    if !self.connected.contains_key(&peer_id) {
                        self.requests.retain(|request| {
                            request
                                != &Request {
                                    peer_id,
                                    protocol: Protocol::Push,
                                }
                        });
                    }
                }

                if let Some(entry) = peer_id.and_then(|id| self.discovered_peers.get_mut(&id)) {
                    if let DialError::Transport(errors) = error {
                        for (addr, _error) in errors {
                            entry.remove(addr);
                        }
                    }
                }
            }
            FromSwarm::NewListenAddr(_) | FromSwarm::ExpiredListenAddr(_) => {
                if self.config.push_listen_addr_updates {
                    for p in self.connected.keys() {
                        let request = Request {
                            peer_id: *p,
                            protocol: Protocol::Push,
                        };
                        if !self.requests.contains(&request) {
                            self.requests.push(request);
                        }
                    }
                }
            }
            FromSwarm::AddressChange(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListener(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddr(_)
            | FromSwarm::ExpiredExternalAddr(_) => {}
        }
    }
}

/// Event emitted  by the `Identify` behaviour.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Event {
    /// Identification information has been received from a peer.
    Received {
        /// The peer that has been identified.
        peer_id: PeerId,
        /// The information provided by the peer.
        info: Info,
    },
    /// Identification information of the local node has been sent to a peer in
    /// response to an identification request.
    Sent {
        /// The peer that the information has been sent to.
        peer_id: PeerId,
    },
    /// Identification information of the local node has been actively pushed to
    /// a peer.
    Pushed {
        /// The peer that the information has been sent to.
        peer_id: PeerId,
    },
    /// Error while attempting to identify the remote.
    Error {
        /// The peer with whom the error originated.
        peer_id: PeerId,
        /// The error that occurred.
        error: ConnectionHandlerUpgrErr<UpgradeError>,
    },
}

fn supported_protocols(params: &impl PollParameters) -> Vec<StreamProtocol> {
    // The protocol names can be bytes, but the identify protocol except UTF-8 strings.
    // There's not much we can do to solve this conflict except strip non-UTF-8 characters.
    params
        .supported_protocols()
        .filter_map(|p| {
            StreamProtocol::try_from_owned(String::from_utf8_lossy(&p).to_string()).ok()
        })
        .collect()
}

/// If there is a given peer_id in the multiaddr, make sure it is the same as
/// the given peer_id. If there is no peer_id for the peer in the mutiaddr, this returns true.
fn multiaddr_matches_peer_id(addr: &Multiaddr, peer_id: &PeerId) -> bool {
    let last_component = addr.iter().last();
    if let Some(multiaddr::Protocol::P2p(multi_addr_peer_id)) = last_component {
        return multi_addr_peer_id == *peer_id.as_ref();
    }
    true
}

struct PeerCache(Option<LruCache<PeerId, HashSet<Multiaddr>>>);

impl PeerCache {
    fn disabled() -> Self {
        Self(None)
    }

    fn enabled(size: NonZeroUsize) -> Self {
        Self(Some(LruCache::new(size)))
    }

    fn get_mut(&mut self, peer: &PeerId) -> Option<&mut HashSet<Multiaddr>> {
        self.0.as_mut()?.get_mut(peer)
    }

    fn put(&mut self, peer: PeerId, addresses: impl Iterator<Item = Multiaddr>) {
        let cache = match self.0.as_mut() {
            None => return,
            Some(cache) => cache,
        };

        cache.put(peer, HashSet::from_iter(addresses));
    }

    fn get(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        let cache = match self.0.as_mut() {
            None => return Vec::new(),
            Some(cache) => cache,
        };

        cache
            .get(peer)
            .cloned()
            .map(Vec::from_iter)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_multiaddr_matches_peer_id() {
        let peer_id = PeerId::random();
        let other_peer_id = PeerId::random();
        let mut addr: Multiaddr = "/ip4/147.75.69.143/tcp/4001"
            .parse()
            .expect("failed to parse multiaddr");

        let addr_without_peer_id: Multiaddr = addr.clone();
        let mut addr_with_other_peer_id = addr.clone();

        addr.push(multiaddr::Protocol::P2p(peer_id.into()));
        addr_with_other_peer_id.push(multiaddr::Protocol::P2p(other_peer_id.into()));

        assert!(multiaddr_matches_peer_id(&addr, &peer_id));
        assert!(!multiaddr_matches_peer_id(
            &addr_with_other_peer_id,
            &peer_id
        ));
        assert!(multiaddr_matches_peer_id(&addr_without_peer_id, &peer_id));
    }
}
