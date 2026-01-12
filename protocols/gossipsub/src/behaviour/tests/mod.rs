// Copyright 2025 Sigma Prime Pty Ltd.
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

//! Test utilities and infrastructure for gossipsub behaviour tests.
//!
//! This module provides shared test utilities used across all gossipsub behaviour test modules.
//! The main components are:
//!
//! - [`BehaviourTestBuilder`]: A builder for creating test network configurations with peers,
//!   topics, and various gossipsub settings.
//! - Peer management functions: [`add_peer`], [`add_peer_with_addr`],
//!   [`add_peer_with_addr_and_kind`], [`disconnect_peer`]
//! - Message utilities: [`proto_to_message`], [`random_message`]
//! - Event helpers: [`count_control_msgs`], [`flush_events`]

mod explicit_peers;
mod floodsub;
mod gossip;
mod graft_prune;
mod idontwant;
mod mesh;
mod peer_queues;
mod publish;
mod scoring;
mod subscription;
mod topic_config;

use std::collections::HashMap;

use byteorder::{BigEndian, ByteOrder};
use hashlink::LinkedHashMap;
use libp2p_core::ConnectedPoint;
use rand::Rng;

use super::*;
use crate::{types::RpcIn, IdentTopic as Topic};

/// Convenience alias for [`BehaviourTestBuilder`] with default transform and subscription filter.
pub(super) type DefaultBehaviourTestBuilder =
    BehaviourTestBuilder<IdentityTransform, AllowAllSubscriptionFilter>;

/// A builder for creating test gossipsub networks with configurable peers and topics.
///
/// This struct uses the builder pattern to configure a test network setup.
/// Call [`create_network`](Self::create_network) to finalize and create the network.
///
/// # Example
///
/// ```ignore
/// let (gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
///     .peer_no(20)
///     .topics(vec!["topic1".into()])
///     .to_subscribe(true)
///     .gs_config(Config::default())
///     .create_network();
/// ```
#[derive(Default, Debug)]
pub(super) struct BehaviourTestBuilder<D, F> {
    peer_no: usize,
    topics: Vec<String>,
    to_subscribe: bool,
    gs_config: Config,
    explicit: usize,
    outbound: usize,
    scoring: Option<(PeerScoreParams, PeerScoreThresholds)>,
    data_transform: D,
    subscription_filter: F,
    peer_kind: Option<PeerKind>,
}

impl<D, F> BehaviourTestBuilder<D, F>
where
    D: DataTransform + Default + Clone + Send + 'static,
    F: TopicSubscriptionFilter + Clone + Default + Send + 'static,
{
    /// Creates the test network with the configured settings.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `Behaviour<D, F>`: The gossipsub behaviour instance
    /// - `Vec<PeerId>`: List of connected peer IDs
    /// - `HashMap<PeerId, Queue>`: Message queues for each peer (for inspecting sent messages)
    /// - `Vec<TopicHash>`: List of subscribed topic hashes
    #[allow(clippy::type_complexity)]
    pub(super) fn create_network(
        self,
    ) -> (
        Behaviour<D, F>,
        Vec<PeerId>,
        HashMap<PeerId, Queue>,
        Vec<TopicHash>,
    ) {
        let keypair = libp2p_identity::Keypair::generate_ed25519();
        // create a gossipsub struct
        let mut gs: Behaviour<D, F> = Behaviour::new_with_subscription_filter_and_transform(
            MessageAuthenticity::Signed(keypair),
            self.gs_config,
            self.subscription_filter,
            self.data_transform,
        )
        .unwrap();

        if let Some((scoring_params, scoring_thresholds)) = self.scoring {
            gs.with_peer_score(scoring_params, scoring_thresholds)
                .unwrap();
        }

        let mut topic_hashes = vec![];

        // subscribe to the topics
        for t in self.topics {
            let topic = Topic::new(t);
            gs.subscribe(&topic).unwrap();
            topic_hashes.push(topic.hash().clone());
        }

        // build and connect peer_no random peers
        let mut peers = vec![];
        let mut queues = HashMap::new();

        let empty = vec![];
        for i in 0..self.peer_no {
            let (peer, queue) = add_peer_with_addr_and_kind(
                &mut gs,
                if self.to_subscribe {
                    &topic_hashes
                } else {
                    &empty
                },
                i < self.outbound,
                i < self.explicit,
                Multiaddr::empty(),
                self.peer_kind.or(Some(PeerKind::Gossipsubv1_1)),
            );
            peers.push(peer);
            queues.insert(peer, queue);
        }

        (gs, peers, queues, topic_hashes)
    }

    /// Sets the number of peers to create in the test network.
    pub(super) fn peer_no(mut self, peer_no: usize) -> Self {
        self.peer_no = peer_no;
        self
    }

    /// Sets the topics that the local node will subscribe to.
    pub(super) fn topics(mut self, topics: Vec<String>) -> Self {
        self.topics = topics;
        self
    }

    /// If `true`, peers will also subscribe to the configured topics.
    /// If `false`, peers are connected but not subscribed.
    #[allow(clippy::wrong_self_convention)]
    pub(super) fn to_subscribe(mut self, to_subscribe: bool) -> Self {
        self.to_subscribe = to_subscribe;
        self
    }

    /// Sets a custom gossipsub configuration.
    pub(super) fn gs_config(mut self, gs_config: Config) -> Self {
        self.gs_config = gs_config;
        self
    }

    /// Sets how many of the first N peers should be marked as explicit peers.
    pub(super) fn explicit(mut self, explicit: usize) -> Self {
        self.explicit = explicit;
        self
    }

    /// Sets how many of the first N peers should be outbound connections.
    /// Outbound peers are dialed by us; inbound peers connected to us.
    pub(super) fn outbound(mut self, outbound: usize) -> Self {
        self.outbound = outbound;
        self
    }

    /// Enables peer scoring with the given parameters and thresholds.
    pub(super) fn scoring(
        mut self,
        scoring: Option<(PeerScoreParams, PeerScoreThresholds)>,
    ) -> Self {
        self.scoring = scoring;
        self
    }

    /// Sets a custom subscription filter.
    pub(super) fn subscription_filter(mut self, subscription_filter: F) -> Self {
        self.subscription_filter = subscription_filter;
        self
    }

    /// Sets the protocol version for all created peers (e.g., Gossipsubv1_1, Gossipsubv1_2).
    pub(super) fn peer_kind(mut self, peer_kind: PeerKind) -> Self {
        self.peer_kind = Some(peer_kind);
        self
    }
}

/// Adds a new peer to the gossipsub network with default settings.
///
/// This is a convenience wrapper around [`add_peer_with_addr_and_kind`] that uses
/// an empty multiaddr and Gossipsubv1_1 protocol.
///
/// # Arguments
///
/// * `gs` - The gossipsub behaviour to add the peer to
/// * `topic_hashes` - Topics the peer will be subscribed to
/// * `outbound` - If `true`, simulates an outbound connection (we dialed them)
/// * `explicit` - If `true`, adds the peer as an explicit peer
///
/// # Returns
///
/// A tuple of the peer's ID and their message queue (for inspecting sent messages).
pub(super) fn add_peer<D, F>(
    gs: &mut Behaviour<D, F>,
    topic_hashes: &[TopicHash],
    outbound: bool,
    explicit: bool,
) -> (PeerId, Queue)
where
    D: DataTransform + Default + Clone + Send + 'static,
    F: TopicSubscriptionFilter + Clone + Default + Send + 'static,
{
    add_peer_with_addr(gs, topic_hashes, outbound, explicit, Multiaddr::empty())
}

/// Adds a new peer with a specific multiaddr.
///
/// See [`add_peer`] for basic usage. Use this variant when testing
/// address-specific behaviour (e.g., IP colocation scoring).
pub(super) fn add_peer_with_addr<D, F>(
    gs: &mut Behaviour<D, F>,
    topic_hashes: &[TopicHash],
    outbound: bool,
    explicit: bool,
    address: Multiaddr,
) -> (PeerId, Queue)
where
    D: DataTransform + Default + Clone + Send + 'static,
    F: TopicSubscriptionFilter + Clone + Default + Send + 'static,
{
    add_peer_with_addr_and_kind(
        gs,
        topic_hashes,
        outbound,
        explicit,
        address,
        Some(PeerKind::Gossipsubv1_1),
    )
}

/// Adds a new peer with full configuration options.
///
/// This is the most flexible peer creation function, allowing control over
/// the peer's address and protocol version.
///
/// # Arguments
///
/// * `gs` - The gossipsub behaviour to add the peer to
/// * `topic_hashes` - Topics the peer will be subscribed to
/// * `outbound` - If `true`, simulates an outbound connection (we dialed them)
/// * `explicit` - If `true`, adds the peer as an explicit peer
/// * `address` - The multiaddr for the peer connection
/// * `kind` - The gossipsub protocol version. Use `None` for Floodsub peers, or
///   `Some(PeerKind::...)` for specific versions.
///
/// # Returns
///
/// A tuple of the peer's ID and their message queue (for inspecting sent messages).
pub(super) fn add_peer_with_addr_and_kind<D, F>(
    gs: &mut Behaviour<D, F>,
    topic_hashes: &[TopicHash],
    outbound: bool,
    explicit: bool,
    address: Multiaddr,
    kind: Option<PeerKind>,
) -> (PeerId, Queue)
where
    D: DataTransform + Default + Clone + Send + 'static,
    F: TopicSubscriptionFilter + Clone + Default + Send + 'static,
{
    let peer = PeerId::random();
    let endpoint = if outbound {
        ConnectedPoint::Dialer {
            address,
            role_override: Endpoint::Dialer,
            port_use: PortUse::Reuse,
        }
    } else {
        ConnectedPoint::Listener {
            local_addr: Multiaddr::empty(),
            send_back_addr: address,
        }
    };

    let queue = Queue::new(gs.config.connection_handler_queue_len());
    let receiver_queue = queue.clone();
    let connection_id = ConnectionId::new_unchecked(0);
    gs.connected_peers.insert(
        peer,
        PeerDetails {
            kind: kind.unwrap_or(PeerKind::Floodsub),
            outbound,
            connections: vec![connection_id],
            topics: Default::default(),
            messages: queue,
            dont_send: LinkedHashMap::new(),
        },
    );

    gs.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
        peer_id: peer,
        connection_id,
        endpoint: &endpoint,
        failed_addresses: &[],
        other_established: 0, // first connection
    }));
    if let Some(kind) = kind {
        gs.on_connection_handler_event(
            peer,
            ConnectionId::new_unchecked(0),
            HandlerEvent::PeerKind(kind),
        );
    }
    if explicit {
        gs.add_explicit_peer(&peer);
    }
    if !topic_hashes.is_empty() {
        gs.handle_received_subscriptions(
            &topic_hashes
                .iter()
                .cloned()
                .map(|t| Subscription {
                    action: SubscriptionAction::Subscribe,
                    topic_hash: t,
                })
                .collect::<Vec<_>>(),
            &peer,
        );
    }
    (peer, receiver_queue)
}

/// Simulates disconnecting a peer from the gossipsub network.
///
/// This triggers the `ConnectionClosed` event for all of the peer's connections,
/// properly cleaning up mesh membership and other peer state.
pub(super) fn disconnect_peer<D, F>(gs: &mut Behaviour<D, F>, peer_id: &PeerId)
where
    D: DataTransform + Default + Clone + Send + 'static,
    F: TopicSubscriptionFilter + Clone + Default + Send + 'static,
{
    if let Some(peer_connections) = gs.connected_peers.get(peer_id) {
        let fake_endpoint = ConnectedPoint::Dialer {
            address: Multiaddr::empty(),
            role_override: Endpoint::Dialer,
            port_use: PortUse::Reuse,
        }; // this is not relevant
           // peer_connections.connections should never be empty.

        let mut active_connections = peer_connections.connections.len();
        for connection_id in peer_connections.connections.clone() {
            active_connections = active_connections.checked_sub(1).unwrap();

            gs.on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id: *peer_id,
                connection_id,
                endpoint: &fake_endpoint,
                remaining_established: active_connections,
                cause: None,
            }));
        }
    }
}

/// Converts a raw protobuf RPC message into a gossipsub [`RpcIn`] structure.
///
/// This is useful for simulating incoming RPC messages from peers in tests.
/// It parses all message types: publish messages, subscriptions, and control
/// messages (IHAVE, IWANT, GRAFT, PRUNE).
pub(super) fn proto_to_message(rpc: &proto::RPC) -> RpcIn {
    // Store valid messages.
    let mut messages = Vec::with_capacity(rpc.publish.len());
    let rpc = rpc.clone();
    for message in rpc.publish.into_iter() {
        messages.push(RawMessage {
            source: message.from.map(|x| PeerId::from_bytes(&x).unwrap()),
            data: message.data.unwrap_or_default(),
            sequence_number: message.seqno.map(|x| BigEndian::read_u64(&x)),
            topic: TopicHash::from_raw(message.topic),
            signature: message.signature,
            key: None,
            validated: false,
        });
    }
    let mut control_msgs = Vec::new();
    if let Some(rpc_control) = rpc.control {
        // Collect the gossipsub control messages
        let ihave_msgs: Vec<ControlAction> = rpc_control
            .ihave
            .into_iter()
            .map(|ihave| {
                ControlAction::IHave(IHave {
                    topic_hash: TopicHash::from_raw(ihave.topic_id.unwrap_or_default()),
                    message_ids: ihave
                        .message_ids
                        .into_iter()
                        .map(MessageId::from)
                        .collect::<Vec<_>>(),
                })
            })
            .collect();

        let iwant_msgs: Vec<ControlAction> = rpc_control
            .iwant
            .into_iter()
            .map(|iwant| {
                ControlAction::IWant(IWant {
                    message_ids: iwant
                        .message_ids
                        .into_iter()
                        .map(MessageId::from)
                        .collect::<Vec<_>>(),
                })
            })
            .collect();

        let graft_msgs: Vec<ControlAction> = rpc_control
            .graft
            .into_iter()
            .map(|graft| {
                ControlAction::Graft(Graft {
                    topic_hash: TopicHash::from_raw(graft.topic_id.unwrap_or_default()),
                })
            })
            .collect();

        let mut prune_msgs = Vec::new();

        for prune in rpc_control.prune {
            // filter out invalid peers
            let peers = prune
                .peers
                .into_iter()
                .filter_map(|info| {
                    info.peer_id
                        .and_then(|id| PeerId::from_bytes(&id).ok())
                        .map(|peer_id|
                            //TODO signedPeerRecord, see https://github.com/libp2p/specs/pull/217
                            PeerInfo {
                                peer_id: Some(peer_id),
                            })
                })
                .collect::<Vec<PeerInfo>>();

            let topic_hash = TopicHash::from_raw(prune.topic_id.unwrap_or_default());
            prune_msgs.push(ControlAction::Prune(Prune {
                topic_hash,
                peers,
                backoff: prune.backoff,
            }));
        }

        control_msgs.extend(ihave_msgs);
        control_msgs.extend(iwant_msgs);
        control_msgs.extend(graft_msgs);
        control_msgs.extend(prune_msgs);
    }

    RpcIn {
        messages,
        subscriptions: rpc
            .subscriptions
            .into_iter()
            .map(|sub| Subscription {
                action: if Some(true) == sub.subscribe {
                    SubscriptionAction::Subscribe
                } else {
                    SubscriptionAction::Unsubscribe
                },
                topic_hash: TopicHash::from_raw(sub.topic_id.unwrap_or_default()),
            })
            .collect(),
        control_msgs,
    }
}

impl Behaviour {
    /// Returns a mutable reference to the peer scoring state.
    ///
    /// # Panics
    ///
    /// Panics if peer scoring is not enabled on this behaviour instance.
    /// Ensure scoring is configured via [`BehaviourTestBuilder::scoring`] before calling.
    pub(super) fn as_peer_score_mut(&mut self) -> &mut PeerScore {
        match self.peer_score {
            PeerScoreState::Active(ref mut peer_score) => peer_score,
            PeerScoreState::Disabled => panic!("PeerScore is deactivated"),
        }
    }
}

/// Counts RPC messages across all peer queues that match a filter predicate.
///
/// This drains all messages from the queues, counting those that match the filter.
/// Returns the count and the (now empty) queues for potential reuse.
///
/// # Arguments
///
/// * `queues` - Map of peer IDs to their message queues
/// * `filter` - Predicate that receives (peer_id, rpc_message) and returns true to count
///
/// # Returns
///
/// A tuple of (count of matching messages, emptied queues map).
///
/// # Example
///
/// ```ignore
/// let (prune_count, queues) = count_control_msgs(queues, |peer_id, msg| {
///     matches!(msg, RpcOut::Prune { .. })
/// });
/// ```
pub(super) fn count_control_msgs(
    queues: HashMap<PeerId, Queue>,
    mut filter: impl FnMut(&PeerId, &RpcOut) -> bool,
) -> (usize, HashMap<PeerId, Queue>) {
    let mut new_queues = HashMap::new();
    let mut collected_messages = 0;
    for (peer_id, mut queue) in queues.into_iter() {
        while !queue.is_empty() {
            if let Some(rpc) = queue.try_pop() {
                if filter(&peer_id, &rpc) {
                    collected_messages += 1;
                }
            }
        }
        new_queues.insert(peer_id, queue);
    }
    (collected_messages, new_queues)
}

/// Clears all pending events from the behaviour and drains all peer message queues.
///
/// Use this to reset state between test phases when you want to ignore
/// messages/events generated by setup operations.
///
/// # Returns
///
/// The emptied queues map for continued use.
pub(super) fn flush_events<D: DataTransform, F: TopicSubscriptionFilter>(
    gs: &mut Behaviour<D, F>,
    queues: HashMap<PeerId, Queue>,
) -> HashMap<PeerId, Queue> {
    gs.events.clear();
    let mut new_queues = HashMap::new();
    for (peer_id, mut queue) in queues.into_iter() {
        while !queue.is_empty() {
            let _ = queue.try_pop();
        }
        new_queues.insert(peer_id, queue);
    }
    new_queues
}

/// Generates a random gossipsub message for testing.
///
/// Creates a message with:
/// - Random source peer ID
/// - Random data (10-10024 bytes)
/// - Sequential sequence number (using the provided counter)
/// - Random topic from the provided list
/// - Pre-validated status
///
/// # Arguments
///
/// * `seq` - Mutable sequence counter (incremented each call)
/// * `topics` - Pool of topics to randomly select from
pub(super) fn random_message(seq: &mut u64, topics: &[TopicHash]) -> RawMessage {
    let mut rng = rand::thread_rng();
    *seq += 1;
    RawMessage {
        source: Some(PeerId::random()),
        data: (0..rng.gen_range(10..10024)).map(|_| rng.gen()).collect(),
        sequence_number: Some(*seq),
        topic: topics[rng.gen_range(0..topics.len())].clone(),
        signature: None,
        key: None,
        validated: true,
    }
}
