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

use crate::handler::{IdentifyHandler, IdentifyHandlerEvent, IdentifyPush};
use crate::protocol::{IdentifyInfo, ReplySubstream, UpgradeError};
use futures::prelude::*;
use libp2p_core::{
    connection::{ConnectionId, ListenerId},
    multiaddr::Protocol,
    ConnectedPoint, Multiaddr, PeerId, PublicKey,
};
use libp2p_swarm::{
    dial_opts::{self, DialOpts},
    AddressScore, ConnectionHandler, ConnectionHandlerUpgrErr, DialError, IntoConnectionHandler,
    NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters,
};
use lru::LruCache;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter::FromIterator,
    pin::Pin,
    task::Context,
    task::Poll,
    time::Duration,
};

/// Network behaviour that automatically identifies nodes periodically, returns information
/// about them, and answers identify queries from other nodes.
///
/// All external addresses of the local node supposedly observed by remotes
/// are reported via [`NetworkBehaviourAction::ReportObservedAddr`] with a
/// [score](AddressScore) of `1`.
pub struct Identify {
    config: IdentifyConfig,
    /// For each peer we're connected to, the observed address to send back to it.
    connected: HashMap<PeerId, HashMap<ConnectionId, Multiaddr>>,
    /// Pending replies to send.
    pending_replies: VecDeque<Reply>,
    /// Pending events to be emitted when polled.
    events: VecDeque<NetworkBehaviourAction<IdentifyEvent, IdentifyHandler>>,
    /// Peers to which an active push with current information about
    /// the local peer should be sent.
    pending_push: HashSet<PeerId>,
    /// The addresses of all peers that we have discovered.
    discovered_peers: LruCache<PeerId, HashSet<Multiaddr>>,
}

/// A pending reply to an inbound identification request.
enum Reply {
    /// The reply is queued for sending.
    Queued {
        peer: PeerId,
        io: ReplySubstream<NegotiatedSubstream>,
        observed: Multiaddr,
    },
    /// The reply is being sent.
    Sending {
        peer: PeerId,
        io: Pin<Box<dyn Future<Output = Result<(), UpgradeError>> + Send>>,
    },
}

/// Configuration for the [`Identify`] [`NetworkBehaviour`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct IdentifyConfig {
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
    /// Defaults to 500ms.
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

impl IdentifyConfig {
    /// Creates a new configuration for the `Identify` behaviour that
    /// advertises the given protocol version and public key.
    pub fn new(protocol_version: String, local_public_key: PublicKey) -> Self {
        IdentifyConfig {
            protocol_version,
            agent_version: format!("rust-libp2p/{}", env!("CARGO_PKG_VERSION")),
            local_public_key,
            initial_delay: Duration::from_millis(500),
            interval: Duration::from_secs(5 * 60),
            push_listen_addr_updates: false,
            cache_size: 0,
        }
    }

    /// Configures the agent version sent to peers.
    pub fn with_agent_version(mut self, v: String) -> Self {
        self.agent_version = v;
        self
    }

    /// Configures the initial delay before the first identification
    /// request is sent on a newly established connection to a peer.
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
    ///
    /// The [`Swarm`](libp2p_swarm::Swarm) may extend the set of addresses of an outgoing connection attempt via
    ///  [`Identify::addresses_of_peer`].
    pub fn with_cache_size(mut self, cache_size: usize) -> Self {
        self.cache_size = cache_size;
        self
    }
}

impl Identify {
    /// Creates a new `Identify` network behaviour.
    pub fn new(config: IdentifyConfig) -> Self {
        let discovered_peers = LruCache::new(config.cache_size);

        Identify {
            config,
            connected: HashMap::new(),
            pending_replies: VecDeque::new(),
            events: VecDeque::new(),
            pending_push: HashSet::new(),
            discovered_peers,
        }
    }

    /// Initiates an active push of the local peer information to the given peers.
    pub fn push<I>(&mut self, peers: I)
    where
        I: IntoIterator<Item = PeerId>,
    {
        for p in peers {
            if self.pending_push.insert(p) && !self.connected.contains_key(&p) {
                let handler = self.new_handler();
                self.events.push_back(NetworkBehaviourAction::Dial {
                    opts: DialOpts::peer_id(p)
                        .condition(dial_opts::PeerCondition::Disconnected)
                        .build(),
                    handler,
                });
            }
        }
    }
}

impl NetworkBehaviour for Identify {
    type ConnectionHandler = IdentifyHandler;
    type OutEvent = IdentifyEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        IdentifyHandler::new(self.config.initial_delay, self.config.interval)
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
        failed_addresses: Option<&Vec<Multiaddr>>,
        _other_established: usize,
    ) {
        let addr = match endpoint {
            ConnectedPoint::Dialer { address, .. } => address.clone(),
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr.clone(),
        };

        self.connected
            .entry(*peer_id)
            .or_default()
            .insert(*conn, addr);

        if let Some(entry) = self.discovered_peers.get_mut(peer_id) {
            for addr in failed_addresses
                .into_iter()
                .flat_map(|addresses| addresses.iter())
            {
                entry.remove(addr);
            }
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        conn: &ConnectionId,
        _: &ConnectedPoint,
        _: <Self::ConnectionHandler as IntoConnectionHandler>::Handler,
        remaining_established: usize,
    ) {
        if remaining_established == 0 {
            self.connected.remove(peer_id);
            self.pending_push.remove(peer_id);
        } else if let Some(addrs) = self.connected.get_mut(peer_id) {
            addrs.remove(conn);
        }
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        _: Self::ConnectionHandler,
        error: &DialError,
    ) {
        if let Some(peer_id) = peer_id {
            if !self.connected.contains_key(&peer_id) {
                self.pending_push.remove(&peer_id);
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

    fn inject_new_listen_addr(&mut self, _id: ListenerId, _addr: &Multiaddr) {
        if self.config.push_listen_addr_updates {
            self.pending_push.extend(self.connected.keys());
        }
    }

    fn inject_expired_listen_addr(&mut self, _id: ListenerId, _addr: &Multiaddr) {
        if self.config.push_listen_addr_updates {
            self.pending_push.extend(self.connected.keys());
        }
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection: ConnectionId,
        event: <Self::ConnectionHandler as ConnectionHandler>::OutEvent,
    ) {
        match event {
            IdentifyHandlerEvent::Identified(mut info) => {
                // Remove invalid multiaddrs.
                info.listen_addrs
                    .retain(|addr| multiaddr_matches_peer_id(addr, &peer_id));

                // Replace existing addresses to prevent other peer from filling up our memory.
                self.discovered_peers
                    .put(peer_id, HashSet::from_iter(info.listen_addrs.clone()));

                let observed = info.observed_addr.clone();
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    IdentifyEvent::Received { peer_id, info },
                ));
                self.events
                    .push_back(NetworkBehaviourAction::ReportObservedAddr {
                        address: observed,
                        score: AddressScore::Finite(1),
                    });
            }
            IdentifyHandlerEvent::IdentificationPushed => {
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    IdentifyEvent::Pushed { peer_id },
                ));
            }
            IdentifyHandlerEvent::Identify(sender) => {
                let observed = self
                    .connected
                    .get(&peer_id)
                    .and_then(|addrs| addrs.get(&connection))
                    .expect(
                        "`inject_event` is only called with an established connection \
                             and `inject_connection_established` ensures there is an entry; qed",
                    );
                self.pending_replies.push_back(Reply::Queued {
                    peer: peer_id,
                    io: sender,
                    observed: observed.clone(),
                });
            }
            IdentifyHandlerEvent::IdentificationError(error) => {
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    IdentifyEvent::Error { peer_id, error },
                ));
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        // Check for a pending active push to perform.
        let peer_push = self.pending_push.iter().find_map(|peer| {
            self.connected.get(peer).map(|conns| {
                let observed_addr = conns
                    .values()
                    .next()
                    .expect("connected peer has a connection")
                    .clone();

                let listen_addrs = listen_addrs(params);
                let protocols = supported_protocols(params);

                let info = IdentifyInfo {
                    public_key: self.config.local_public_key.clone(),
                    protocol_version: self.config.protocol_version.clone(),
                    agent_version: self.config.agent_version.clone(),
                    listen_addrs,
                    protocols,
                    observed_addr,
                };

                (*peer, IdentifyPush(info))
            })
        });

        if let Some((peer_id, push)) = peer_push {
            self.pending_push.remove(&peer_id);
            return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                peer_id,
                event: push,
                handler: NotifyHandler::Any,
            });
        }

        // Check for pending replies to send.
        if let Some(r) = self.pending_replies.pop_front() {
            let mut sending = 0;
            let to_send = self.pending_replies.len() + 1;
            let mut reply = Some(r);
            loop {
                match reply {
                    Some(Reply::Queued { peer, io, observed }) => {
                        let info = IdentifyInfo {
                            listen_addrs: listen_addrs(params),
                            protocols: supported_protocols(params),
                            public_key: self.config.local_public_key.clone(),
                            protocol_version: self.config.protocol_version.clone(),
                            agent_version: self.config.agent_version.clone(),
                            observed_addr: observed,
                        };
                        let io = Box::pin(io.send(info));
                        reply = Some(Reply::Sending { peer, io });
                    }
                    Some(Reply::Sending { peer, mut io }) => {
                        sending += 1;
                        match Future::poll(Pin::new(&mut io), cx) {
                            Poll::Ready(Ok(())) => {
                                let event = IdentifyEvent::Sent { peer_id: peer };
                                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
                            }
                            Poll::Pending => {
                                self.pending_replies.push_back(Reply::Sending { peer, io });
                                if sending == to_send {
                                    // All remaining futures are NotReady
                                    break;
                                } else {
                                    reply = self.pending_replies.pop_front();
                                }
                            }
                            Poll::Ready(Err(err)) => {
                                let event = IdentifyEvent::Error {
                                    peer_id: peer,
                                    error: ConnectionHandlerUpgrErr::Upgrade(
                                        libp2p_core::upgrade::UpgradeError::Apply(err),
                                    ),
                                };
                                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
                            }
                        }
                    }
                    None => unreachable!(),
                }
            }
        }

        Poll::Pending
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.discovered_peers
            .get(peer)
            .cloned()
            .map(Vec::from_iter)
            .unwrap_or_default()
    }
}

/// Event emitted  by the `Identify` behaviour.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum IdentifyEvent {
    /// Identification information has been received from a peer.
    Received {
        /// The peer that has been identified.
        peer_id: PeerId,
        /// The information provided by the peer.
        info: IdentifyInfo,
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

fn supported_protocols(params: &impl PollParameters) -> Vec<String> {
    // The protocol names can be bytes, but the identify protocol except UTF-8 strings.
    // There's not much we can do to solve this conflict except strip non-UTF-8 characters.
    params
        .supported_protocols()
        .map(|p| String::from_utf8_lossy(&p).to_string())
        .collect()
}

fn listen_addrs(params: &impl PollParameters) -> Vec<Multiaddr> {
    let mut listen_addrs: Vec<_> = params.external_addresses().map(|r| r.addr).collect();
    listen_addrs.extend(params.listened_addresses());
    listen_addrs
}

/// If there is a given peer_id in the multiaddr, make sure it is the same as
/// the given peer_id. If there is no peer_id for the peer in the mutiaddr, this returns true.
fn multiaddr_matches_peer_id(addr: &Multiaddr, peer_id: &PeerId) -> bool {
    let last_component = addr.iter().last();
    if let Some(Protocol::P2p(multi_addr_peer_id)) = last_component {
        return multi_addr_peer_id == *peer_id.as_ref();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::pin_mut;
    use libp2p_core::{identity, muxing::StreamMuxerBox, transport, upgrade, PeerId, Transport};
    use libp2p_mplex::MplexConfig;
    use libp2p_noise as noise;
    use libp2p_swarm::{Swarm, SwarmEvent};
    use libp2p_tcp::TcpConfig;

    fn transport() -> (
        identity::PublicKey,
        transport::Boxed<(PeerId, StreamMuxerBox)>,
    ) {
        let id_keys = identity::Keypair::generate_ed25519();
        let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
            .into_authentic(&id_keys)
            .unwrap();
        let pubkey = id_keys.public();
        let transport = TcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
            .multiplex(MplexConfig::new())
            .boxed();
        (pubkey, transport)
    }

    #[test]
    fn periodic_identify() {
        let (mut swarm1, pubkey1) = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new(
                IdentifyConfig::new("a".to_string(), pubkey.clone())
                    .with_agent_version("b".to_string()),
            );
            let swarm = Swarm::new(transport, protocol, pubkey.to_peer_id());
            (swarm, pubkey)
        };

        let (mut swarm2, pubkey2) = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new(
                IdentifyConfig::new("c".to_string(), pubkey.clone())
                    .with_agent_version("d".to_string()),
            );
            let swarm = Swarm::new(transport, protocol, pubkey.to_peer_id());
            (swarm, pubkey)
        };

        swarm1
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let listen_addr = async_std::task::block_on(async {
            loop {
                let swarm1_fut = swarm1.select_next_some();
                pin_mut!(swarm1_fut);
                match swarm1_fut.await {
                    SwarmEvent::NewListenAddr { address, .. } => return address,
                    _ => {}
                }
            }
        });
        swarm2.dial(listen_addr).unwrap();

        // nb. Either swarm may receive the `Identified` event first, upon which
        // it will permit the connection to be closed, as defined by
        // `IdentifyHandler::connection_keep_alive`. Hence the test succeeds if
        // either `Identified` event arrives correctly.
        async_std::task::block_on(async move {
            loop {
                let swarm1_fut = swarm1.select_next_some();
                pin_mut!(swarm1_fut);
                let swarm2_fut = swarm2.select_next_some();
                pin_mut!(swarm2_fut);

                match future::select(swarm1_fut, swarm2_fut)
                    .await
                    .factor_second()
                    .0
                {
                    future::Either::Left(SwarmEvent::Behaviour(IdentifyEvent::Received {
                        info,
                        ..
                    })) => {
                        assert_eq!(info.public_key, pubkey2);
                        assert_eq!(info.protocol_version, "c");
                        assert_eq!(info.agent_version, "d");
                        assert!(!info.protocols.is_empty());
                        assert!(info.listen_addrs.is_empty());
                        return;
                    }
                    future::Either::Right(SwarmEvent::Behaviour(IdentifyEvent::Received {
                        info,
                        ..
                    })) => {
                        assert_eq!(info.public_key, pubkey1);
                        assert_eq!(info.protocol_version, "a");
                        assert_eq!(info.agent_version, "b");
                        assert!(!info.protocols.is_empty());
                        assert_eq!(info.listen_addrs.len(), 1);
                        return;
                    }
                    _ => {}
                }
            }
        })
    }

    #[test]
    fn identify_push() {
        let _ = env_logger::try_init();

        let (mut swarm1, pubkey1) = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new(IdentifyConfig::new("a".to_string(), pubkey.clone()));
            let swarm = Swarm::new(transport, protocol, pubkey.to_peer_id());
            (swarm, pubkey)
        };

        let (mut swarm2, pubkey2) = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new(
                IdentifyConfig::new("a".to_string(), pubkey.clone())
                    .with_agent_version("b".to_string()),
            );
            let swarm = Swarm::new(transport, protocol, pubkey.to_peer_id());
            (swarm, pubkey)
        };

        Swarm::listen_on(&mut swarm1, "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

        let listen_addr = async_std::task::block_on(async {
            loop {
                let swarm1_fut = swarm1.select_next_some();
                pin_mut!(swarm1_fut);
                match swarm1_fut.await {
                    SwarmEvent::NewListenAddr { address, .. } => return address,
                    _ => {}
                }
            }
        });

        swarm2.dial(listen_addr).unwrap();

        async_std::task::block_on(async move {
            loop {
                let swarm1_fut = swarm1.select_next_some();
                let swarm2_fut = swarm2.select_next_some();

                {
                    pin_mut!(swarm1_fut);
                    pin_mut!(swarm2_fut);
                    match future::select(swarm1_fut, swarm2_fut)
                        .await
                        .factor_second()
                        .0
                    {
                        future::Either::Left(SwarmEvent::Behaviour(IdentifyEvent::Received {
                            info,
                            ..
                        })) => {
                            assert_eq!(info.public_key, pubkey2);
                            assert_eq!(info.protocol_version, "a");
                            assert_eq!(info.agent_version, "b");
                            assert!(!info.protocols.is_empty());
                            assert!(info.listen_addrs.is_empty());
                            return;
                        }
                        future::Either::Right(SwarmEvent::ConnectionEstablished { .. }) => {
                            // Once a connection is established, we can initiate an
                            // active push below.
                        }
                        _ => continue,
                    }
                }

                swarm2
                    .behaviour_mut()
                    .push(std::iter::once(pubkey1.to_peer_id()));
            }
        })
    }

    #[test]
    fn discover_peer_after_disconnect() {
        let _ = env_logger::try_init();

        let mut swarm1 = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new(IdentifyConfig::new("a".to_string(), pubkey.clone()));

            Swarm::new(transport, protocol, pubkey.to_peer_id())
        };

        let mut swarm2 = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new(
                IdentifyConfig::new("a".to_string(), pubkey.clone())
                    .with_cache_size(100)
                    .with_agent_version("b".to_string()),
            );

            Swarm::new(transport, protocol, pubkey.to_peer_id())
        };

        let swarm1_peer_id = *swarm1.local_peer_id();

        let listener = swarm1
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let listen_addr = async_std::task::block_on(async {
            loop {
                match swarm1.select_next_some().await {
                    SwarmEvent::NewListenAddr {
                        address,
                        listener_id,
                    } if listener_id == listener => return address,
                    _ => {}
                }
            }
        });

        async_std::task::spawn(async move {
            loop {
                swarm1.next().await;
            }
        });

        swarm2.dial(listen_addr).unwrap();

        // Wait until we identified.
        async_std::task::block_on(async {
            loop {
                if let SwarmEvent::Behaviour(IdentifyEvent::Received { .. }) =
                    swarm2.select_next_some().await
                {
                    break;
                }
            }
        });

        swarm2.disconnect_peer_id(swarm1_peer_id).unwrap();

        // Wait for connection to close.
        async_std::task::block_on(async {
            loop {
                if let SwarmEvent::ConnectionClosed { peer_id, .. } =
                    swarm2.select_next_some().await
                {
                    break peer_id;
                }
            }
        });

        // We should still be able to dial now!
        swarm2.dial(swarm1_peer_id).unwrap();

        let connected_peer = async_std::task::block_on(async {
            loop {
                if let SwarmEvent::ConnectionEstablished { peer_id, .. } =
                    swarm2.select_next_some().await
                {
                    break peer_id;
                }
            }
        });

        assert_eq!(connected_peer, swarm1_peer_id);
    }

    #[test]
    fn check_multiaddr_matches_peer_id() {
        let peer_id = PeerId::random();
        let other_peer_id = PeerId::random();
        let mut addr: Multiaddr = "/ip4/147.75.69.143/tcp/4001"
            .parse()
            .expect("failed to parse multiaddr");

        let addr_without_peer_id: Multiaddr = addr.clone();
        let mut addr_with_other_peer_id = addr.clone();

        addr.push(Protocol::P2p(peer_id.clone().into()));
        addr_with_other_peer_id.push(Protocol::P2p(other_peer_id.into()));

        assert!(multiaddr_matches_peer_id(&addr, &peer_id));
        assert!(!multiaddr_matches_peer_id(
            &addr_with_other_peer_id,
            &peer_id
        ));
        assert!(multiaddr_matches_peer_id(&addr_without_peer_id, &peer_id));
    }
}
