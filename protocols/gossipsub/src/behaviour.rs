// Copyright 2020 Sigma Prime Pty Ltd.
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

use std::{
    cmp::{max, Ordering, Ordering::Equal},
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fmt,
    fmt::Debug,
    net::IpAddr,
    task::{Context, Poll},
    time::Duration,
};

use futures::FutureExt;
use futures_timer::Delay;
use libp2p_core::{
    multiaddr::Protocol::{Ip4, Ip6},
    transport::PortUse,
    Endpoint, Multiaddr,
};
use libp2p_identity::{Keypair, PeerId};
use libp2p_swarm::{
    behaviour::{AddressChange, ConnectionClosed, ConnectionEstablished, FromSwarm},
    dial_opts::DialOpts,
    ConnectionDenied, ConnectionId, NetworkBehaviour, NotifyHandler, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use prometheus_client::registry::Registry;
use quick_protobuf::{MessageWrite, Writer};
use rand::{seq::SliceRandom, thread_rng};
use web_time::{Instant, SystemTime};

use crate::{
    backoff::BackoffStorage,
    config::{Config, ValidationMode},
    gossip_promises::GossipPromises,
    handler::{Handler, HandlerEvent, HandlerIn},
    mcache::MessageCache,
    metrics::{Churn, Config as MetricsConfig, Inclusion, Metrics, Penalty},
    peer_score::{PeerScore, PeerScoreParams, PeerScoreThresholds, RejectReason},
    protocol::SIGNING_PREFIX,
    rpc::Sender,
    rpc_proto::proto,
    subscription_filter::{AllowAllSubscriptionFilter, TopicSubscriptionFilter},
    time_cache::DuplicateCache,
    topic::{Hasher, Topic, TopicHash},
    transform::{DataTransform, IdentityTransform},
    types::{
        ControlAction, Graft, IHave, IWant, Message, MessageAcceptance, MessageId, PeerConnections,
        PeerInfo, PeerKind, Prune, RawMessage, RpcOut, Subscription, SubscriptionAction,
    },
    FailedMessages, PublishError, SubscriptionError, TopicScoreParams, ValidationError,
};

#[cfg(test)]
mod tests;

/// Determines if published messages should be signed or not.
///
/// Without signing, a number of privacy preserving modes can be selected.
///
/// NOTE: The default validation settings are to require signatures. The [`ValidationMode`]
/// should be updated in the [`Config`] to allow for unsigned messages.
#[derive(Clone)]
pub enum MessageAuthenticity {
    /// Message signing is enabled. The author will be the owner of the key and the sequence number
    /// will be linearly increasing.
    Signed(Keypair),
    /// Message signing is disabled.
    ///
    /// The specified [`PeerId`] will be used as the author of all published messages. The sequence
    /// number will be randomized.
    Author(PeerId),
    /// Message signing is disabled.
    ///
    /// A random [`PeerId`] will be used when publishing each message. The sequence number will be
    /// randomized.
    RandomAuthor,
    /// Message signing is disabled.
    ///
    /// The author of the message and the sequence numbers are excluded from the message.
    ///
    /// NOTE: Excluding these fields may make these messages invalid by other nodes who
    /// enforce validation of these fields. See [`ValidationMode`] in the [`Config`]
    /// for how to customise this for rust-libp2p gossipsub.  A custom `message_id`
    /// function will need to be set to prevent all messages from a peer being filtered
    /// as duplicates.
    Anonymous,
}

impl MessageAuthenticity {
    /// Returns true if signing is enabled.
    pub fn is_signing(&self) -> bool {
        matches!(self, MessageAuthenticity::Signed(_))
    }

    pub fn is_anonymous(&self) -> bool {
        matches!(self, MessageAuthenticity::Anonymous)
    }
}

/// Event that can be emitted by the gossipsub behaviour.
#[derive(Debug)]
pub enum Event {
    /// A message has been received.
    Message {
        /// The peer that forwarded us this message.
        propagation_source: PeerId,
        /// The [`MessageId`] of the message. This should be referenced by the application when
        /// validating a message (if required).
        message_id: MessageId,
        /// The decompressed message itself.
        message: Message,
    },
    /// A remote subscribed to a topic.
    Subscribed {
        /// Remote that has subscribed.
        peer_id: PeerId,
        /// The topic it has subscribed to.
        topic: TopicHash,
    },
    /// A remote unsubscribed from a topic.
    Unsubscribed {
        /// Remote that has unsubscribed.
        peer_id: PeerId,
        /// The topic it has subscribed from.
        topic: TopicHash,
    },
    /// A peer that does not support gossipsub has connected.
    GossipsubNotSupported { peer_id: PeerId },
    /// A peer is not able to download messages in time.
    SlowPeer {
        /// The peer_id
        peer_id: PeerId,
        /// The types and amounts of failed messages that are occurring for this peer.
        failed_messages: FailedMessages,
    },
}

/// A data structure for storing configuration for publishing messages. See [`MessageAuthenticity`]
/// for further details.
#[allow(clippy::large_enum_variant)]
enum PublishConfig {
    Signing {
        keypair: Keypair,
        author: PeerId,
        inline_key: Option<Vec<u8>>,
        last_seq_no: SequenceNumber,
    },
    Author(PeerId),
    RandomAuthor,
    Anonymous,
}

/// A strictly linearly increasing sequence number.
///
/// We start from the current time as unix timestamp in milliseconds.
#[derive(Debug)]
struct SequenceNumber(u64);

impl SequenceNumber {
    fn new() -> Self {
        let unix_timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time to be linear")
            .as_nanos();

        Self(unix_timestamp as u64)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .checked_add(1)
            .expect("to not exhaust u64 space for sequence numbers");

        self.0
    }
}

impl PublishConfig {
    pub(crate) fn get_own_id(&self) -> Option<&PeerId> {
        match self {
            Self::Signing { author, .. } => Some(author),
            Self::Author(author) => Some(author),
            _ => None,
        }
    }
}

impl From<MessageAuthenticity> for PublishConfig {
    fn from(authenticity: MessageAuthenticity) -> Self {
        match authenticity {
            MessageAuthenticity::Signed(keypair) => {
                let public_key = keypair.public();
                let key_enc = public_key.encode_protobuf();
                let key = if key_enc.len() <= 42 {
                    // The public key can be inlined in [`rpc_proto::proto::::Message::from`], so we
                    // don't include it specifically in the
                    // [`rpc_proto::proto::Message::key`] field.
                    None
                } else {
                    // Include the protobuf encoding of the public key in the message.
                    Some(key_enc)
                };

                PublishConfig::Signing {
                    keypair,
                    author: public_key.to_peer_id(),
                    inline_key: key,
                    last_seq_no: SequenceNumber::new(),
                }
            }
            MessageAuthenticity::Author(peer_id) => PublishConfig::Author(peer_id),
            MessageAuthenticity::RandomAuthor => PublishConfig::RandomAuthor,
            MessageAuthenticity::Anonymous => PublishConfig::Anonymous,
        }
    }
}

/// Network behaviour that handles the gossipsub protocol.
///
/// NOTE: Initialisation requires a [`MessageAuthenticity`] and [`Config`] instance. If
/// message signing is disabled, the [`ValidationMode`] in the config should be adjusted to an
/// appropriate level to accept unsigned messages.
///
/// The DataTransform trait allows applications to optionally add extra encoding/decoding
/// functionality to the underlying messages. This is intended for custom compression algorithms.
///
/// The TopicSubscriptionFilter allows applications to implement specific filters on topics to
/// prevent unwanted messages being propagated and evaluated.
pub struct Behaviour<D = IdentityTransform, F = AllowAllSubscriptionFilter> {
    /// Configuration providing gossipsub performance parameters.
    config: Config,

    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<ToSwarm<Event, HandlerIn>>,

    /// Information used for publishing messages.
    publish_config: PublishConfig,

    /// An LRU Time cache for storing seen messages (based on their ID). This cache prevents
    /// duplicates from being propagated to the application and on the network.
    duplicate_cache: DuplicateCache<MessageId>,

    /// A set of connected peers, indexed by their [`PeerId`] tracking both the [`PeerKind`] and
    /// the set of [`ConnectionId`]s.
    connected_peers: HashMap<PeerId, PeerConnections>,

    /// A set of all explicit peers. These are peers that remain connected and we unconditionally
    /// forward messages to, outside of the scoring system.
    explicit_peers: HashSet<PeerId>,

    /// A list of peers that have been blacklisted by the user.
    /// Messages are not sent to and are rejected from these peers.
    blacklisted_peers: HashSet<PeerId>,

    /// Overlay network of connected peers - Maps topics to connected gossipsub peers.
    mesh: HashMap<TopicHash, BTreeSet<PeerId>>,

    /// Map of topics to list of peers that we publish to, but don't subscribe to.
    fanout: HashMap<TopicHash, BTreeSet<PeerId>>,

    /// The last publish time for fanout topics.
    fanout_last_pub: HashMap<TopicHash, Instant>,

    /// Storage for backoffs
    backoffs: BackoffStorage,

    /// Message cache for the last few heartbeats.
    mcache: MessageCache,

    /// Heartbeat interval stream.
    heartbeat: Delay,

    /// Number of heartbeats since the beginning of time; this allows us to amortize some resource
    /// clean up -- eg backoff clean up.
    heartbeat_ticks: u64,

    /// We remember all peers we found through peer exchange, since those peers are not considered
    /// as safe as randomly discovered outbound peers. This behaviour diverges from the go
    /// implementation to avoid possible love bombing attacks in PX. When disconnecting peers will
    /// be removed from this list which may result in a true outbound rediscovery.
    px_peers: HashSet<PeerId>,

    /// Set of connected outbound peers (we only consider true outbound peers found through
    /// discovery and not by PX).
    outbound_peers: HashSet<PeerId>,

    /// Stores optional peer score data together with thresholds, decay interval and gossip
    /// promises.
    peer_score: Option<(PeerScore, PeerScoreThresholds, Delay, GossipPromises)>,

    /// Counts the number of `IHAVE` received from each peer since the last heartbeat.
    count_received_ihave: HashMap<PeerId, usize>,

    /// Counts the number of `IWANT` that we sent the each peer since the last heartbeat.
    count_sent_iwant: HashMap<PeerId, usize>,

    /// Short term cache for published message ids. This is used for penalizing peers sending
    /// our own messages back if the messages are anonymous or use a random author.
    published_message_ids: DuplicateCache<MessageId>,

    /// The filter used to handle message subscriptions.
    subscription_filter: F,

    /// A general transformation function that can be applied to data received from the wire before
    /// calculating the message-id and sending to the application. This is designed to allow the
    /// user to implement arbitrary topic-based compression algorithms.
    data_transform: D,

    /// Keep track of a set of internal metrics relating to gossipsub.
    metrics: Option<Metrics>,

    /// Tracks the numbers of failed messages per peer-id.
    failed_messages: HashMap<PeerId, FailedMessages>,
}

impl<D, F> Behaviour<D, F>
where
    D: DataTransform + Default,
    F: TopicSubscriptionFilter + Default,
{
    /// Creates a Gossipsub [`Behaviour`] struct given a set of parameters specified via a
    /// [`Config`]. This has no subscription filter and uses no compression.
    pub fn new(privacy: MessageAuthenticity, config: Config) -> Result<Self, &'static str> {
        Self::new_with_subscription_filter_and_transform(
            privacy,
            config,
            None,
            F::default(),
            D::default(),
        )
    }

    /// Creates a Gossipsub [`Behaviour`] struct given a set of parameters specified via a
    /// [`Config`]. This has no subscription filter and uses no compression.
    /// Metrics can be evaluated by passing a reference to a [`Registry`].
    pub fn new_with_metrics(
        privacy: MessageAuthenticity,
        config: Config,
        metrics_registry: &mut Registry,
        metrics_config: MetricsConfig,
    ) -> Result<Self, &'static str> {
        Self::new_with_subscription_filter_and_transform(
            privacy,
            config,
            Some((metrics_registry, metrics_config)),
            F::default(),
            D::default(),
        )
    }
}

impl<D, F> Behaviour<D, F>
where
    D: DataTransform + Default,
    F: TopicSubscriptionFilter,
{
    /// Creates a Gossipsub [`Behaviour`] struct given a set of parameters specified via a
    /// [`Config`] and a custom subscription filter.
    pub fn new_with_subscription_filter(
        privacy: MessageAuthenticity,
        config: Config,
        metrics: Option<(&mut Registry, MetricsConfig)>,
        subscription_filter: F,
    ) -> Result<Self, &'static str> {
        Self::new_with_subscription_filter_and_transform(
            privacy,
            config,
            metrics,
            subscription_filter,
            D::default(),
        )
    }
}

impl<D, F> Behaviour<D, F>
where
    D: DataTransform,
    F: TopicSubscriptionFilter + Default,
{
    /// Creates a Gossipsub [`Behaviour`] struct given a set of parameters specified via a
    /// [`Config`] and a custom data transform.
    pub fn new_with_transform(
        privacy: MessageAuthenticity,
        config: Config,
        metrics: Option<(&mut Registry, MetricsConfig)>,
        data_transform: D,
    ) -> Result<Self, &'static str> {
        Self::new_with_subscription_filter_and_transform(
            privacy,
            config,
            metrics,
            F::default(),
            data_transform,
        )
    }
}

impl<D, F> Behaviour<D, F>
where
    D: DataTransform,
    F: TopicSubscriptionFilter,
{
    /// Creates a Gossipsub [`Behaviour`] struct given a set of parameters specified via a
    /// [`Config`] and a custom subscription filter and data transform.
    pub fn new_with_subscription_filter_and_transform(
        privacy: MessageAuthenticity,
        config: Config,
        metrics: Option<(&mut Registry, MetricsConfig)>,
        subscription_filter: F,
        data_transform: D,
    ) -> Result<Self, &'static str> {
        // Set up the router given the configuration settings.

        // We do not allow configurations where a published message would also be rejected if it
        // were received locally.
        validate_config(&privacy, config.validation_mode())?;

        Ok(Behaviour {
            metrics: metrics.map(|(registry, cfg)| Metrics::new(registry, cfg)),
            events: VecDeque::new(),
            publish_config: privacy.into(),
            duplicate_cache: DuplicateCache::new(config.duplicate_cache_time()),
            explicit_peers: HashSet::new(),
            blacklisted_peers: HashSet::new(),
            mesh: HashMap::new(),
            fanout: HashMap::new(),
            fanout_last_pub: HashMap::new(),
            backoffs: BackoffStorage::new(
                &config.prune_backoff(),
                config.heartbeat_interval(),
                config.backoff_slack(),
            ),
            mcache: MessageCache::new(config.history_gossip(), config.history_length()),
            heartbeat: Delay::new(config.heartbeat_interval() + config.heartbeat_initial_delay()),
            heartbeat_ticks: 0,
            px_peers: HashSet::new(),
            outbound_peers: HashSet::new(),
            peer_score: None,
            count_received_ihave: HashMap::new(),
            count_sent_iwant: HashMap::new(),
            connected_peers: HashMap::new(),
            published_message_ids: DuplicateCache::new(config.published_message_ids_cache_time()),
            config,
            subscription_filter,
            data_transform,
            failed_messages: Default::default(),
        })
    }
}

impl<D, F> Behaviour<D, F>
where
    D: DataTransform + Send + 'static,
    F: TopicSubscriptionFilter + Send + 'static,
{
    /// Lists the hashes of the topics we are currently subscribed to.
    pub fn topics(&self) -> impl Iterator<Item = &TopicHash> {
        self.mesh.keys()
    }

    /// Lists all mesh peers for a certain topic hash.
    pub fn mesh_peers(&self, topic_hash: &TopicHash) -> impl Iterator<Item = &PeerId> {
        self.mesh.get(topic_hash).into_iter().flat_map(|x| x.iter())
    }

    pub fn all_mesh_peers(&self) -> impl Iterator<Item = &PeerId> {
        let mut res = BTreeSet::new();
        for peers in self.mesh.values() {
            res.extend(peers);
        }
        res.into_iter()
    }

    /// Lists all known peers and their associated subscribed topics.
    pub fn all_peers(&self) -> impl Iterator<Item = (&PeerId, Vec<&TopicHash>)> {
        self.connected_peers
            .iter()
            .map(|(peer_id, peer)| (peer_id, peer.topics.iter().collect()))
    }

    /// Lists all known peers and their associated protocol.
    pub fn peer_protocol(&self) -> impl Iterator<Item = (&PeerId, &PeerKind)> {
        self.connected_peers.iter().map(|(k, v)| (k, &v.kind))
    }

    /// Returns the gossipsub score for a given peer, if one exists.
    pub fn peer_score(&self, peer_id: &PeerId) -> Option<f64> {
        self.peer_score
            .as_ref()
            .map(|(score, ..)| score.score(peer_id))
    }

    /// Subscribe to a topic.
    ///
    /// Returns [`Ok(true)`] if the subscription worked. Returns [`Ok(false)`] if we were already
    /// subscribed.
    pub fn subscribe<H: Hasher>(&mut self, topic: &Topic<H>) -> Result<bool, SubscriptionError> {
        tracing::debug!(%topic, "Subscribing to topic");
        let topic_hash = topic.hash();
        if !self.subscription_filter.can_subscribe(&topic_hash) {
            return Err(SubscriptionError::NotAllowed);
        }

        if self.mesh.contains_key(&topic_hash) {
            tracing::debug!(%topic, "Topic is already in the mesh");
            return Ok(false);
        }

        // send subscription request to all peers
        for peer_id in self.connected_peers.keys().copied().collect::<Vec<_>>() {
            tracing::debug!(%peer_id, "Sending SUBSCRIBE to peer");
            let event = RpcOut::Subscribe(topic_hash.clone());
            self.send_message(peer_id, event);
        }

        // call JOIN(topic)
        // this will add new peers to the mesh for the topic
        self.join(&topic_hash);
        tracing::debug!(%topic, "Subscribed to topic");
        Ok(true)
    }

    /// Unsubscribes from a topic.
    ///
    /// Returns `true` if we were subscribed to this topic.
    pub fn unsubscribe<H: Hasher>(&mut self, topic: &Topic<H>) -> bool {
        tracing::debug!(%topic, "Unsubscribing from topic");
        let topic_hash = topic.hash();

        if !self.mesh.contains_key(&topic_hash) {
            tracing::debug!(topic=%topic_hash, "Already unsubscribed from topic");
            // we are not subscribed
            return false;
        }

        // announce to all peers
        for peer in self.connected_peers.keys().copied().collect::<Vec<_>>() {
            tracing::debug!(%peer, "Sending UNSUBSCRIBE to peer");
            let event = RpcOut::Unsubscribe(topic_hash.clone());
            self.send_message(peer, event);
        }

        // call LEAVE(topic)
        // this will remove the topic from the mesh
        self.leave(&topic_hash);

        tracing::debug!(topic=%topic_hash, "Unsubscribed from topic");
        true
    }

    /// Publishes a message with multiple topics to the network.
    pub fn publish(
        &mut self,
        topic: impl Into<TopicHash>,
        data: impl Into<Vec<u8>>,
    ) -> Result<MessageId, PublishError> {
        let data = data.into();
        let topic = topic.into();

        // Transform the data before building a raw_message.
        let transformed_data = self
            .data_transform
            .outbound_transform(&topic, data.clone())?;

        // check that the size doesn't exceed the max transmission size.
        if transformed_data.len() > self.config.max_transmit_size() {
            return Err(PublishError::MessageTooLarge);
        }

        let raw_message = self.build_raw_message(topic, transformed_data)?;

        // calculate the message id from the un-transformed data
        let msg_id = self.config.message_id(&Message {
            source: raw_message.source,
            data, // the uncompressed form
            sequence_number: raw_message.sequence_number,
            topic: raw_message.topic.clone(),
        });

        // Check the if the message has been published before
        if self.duplicate_cache.contains(&msg_id) {
            // This message has already been seen. We don't re-publish messages that have already
            // been published on the network.
            tracing::warn!(
                message=%msg_id,
                "Not publishing a message that has already been published"
            );
            return Err(PublishError::Duplicate);
        }

        tracing::trace!(message=%msg_id, "Publishing message");

        let topic_hash = raw_message.topic.clone();

        let mut peers_on_topic = self
            .connected_peers
            .iter()
            .filter(|(_, p)| p.topics.contains(&topic_hash))
            .map(|(peer_id, _)| peer_id)
            .peekable();

        if peers_on_topic.peek().is_none() {
            return Err(PublishError::InsufficientPeers);
        }

        let mut recipient_peers = HashSet::new();
        if self.config.flood_publish() {
            // Forward to all peers above score and all explicit peers
            recipient_peers.extend(peers_on_topic.filter(|p| {
                self.explicit_peers.contains(*p)
                    || !self.score_below_threshold(p, |ts| ts.publish_threshold).0
            }));
        } else {
            match self.mesh.get(&topic_hash) {
                // Mesh peers
                Some(mesh_peers) => {
                    // We have a mesh set. We want to make sure to publish to at least `mesh_n`
                    // peers (if possible).
                    let needed_extra_peers = self.config.mesh_n().saturating_sub(mesh_peers.len());

                    if needed_extra_peers > 0 {
                        // We don't have `mesh_n` peers in our mesh, we will randomly select extras
                        // and publish to them.

                        // Get a random set of peers that are appropriate to send messages too.
                        let peer_list = get_random_peers(
                            &self.connected_peers,
                            &topic_hash,
                            needed_extra_peers,
                            |peer| {
                                !mesh_peers.contains(peer)
                                    && !self.explicit_peers.contains(peer)
                                    && !self
                                        .score_below_threshold(peer, |pst| pst.publish_threshold)
                                        .0
                            },
                        );
                        recipient_peers.extend(peer_list);
                    }

                    recipient_peers.extend(mesh_peers);
                }
                // Gossipsub peers
                None => {
                    tracing::debug!(topic=%topic_hash, "Topic not in the mesh");
                    // If we have fanout peers add them to the map.
                    if self.fanout.contains_key(&topic_hash) {
                        for peer in self.fanout.get(&topic_hash).expect("Topic must exist") {
                            recipient_peers.insert(*peer);
                        }
                    } else {
                        // We have no fanout peers, select mesh_n of them and add them to the fanout
                        let mesh_n = self.config.mesh_n();
                        let new_peers =
                            get_random_peers(&self.connected_peers, &topic_hash, mesh_n, {
                                |p| {
                                    !self.explicit_peers.contains(p)
                                        && !self
                                            .score_below_threshold(p, |pst| pst.publish_threshold)
                                            .0
                                }
                            });
                        // Add the new peers to the fanout and recipient peers
                        self.fanout.insert(topic_hash.clone(), new_peers.clone());
                        for peer in new_peers {
                            tracing::debug!(%peer, "Peer added to fanout");
                            recipient_peers.insert(peer);
                        }
                    }
                    // We are publishing to fanout peers - update the time we published
                    self.fanout_last_pub
                        .insert(topic_hash.clone(), Instant::now());
                }
            }

            // Explicit peers that are part of the topic
            recipient_peers
                .extend(peers_on_topic.filter(|peer_id| self.explicit_peers.contains(peer_id)));

            // Floodsub peers
            for (peer, connections) in &self.connected_peers {
                if connections.kind == PeerKind::Floodsub
                    && !self
                        .score_below_threshold(peer, |ts| ts.publish_threshold)
                        .0
                {
                    recipient_peers.insert(*peer);
                }
            }
        }

        // If the message isn't a duplicate and we have sent it to some peers add it to the
        // duplicate cache and memcache.
        self.duplicate_cache.insert(msg_id.clone());
        self.mcache.put(&msg_id, raw_message.clone());

        // If the message is anonymous or has a random author add it to the published message ids
        // cache.
        if let PublishConfig::RandomAuthor | PublishConfig::Anonymous = self.publish_config {
            if !self.config.allow_self_origin() {
                self.published_message_ids.insert(msg_id.clone());
            }
        }

        // Send to peers we know are subscribed to the topic.
        let mut publish_failed = true;
        for peer_id in recipient_peers.iter() {
            tracing::trace!(peer=%peer_id, "Sending message to peer");
            if self.send_message(
                *peer_id,
                RpcOut::Publish {
                    message: raw_message.clone(),
                    timeout: Delay::new(self.config.publish_queue_duration()),
                },
            ) {
                publish_failed = false
            }
        }

        if recipient_peers.is_empty() {
            return Err(PublishError::InsufficientPeers);
        }

        if publish_failed {
            return Err(PublishError::AllQueuesFull(recipient_peers.len()));
        }

        tracing::debug!(message=%msg_id, "Published message");

        if let Some(metrics) = self.metrics.as_mut() {
            metrics.register_published_message(&topic_hash);
        }

        Ok(msg_id)
    }

    /// This function should be called when [`Config::validate_messages()`] is `true` after
    /// the message got validated by the caller. Messages are stored in the ['Memcache'] and
    /// validation is expected to be fast enough that the messages should still exist in the cache.
    /// There are three possible validation outcomes and the outcome is given in acceptance.
    ///
    /// If acceptance = [`MessageAcceptance::Accept`] the message will get propagated to the
    /// network. The `propagation_source` parameter indicates who the message was received by and
    /// will not be forwarded back to that peer.
    ///
    /// If acceptance = [`MessageAcceptance::Reject`] the message will be deleted from the memcache
    /// and the P₄ penalty will be applied to the `propagation_source`.
    //
    /// If acceptance = [`MessageAcceptance::Ignore`] the message will be deleted from the memcache
    /// but no P₄ penalty will be applied.
    ///
    /// This function will return true if the message was found in the cache and false if was not
    /// in the cache anymore.
    ///
    /// This should only be called once per message.
    pub fn report_message_validation_result(
        &mut self,
        msg_id: &MessageId,
        propagation_source: &PeerId,
        acceptance: MessageAcceptance,
    ) -> bool {
        let reject_reason = match acceptance {
            MessageAcceptance::Accept => {
                let (raw_message, originating_peers) = match self.mcache.validate(msg_id) {
                    Some((raw_message, originating_peers)) => {
                        (raw_message.clone(), originating_peers)
                    }
                    None => {
                        tracing::warn!(
                            message=%msg_id,
                            "Message not in cache. Ignoring forwarding"
                        );
                        if let Some(metrics) = self.metrics.as_mut() {
                            metrics.memcache_miss();
                        }
                        return false;
                    }
                };

                if let Some(metrics) = self.metrics.as_mut() {
                    metrics.register_msg_validation(&raw_message.topic, &acceptance);
                }

                self.forward_msg(
                    msg_id,
                    raw_message,
                    Some(propagation_source),
                    originating_peers,
                );
                return true;
            }
            MessageAcceptance::Reject => RejectReason::ValidationFailed,
            MessageAcceptance::Ignore => RejectReason::ValidationIgnored,
        };

        if let Some((raw_message, originating_peers)) = self.mcache.remove(msg_id) {
            if let Some(metrics) = self.metrics.as_mut() {
                metrics.register_msg_validation(&raw_message.topic, &acceptance);
            }

            // Tell peer_score about reject
            // Reject the original source, and any duplicates we've seen from other peers.
            if let Some((peer_score, ..)) = &mut self.peer_score {
                peer_score.reject_message(
                    propagation_source,
                    msg_id,
                    &raw_message.topic,
                    reject_reason,
                );
                for peer in originating_peers.iter() {
                    peer_score.reject_message(peer, msg_id, &raw_message.topic, reject_reason);
                }
            }
            true
        } else {
            tracing::warn!(message=%msg_id, "Rejected message not in cache");
            false
        }
    }

    /// Adds a new peer to the list of explicitly connected peers.
    pub fn add_explicit_peer(&mut self, peer_id: &PeerId) {
        tracing::debug!(peer=%peer_id, "Adding explicit peer");

        self.explicit_peers.insert(*peer_id);

        self.check_explicit_peer_connection(peer_id);
    }

    /// This removes the peer from explicitly connected peers, note that this does not disconnect
    /// the peer.
    pub fn remove_explicit_peer(&mut self, peer_id: &PeerId) {
        tracing::debug!(peer=%peer_id, "Removing explicit peer");
        self.explicit_peers.remove(peer_id);
    }

    /// Blacklists a peer. All messages from this peer will be rejected and any message that was
    /// created by this peer will be rejected.
    pub fn blacklist_peer(&mut self, peer_id: &PeerId) {
        if self.blacklisted_peers.insert(*peer_id) {
            tracing::debug!(peer=%peer_id, "Peer has been blacklisted");
        }
    }

    /// Removes a peer from the blacklist if it has previously been blacklisted.
    pub fn remove_blacklisted_peer(&mut self, peer_id: &PeerId) {
        if self.blacklisted_peers.remove(peer_id) {
            tracing::debug!(peer=%peer_id, "Peer has been removed from the blacklist");
        }
    }

    /// Activates the peer scoring system with the given parameters. This will reset all scores
    /// if there was already another peer scoring system activated. Returns an error if the
    /// params are not valid or if they got already set.
    pub fn with_peer_score(
        &mut self,
        params: PeerScoreParams,
        threshold: PeerScoreThresholds,
    ) -> Result<(), String> {
        self.with_peer_score_and_message_delivery_time_callback(params, threshold, None)
    }

    /// Activates the peer scoring system with the given parameters and a message delivery time
    /// callback. Returns an error if the parameters got already set.
    pub fn with_peer_score_and_message_delivery_time_callback(
        &mut self,
        params: PeerScoreParams,
        threshold: PeerScoreThresholds,
        callback: Option<fn(&PeerId, &TopicHash, f64)>,
    ) -> Result<(), String> {
        params.validate()?;
        threshold.validate()?;

        if self.peer_score.is_some() {
            return Err("Peer score set twice".into());
        }

        let interval = Delay::new(params.decay_interval);
        let peer_score = PeerScore::new_with_message_delivery_time_callback(params, callback);
        self.peer_score = Some((peer_score, threshold, interval, GossipPromises::default()));
        Ok(())
    }

    /// Sets scoring parameters for a topic.
    ///
    /// The [`Self::with_peer_score()`] must first be called to initialise peer scoring.
    pub fn set_topic_params<H: Hasher>(
        &mut self,
        topic: Topic<H>,
        params: TopicScoreParams,
    ) -> Result<(), &'static str> {
        if let Some((peer_score, ..)) = &mut self.peer_score {
            peer_score.set_topic_params(topic.hash(), params);
            Ok(())
        } else {
            Err("Peer score must be initialised with `with_peer_score()`")
        }
    }

    /// Returns a scoring parameters for a topic if existent.
    pub fn get_topic_params<H: Hasher>(&self, topic: &Topic<H>) -> Option<&TopicScoreParams> {
        self.peer_score.as_ref()?.0.get_topic_params(&topic.hash())
    }

    /// Sets the application specific score for a peer. Returns true if scoring is active and
    /// the peer is connected or if the score of the peer is not yet expired, false otherwise.
    pub fn set_application_score(&mut self, peer_id: &PeerId, new_score: f64) -> bool {
        if let Some((peer_score, ..)) = &mut self.peer_score {
            peer_score.set_application_score(peer_id, new_score)
        } else {
            false
        }
    }

    /// Gossipsub JOIN(topic) - adds topic peers to mesh and sends them GRAFT messages.
    fn join(&mut self, topic_hash: &TopicHash) {
        tracing::debug!(topic=%topic_hash, "Running JOIN for topic");

        // if we are already in the mesh, return
        if self.mesh.contains_key(topic_hash) {
            tracing::debug!(topic=%topic_hash, "JOIN: The topic is already in the mesh, ignoring JOIN");
            return;
        }

        let mut added_peers = HashSet::new();

        if let Some(m) = self.metrics.as_mut() {
            m.joined(topic_hash)
        }

        // check if we have mesh_n peers in fanout[topic] and add them to the mesh if we do,
        // removing the fanout entry.
        if let Some((_, mut peers)) = self.fanout.remove_entry(topic_hash) {
            tracing::debug!(
                topic=%topic_hash,
                "JOIN: Removing peers from the fanout for topic"
            );

            // remove explicit peers, peers with negative scores, and backoffed peers
            peers.retain(|p| {
                !self.explicit_peers.contains(p)
                    && !self.score_below_threshold(p, |_| 0.0).0
                    && !self.backoffs.is_backoff_with_slack(topic_hash, p)
            });

            // Add up to mesh_n of them to the mesh
            // NOTE: These aren't randomly added, currently FIFO
            let add_peers = std::cmp::min(peers.len(), self.config.mesh_n());
            tracing::debug!(
                topic=%topic_hash,
                "JOIN: Adding {:?} peers from the fanout for topic",
                add_peers
            );
            added_peers.extend(peers.iter().take(add_peers));

            self.mesh.insert(
                topic_hash.clone(),
                peers.into_iter().take(add_peers).collect(),
            );

            // remove the last published time
            self.fanout_last_pub.remove(topic_hash);
        }

        let fanaout_added = added_peers.len();
        if let Some(m) = self.metrics.as_mut() {
            m.peers_included(topic_hash, Inclusion::Fanout, fanaout_added)
        }

        // check if we need to get more peers, which we randomly select
        if added_peers.len() < self.config.mesh_n() {
            // get the peers
            let new_peers = get_random_peers(
                &self.connected_peers,
                topic_hash,
                self.config.mesh_n() - added_peers.len(),
                |peer| {
                    !added_peers.contains(peer)
                        && !self.explicit_peers.contains(peer)
                        && !self.score_below_threshold(peer, |_| 0.0).0
                        && !self.backoffs.is_backoff_with_slack(topic_hash, peer)
                },
            );
            added_peers.extend(new_peers.clone());
            // add them to the mesh
            tracing::debug!(
                "JOIN: Inserting {:?} random peers into the mesh",
                new_peers.len()
            );
            let mesh_peers = self.mesh.entry(topic_hash.clone()).or_default();
            mesh_peers.extend(new_peers);
        }

        let random_added = added_peers.len() - fanaout_added;
        if let Some(m) = self.metrics.as_mut() {
            m.peers_included(topic_hash, Inclusion::Random, random_added)
        }

        for peer_id in added_peers {
            // Send a GRAFT control message
            tracing::debug!(peer=%peer_id, "JOIN: Sending Graft message to peer");
            if let Some((peer_score, ..)) = &mut self.peer_score {
                peer_score.graft(&peer_id, topic_hash.clone());
            }
            self.send_message(
                peer_id,
                RpcOut::Graft(Graft {
                    topic_hash: topic_hash.clone(),
                }),
            );

            // If the peer did not previously exist in any mesh, inform the handler
            peer_added_to_mesh(
                peer_id,
                vec![topic_hash],
                &self.mesh,
                &mut self.events,
                &self.connected_peers,
            );
        }

        let mesh_peers = self.mesh_peers(topic_hash).count();
        if let Some(m) = self.metrics.as_mut() {
            m.set_mesh_peers(topic_hash, mesh_peers)
        }

        tracing::debug!(topic=%topic_hash, "Completed JOIN for topic");
    }

    /// Creates a PRUNE gossipsub action.
    fn make_prune(
        &mut self,
        topic_hash: &TopicHash,
        peer: &PeerId,
        do_px: bool,
        on_unsubscribe: bool,
    ) -> Prune {
        if let Some((peer_score, ..)) = &mut self.peer_score {
            peer_score.prune(peer, topic_hash.clone());
        }

        match self.connected_peers.get(peer).map(|v| &v.kind) {
            Some(PeerKind::Floodsub) => {
                tracing::error!("Attempted to prune a Floodsub peer");
            }
            Some(PeerKind::Gossipsub) => {
                // GossipSub v1.0 -- no peer exchange, the peer won't be able to parse it anyway
                return Prune {
                    topic_hash: topic_hash.clone(),
                    peers: Vec::new(),
                    backoff: None,
                };
            }
            None => {
                tracing::error!("Attempted to Prune an unknown peer");
            }
            _ => {} // Gossipsub 1.1 peer perform the `Prune`
        }

        // Select peers for peer exchange
        let peers = if do_px {
            get_random_peers(
                &self.connected_peers,
                topic_hash,
                self.config.prune_peers(),
                |p| p != peer && !self.score_below_threshold(p, |_| 0.0).0,
            )
            .into_iter()
            .map(|p| PeerInfo { peer_id: Some(p) })
            .collect()
        } else {
            Vec::new()
        };

        let backoff = if on_unsubscribe {
            self.config.unsubscribe_backoff()
        } else {
            self.config.prune_backoff()
        };

        // update backoff
        self.backoffs.update_backoff(topic_hash, peer, backoff);

        Prune {
            topic_hash: topic_hash.clone(),
            peers,
            backoff: Some(backoff.as_secs()),
        }
    }

    /// Gossipsub LEAVE(topic) - Notifies mesh\[topic\] peers with PRUNE messages.
    fn leave(&mut self, topic_hash: &TopicHash) {
        tracing::debug!(topic=%topic_hash, "Running LEAVE for topic");

        // If our mesh contains the topic, send prune to peers and delete it from the mesh
        if let Some((_, peers)) = self.mesh.remove_entry(topic_hash) {
            if let Some(m) = self.metrics.as_mut() {
                m.left(topic_hash)
            }
            for peer_id in peers {
                // Send a PRUNE control message
                tracing::debug!(%peer_id, "LEAVE: Sending PRUNE to peer");

                let on_unsubscribe = true;
                let prune =
                    self.make_prune(topic_hash, &peer_id, self.config.do_px(), on_unsubscribe);
                self.send_message(peer_id, RpcOut::Prune(prune));

                // If the peer did not previously exist in any mesh, inform the handler
                peer_removed_from_mesh(
                    peer_id,
                    topic_hash,
                    &self.mesh,
                    &mut self.events,
                    &self.connected_peers,
                );
            }
        }
        tracing::debug!(topic=%topic_hash, "Completed LEAVE for topic");
    }

    /// Checks if the given peer is still connected and if not dials the peer again.
    fn check_explicit_peer_connection(&mut self, peer_id: &PeerId) {
        if !self.connected_peers.contains_key(peer_id) {
            // Connect to peer
            tracing::debug!(peer=%peer_id, "Connecting to explicit peer");
            self.events.push_back(ToSwarm::Dial {
                opts: DialOpts::peer_id(*peer_id).build(),
            });
        }
    }

    /// Determines if a peer's score is below a given `PeerScoreThreshold` chosen via the
    /// `threshold` parameter.
    fn score_below_threshold(
        &self,
        peer_id: &PeerId,
        threshold: impl Fn(&PeerScoreThresholds) -> f64,
    ) -> (bool, f64) {
        Self::score_below_threshold_from_scores(&self.peer_score, peer_id, threshold)
    }

    fn score_below_threshold_from_scores(
        peer_score: &Option<(PeerScore, PeerScoreThresholds, Delay, GossipPromises)>,
        peer_id: &PeerId,
        threshold: impl Fn(&PeerScoreThresholds) -> f64,
    ) -> (bool, f64) {
        if let Some((peer_score, thresholds, ..)) = peer_score {
            let score = peer_score.score(peer_id);
            if score < threshold(thresholds) {
                return (true, score);
            }
            (false, score)
        } else {
            (false, 0.0)
        }
    }

    /// Handles an IHAVE control message. Checks our cache of messages. If the message is unknown,
    /// requests it with an IWANT control message.
    fn handle_ihave(&mut self, peer_id: &PeerId, ihave_msgs: Vec<(TopicHash, Vec<MessageId>)>) {
        // We ignore IHAVE gossip from any peer whose score is below the gossip threshold
        if let (true, score) = self.score_below_threshold(peer_id, |pst| pst.gossip_threshold) {
            tracing::debug!(
                peer=%peer_id,
                %score,
                "IHAVE: ignoring peer with score below threshold"
            );
            return;
        }

        // IHAVE flood protection
        let peer_have = self.count_received_ihave.entry(*peer_id).or_insert(0);
        *peer_have += 1;
        if *peer_have > self.config.max_ihave_messages() {
            tracing::debug!(
                peer=%peer_id,
                "IHAVE: peer has advertised too many times ({}) within this heartbeat \
            interval; ignoring",
                *peer_have
            );
            return;
        }

        if let Some(iasked) = self.count_sent_iwant.get(peer_id) {
            if *iasked >= self.config.max_ihave_length() {
                tracing::debug!(
                    peer=%peer_id,
                    "IHAVE: peer has already advertised too many messages ({}); ignoring",
                    *iasked
                );
                return;
            }
        }

        tracing::trace!(peer=%peer_id, "Handling IHAVE for peer");

        let mut iwant_ids = HashSet::new();

        let want_message = |id: &MessageId| {
            if self.duplicate_cache.contains(id) {
                return false;
            }

            self.peer_score
                .as_ref()
                .map(|(_, _, _, promises)| !promises.contains(id))
                .unwrap_or(true)
        };

        for (topic, ids) in ihave_msgs {
            // only process the message if we are subscribed
            if !self.mesh.contains_key(&topic) {
                tracing::debug!(
                    %topic,
                    "IHAVE: Ignoring IHAVE - Not subscribed to topic"
                );
                continue;
            }

            for id in ids.into_iter().filter(want_message) {
                // have not seen this message and are not currently requesting it
                if iwant_ids.insert(id) {
                    // Register the IWANT metric
                    if let Some(metrics) = self.metrics.as_mut() {
                        metrics.register_iwant(&topic);
                    }
                }
            }
        }

        if !iwant_ids.is_empty() {
            let iasked = self.count_sent_iwant.entry(*peer_id).or_insert(0);
            let mut iask = iwant_ids.len();
            if *iasked + iask > self.config.max_ihave_length() {
                iask = self.config.max_ihave_length().saturating_sub(*iasked);
            }

            // Send the list of IWANT control messages
            tracing::debug!(
                peer=%peer_id,
                "IHAVE: Asking for {} out of {} messages from peer",
                iask,
                iwant_ids.len()
            );

            // Ask in random order
            let mut iwant_ids_vec: Vec<_> = iwant_ids.into_iter().collect();
            let mut rng = thread_rng();
            iwant_ids_vec.partial_shuffle(&mut rng, iask);

            iwant_ids_vec.truncate(iask);
            *iasked += iask;

            if let Some((_, _, _, gossip_promises)) = &mut self.peer_score {
                gossip_promises.add_promise(
                    *peer_id,
                    &iwant_ids_vec,
                    Instant::now() + self.config.iwant_followup_time(),
                );
            }
            tracing::trace!(
                peer=%peer_id,
                "IHAVE: Asking for the following messages from peer: {:?}",
                iwant_ids_vec
            );

            self.send_message(
                *peer_id,
                RpcOut::IWant(IWant {
                    message_ids: iwant_ids_vec,
                }),
            );
        }
        tracing::trace!(peer=%peer_id, "Completed IHAVE handling for peer");
    }

    /// Handles an IWANT control message. Checks our cache of messages. If the message exists it is
    /// forwarded to the requesting peer.
    fn handle_iwant(&mut self, peer_id: &PeerId, iwant_msgs: Vec<MessageId>) {
        // We ignore IWANT gossip from any peer whose score is below the gossip threshold
        if let (true, score) = self.score_below_threshold(peer_id, |pst| pst.gossip_threshold) {
            tracing::debug!(
                peer=%peer_id,
                "IWANT: ignoring peer with score below threshold [score = {}]",
                score
            );
            return;
        }

        tracing::debug!(peer=%peer_id, "Handling IWANT for peer");

        for id in iwant_msgs {
            // If we have it and the IHAVE count is not above the threshold,
            // forward the message.
            if let Some((msg, count)) = self
                .mcache
                .get_with_iwant_counts(&id, peer_id)
                .map(|(msg, count)| (msg.clone(), count))
            {
                if count > self.config.gossip_retransimission() {
                    tracing::debug!(
                        peer=%peer_id,
                        message=%id,
                        "IWANT: Peer has asked for message too many times; ignoring request"
                    );
                } else {
                    tracing::debug!(peer=%peer_id, "IWANT: Sending cached messages to peer");
                    self.send_message(
                        *peer_id,
                        RpcOut::Forward {
                            message: msg,
                            timeout: Delay::new(self.config.forward_queue_duration()),
                        },
                    );
                }
            }
        }
        tracing::debug!(peer=%peer_id, "Completed IWANT handling for peer");
    }

    /// Handles GRAFT control messages. If subscribed to the topic, adds the peer to mesh, if not,
    /// responds with PRUNE messages.
    fn handle_graft(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        tracing::debug!(peer=%peer_id, "Handling GRAFT message for peer");

        let mut to_prune_topics = HashSet::new();

        let mut do_px = self.config.do_px();

        let Some(connected_peer) = self.connected_peers.get_mut(peer_id) else {
            tracing::error!(peer_id = %peer_id, "Peer non-existent when handling graft");
            return;
        };

        // For each topic, if a peer has grafted us, then we necessarily must be in their mesh
        // and they must be subscribed to the topic. Ensure we have recorded the mapping.
        for topic in &topics {
            if connected_peer.topics.insert(topic.clone()) {
                if let Some(m) = self.metrics.as_mut() {
                    m.inc_topic_peers(topic);
                }
            }
        }

        // we don't GRAFT to/from explicit peers; complain loudly if this happens
        if self.explicit_peers.contains(peer_id) {
            tracing::warn!(peer=%peer_id, "GRAFT: ignoring request from direct peer");
            // this is possibly a bug from non-reciprocal configuration; send a PRUNE for all topics
            to_prune_topics = topics.into_iter().collect();
            // but don't PX
            do_px = false
        } else {
            let (below_zero, score) = self.score_below_threshold(peer_id, |_| 0.0);
            let now = Instant::now();
            for topic_hash in topics {
                if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                    // if the peer is already in the mesh ignore the graft
                    if peers.contains(peer_id) {
                        tracing::debug!(
                            peer=%peer_id,
                            topic=%&topic_hash,
                            "GRAFT: Received graft for peer that is already in topic"
                        );
                        continue;
                    }

                    // make sure we are not backing off that peer
                    if let Some(backoff_time) = self.backoffs.get_backoff_time(&topic_hash, peer_id)
                    {
                        if backoff_time > now {
                            tracing::warn!(
                                peer=%peer_id,
                                "[Penalty] Peer attempted graft within backoff time, penalizing"
                            );
                            // add behavioural penalty
                            if let Some((peer_score, ..)) = &mut self.peer_score {
                                if let Some(metrics) = self.metrics.as_mut() {
                                    metrics.register_score_penalty(Penalty::GraftBackoff);
                                }
                                peer_score.add_penalty(peer_id, 1);

                                // check the flood cutoff
                                // See: https://github.com/rust-lang/rust-clippy/issues/10061
                                #[allow(unknown_lints, clippy::unchecked_duration_subtraction)]
                                let flood_cutoff = (backoff_time
                                    + self.config.graft_flood_threshold())
                                    - self.config.prune_backoff();
                                if flood_cutoff > now {
                                    // extra penalty
                                    peer_score.add_penalty(peer_id, 1);
                                }
                            }
                            // no PX
                            do_px = false;

                            to_prune_topics.insert(topic_hash.clone());
                            continue;
                        }
                    }

                    // check the score
                    if below_zero {
                        // we don't GRAFT peers with negative score
                        tracing::debug!(
                            peer=%peer_id,
                            %score,
                            topic=%topic_hash,
                            "GRAFT: ignoring peer with negative score"
                        );
                        // we do send them PRUNE however, because it's a matter of protocol
                        // correctness
                        to_prune_topics.insert(topic_hash.clone());
                        // but we won't PX to them
                        do_px = false;
                        continue;
                    }

                    // check mesh upper bound and only allow graft if the upper bound is not reached
                    // or if it is an outbound peer
                    if peers.len() >= self.config.mesh_n_high()
                        && !self.outbound_peers.contains(peer_id)
                    {
                        to_prune_topics.insert(topic_hash.clone());
                        continue;
                    }

                    // add peer to the mesh
                    tracing::debug!(
                        peer=%peer_id,
                        topic=%topic_hash,
                        "GRAFT: Mesh link added for peer in topic"
                    );

                    if peers.insert(*peer_id) {
                        if let Some(m) = self.metrics.as_mut() {
                            m.peers_included(&topic_hash, Inclusion::Subscribed, 1)
                        }
                    }

                    // If the peer did not previously exist in any mesh, inform the handler
                    peer_added_to_mesh(
                        *peer_id,
                        vec![&topic_hash],
                        &self.mesh,
                        &mut self.events,
                        &self.connected_peers,
                    );

                    if let Some((peer_score, ..)) = &mut self.peer_score {
                        peer_score.graft(peer_id, topic_hash);
                    }
                } else {
                    // don't do PX when there is an unknown topic to avoid leaking our peers
                    do_px = false;
                    tracing::debug!(
                        peer=%peer_id,
                        topic=%topic_hash,
                        "GRAFT: Received graft for unknown topic from peer"
                    );
                    // spam hardening: ignore GRAFTs for unknown topics
                    continue;
                }
            }
        }

        if !to_prune_topics.is_empty() {
            // build the prune messages to send
            let on_unsubscribe = false;

            for prune in to_prune_topics
                .iter()
                .map(|t| self.make_prune(t, peer_id, do_px, on_unsubscribe))
                .collect::<Vec<_>>()
            {
                self.send_message(*peer_id, RpcOut::Prune(prune));
            }
            // Send the prune messages to the peer
            tracing::debug!(
                peer=%peer_id,
                "GRAFT: Not subscribed to topics -  Sending PRUNE to peer"
            );
        }
        tracing::debug!(peer=%peer_id, "Completed GRAFT handling for peer");
    }

    fn remove_peer_from_mesh(
        &mut self,
        peer_id: &PeerId,
        topic_hash: &TopicHash,
        backoff: Option<u64>,
        always_update_backoff: bool,
        reason: Churn,
    ) {
        let mut update_backoff = always_update_backoff;
        if let Some(peers) = self.mesh.get_mut(topic_hash) {
            // remove the peer if it exists in the mesh
            if peers.remove(peer_id) {
                tracing::debug!(
                    peer=%peer_id,
                    topic=%topic_hash,
                    "PRUNE: Removing peer from the mesh for topic"
                );
                if let Some(m) = self.metrics.as_mut() {
                    m.peers_removed(topic_hash, reason, 1)
                }

                if let Some((peer_score, ..)) = &mut self.peer_score {
                    peer_score.prune(peer_id, topic_hash.clone());
                }

                update_backoff = true;

                // inform the handler
                peer_removed_from_mesh(
                    *peer_id,
                    topic_hash,
                    &self.mesh,
                    &mut self.events,
                    &self.connected_peers,
                );
            }
        }
        if update_backoff {
            let time = if let Some(backoff) = backoff {
                Duration::from_secs(backoff)
            } else {
                self.config.prune_backoff()
            };
            // is there a backoff specified by the peer? if so obey it.
            self.backoffs.update_backoff(topic_hash, peer_id, time);
        }
    }

    /// Handles PRUNE control messages. Removes peer from the mesh.
    fn handle_prune(
        &mut self,
        peer_id: &PeerId,
        prune_data: Vec<(TopicHash, Vec<PeerInfo>, Option<u64>)>,
    ) {
        tracing::debug!(peer=%peer_id, "Handling PRUNE message for peer");
        let (below_threshold, score) =
            self.score_below_threshold(peer_id, |pst| pst.accept_px_threshold);
        for (topic_hash, px, backoff) in prune_data {
            self.remove_peer_from_mesh(peer_id, &topic_hash, backoff, true, Churn::Prune);

            if self.mesh.contains_key(&topic_hash) {
                // connect to px peers
                if !px.is_empty() {
                    // we ignore PX from peers with insufficient score
                    if below_threshold {
                        tracing::debug!(
                            peer=%peer_id,
                            %score,
                            topic=%topic_hash,
                            "PRUNE: ignoring PX from peer with insufficient score"
                        );
                        continue;
                    }

                    // NOTE: We cannot dial any peers from PX currently as we typically will not
                    // know their multiaddr. Until SignedRecords are spec'd this
                    // remains a stub. By default `config.prune_peers()` is set to zero and
                    // this is skipped. If the user modifies this, this will only be able to
                    // dial already known peers (from an external discovery mechanism for
                    // example).
                    if self.config.prune_peers() > 0 {
                        self.px_connect(px);
                    }
                }
            }
        }
        tracing::debug!(peer=%peer_id, "Completed PRUNE handling for peer");
    }

    fn px_connect(&mut self, mut px: Vec<PeerInfo>) {
        let n = self.config.prune_peers();
        // Ignore peerInfo with no ID
        //
        // TODO: Once signed records are spec'd: Can we use peerInfo without any IDs if they have a
        // signed peer record?
        px.retain(|p| p.peer_id.is_some());
        if px.len() > n {
            // only use at most prune_peers many random peers
            let mut rng = thread_rng();
            px.partial_shuffle(&mut rng, n);
            px = px.into_iter().take(n).collect();
        }

        for p in px {
            // TODO: Once signed records are spec'd: extract signed peer record if given and handle
            // it, see https://github.com/libp2p/specs/pull/217
            if let Some(peer_id) = p.peer_id {
                // mark as px peer
                self.px_peers.insert(peer_id);

                // dial peer
                self.events.push_back(ToSwarm::Dial {
                    opts: DialOpts::peer_id(peer_id).build(),
                });
            }
        }
    }

    /// Applies some basic checks to whether this message is valid. Does not apply user validation
    /// checks.
    fn message_is_valid(
        &mut self,
        msg_id: &MessageId,
        raw_message: &mut RawMessage,
        propagation_source: &PeerId,
    ) -> bool {
        tracing::debug!(
            peer=%propagation_source,
            message=%msg_id,
            "Handling message from peer"
        );

        // Reject any message from a blacklisted peer
        if self.blacklisted_peers.contains(propagation_source) {
            tracing::debug!(
                peer=%propagation_source,
                "Rejecting message from blacklisted peer"
            );
            if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
                peer_score.reject_message(
                    propagation_source,
                    msg_id,
                    &raw_message.topic,
                    RejectReason::BlackListedPeer,
                );
                gossip_promises.reject_message(msg_id, &RejectReason::BlackListedPeer);
            }
            return false;
        }

        // Also reject any message that originated from a blacklisted peer
        if let Some(source) = raw_message.source.as_ref() {
            if self.blacklisted_peers.contains(source) {
                tracing::debug!(
                    peer=%propagation_source,
                    %source,
                    "Rejecting message from peer because of blacklisted source"
                );
                self.handle_invalid_message(
                    propagation_source,
                    raw_message,
                    RejectReason::BlackListedSource,
                );
                return false;
            }
        }

        // If we are not validating messages, assume this message is validated
        // This will allow the message to be gossiped without explicitly calling
        // `validate_message`.
        if !self.config.validate_messages() {
            raw_message.validated = true;
        }

        // reject messages claiming to be from ourselves but not locally published
        let self_published = !self.config.allow_self_origin()
            && if let Some(own_id) = self.publish_config.get_own_id() {
                own_id != propagation_source
                    && raw_message.source.as_ref().is_some_and(|s| s == own_id)
            } else {
                self.published_message_ids.contains(msg_id)
            };

        if self_published {
            tracing::debug!(
                message=%msg_id,
                source=%propagation_source,
                "Dropping message claiming to be from self but forwarded from source"
            );
            self.handle_invalid_message(propagation_source, raw_message, RejectReason::SelfOrigin);
            return false;
        }

        true
    }

    /// Handles a newly received [`RawMessage`].
    ///
    /// Forwards the message to all peers in the mesh.
    fn handle_received_message(
        &mut self,
        mut raw_message: RawMessage,
        propagation_source: &PeerId,
    ) {
        // Record the received metric
        if let Some(metrics) = self.metrics.as_mut() {
            metrics.msg_recvd_unfiltered(&raw_message.topic, raw_message.raw_protobuf_len());
        }

        // Try and perform the data transform to the message. If it fails, consider it invalid.
        let message = match self.data_transform.inbound_transform(raw_message.clone()) {
            Ok(message) => message,
            Err(e) => {
                tracing::debug!("Invalid message. Transform error: {:?}", e);
                // Reject the message and return
                self.handle_invalid_message(
                    propagation_source,
                    &raw_message,
                    RejectReason::ValidationError(ValidationError::TransformFailed),
                );
                return;
            }
        };

        // Calculate the message id on the transformed data.
        let msg_id = self.config.message_id(&message);

        // Check the validity of the message
        // Peers get penalized if this message is invalid. We don't add it to the duplicate cache
        // and instead continually penalize peers that repeatedly send this message.
        if !self.message_is_valid(&msg_id, &mut raw_message, propagation_source) {
            return;
        }

        if !self.duplicate_cache.insert(msg_id.clone()) {
            tracing::debug!(message=%msg_id, "Message already received, ignoring");
            if let Some((peer_score, ..)) = &mut self.peer_score {
                peer_score.duplicated_message(propagation_source, &msg_id, &message.topic);
            }
            self.mcache.observe_duplicate(&msg_id, propagation_source);
            return;
        }
        tracing::debug!(
            message=%msg_id,
            "Put message in duplicate_cache and resolve promises"
        );

        // Record the received message with the metrics
        if let Some(metrics) = self.metrics.as_mut() {
            metrics.msg_recvd(&message.topic);
        }

        // Tells score that message arrived (but is maybe not fully validated yet).
        // Consider the message as delivered for gossip promises.
        if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
            peer_score.validate_message(propagation_source, &msg_id, &message.topic);
            gossip_promises.message_delivered(&msg_id);
        }

        // Add the message to our memcache
        self.mcache.put(&msg_id, raw_message.clone());

        // Dispatch the message to the user if we are subscribed to any of the topics
        if self.mesh.contains_key(&message.topic) {
            tracing::debug!("Sending received message to user");
            self.events
                .push_back(ToSwarm::GenerateEvent(Event::Message {
                    propagation_source: *propagation_source,
                    message_id: msg_id.clone(),
                    message,
                }));
        } else {
            tracing::debug!(
                topic=%message.topic,
                "Received message on a topic we are not subscribed to"
            );
            return;
        }

        // forward the message to mesh peers, if no validation is required
        if !self.config.validate_messages() {
            self.forward_msg(
                &msg_id,
                raw_message,
                Some(propagation_source),
                HashSet::new(),
            );
            tracing::debug!(message=%msg_id, "Completed message handling for message");
        }
    }

    // Handles invalid messages received.
    fn handle_invalid_message(
        &mut self,
        propagation_source: &PeerId,
        raw_message: &RawMessage,
        reject_reason: RejectReason,
    ) {
        if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
            if let Some(metrics) = self.metrics.as_mut() {
                metrics.register_invalid_message(&raw_message.topic);
            }

            if let Ok(message) = self.data_transform.inbound_transform(raw_message.clone()) {
                let message_id = self.config.message_id(&message);

                peer_score.reject_message(
                    propagation_source,
                    &message_id,
                    &message.topic,
                    reject_reason,
                );

                gossip_promises.reject_message(&message_id, &reject_reason);
            } else {
                // The message is invalid, we reject it ignoring any gossip promises. If a peer is
                // advertising this message via an IHAVE and it's invalid it will be double
                // penalized, one for sending us an invalid and again for breaking a promise.
                peer_score.reject_invalid_message(propagation_source, &raw_message.topic);
            }
        }
    }

    /// Handles received subscriptions.
    fn handle_received_subscriptions(
        &mut self,
        subscriptions: &[Subscription],
        propagation_source: &PeerId,
    ) {
        tracing::debug!(
            source=%propagation_source,
            "Handling subscriptions: {:?}",
            subscriptions,
        );

        let mut unsubscribed_peers = Vec::new();

        let Some(peer) = self.connected_peers.get_mut(propagation_source) else {
            tracing::error!(
                peer=%propagation_source,
                "Subscription by unknown peer"
            );
            return;
        };

        // Collect potential graft topics for the peer.
        let mut topics_to_graft = Vec::new();

        // Notify the application about the subscription, after the grafts are sent.
        let mut application_event = Vec::new();

        let filtered_topics = match self
            .subscription_filter
            .filter_incoming_subscriptions(subscriptions, &peer.topics)
        {
            Ok(topics) => topics,
            Err(s) => {
                tracing::error!(
                    peer=%propagation_source,
                    "Subscription filter error: {}; ignoring RPC from peer",
                    s
                );
                return;
            }
        };

        for subscription in filtered_topics {
            // get the peers from the mapping, or insert empty lists if the topic doesn't exist
            let topic_hash = &subscription.topic_hash;

            match subscription.action {
                SubscriptionAction::Subscribe => {
                    if peer.topics.insert(topic_hash.clone()) {
                        tracing::debug!(
                            peer=%propagation_source,
                            topic=%topic_hash,
                            "SUBSCRIPTION: Adding gossip peer to topic"
                        );

                        if let Some(m) = self.metrics.as_mut() {
                            m.inc_topic_peers(topic_hash);
                        }
                    }

                    // if the mesh needs peers add the peer to the mesh
                    if !self.explicit_peers.contains(propagation_source)
                        && matches!(peer.kind, PeerKind::Gossipsubv1_1 | PeerKind::Gossipsub)
                        && !Self::score_below_threshold_from_scores(
                            &self.peer_score,
                            propagation_source,
                            |_| 0.0,
                        )
                        .0
                        && !self
                            .backoffs
                            .is_backoff_with_slack(topic_hash, propagation_source)
                    {
                        if let Some(peers) = self.mesh.get_mut(topic_hash) {
                            if peers.len() < self.config.mesh_n_low()
                                && peers.insert(*propagation_source)
                            {
                                tracing::debug!(
                                    peer=%propagation_source,
                                    topic=%topic_hash,
                                    "SUBSCRIPTION: Adding peer to the mesh for topic"
                                );
                                if let Some(m) = self.metrics.as_mut() {
                                    m.peers_included(topic_hash, Inclusion::Subscribed, 1)
                                }
                                // send graft to the peer
                                tracing::debug!(
                                    peer=%propagation_source,
                                    topic=%topic_hash,
                                    "Sending GRAFT to peer for topic"
                                );
                                if let Some((peer_score, ..)) = &mut self.peer_score {
                                    peer_score.graft(propagation_source, topic_hash.clone());
                                }
                                topics_to_graft.push(topic_hash.clone());
                            }
                        }
                    }
                    // generates a subscription event to be polled
                    application_event.push(ToSwarm::GenerateEvent(Event::Subscribed {
                        peer_id: *propagation_source,
                        topic: topic_hash.clone(),
                    }));
                }
                SubscriptionAction::Unsubscribe => {
                    if peer.topics.remove(topic_hash) {
                        tracing::debug!(
                            peer=%propagation_source,
                            topic=%topic_hash,
                            "SUBSCRIPTION: Removing gossip peer from topic"
                        );

                        if let Some(m) = self.metrics.as_mut() {
                            m.dec_topic_peers(topic_hash);
                        }
                    }

                    unsubscribed_peers.push((*propagation_source, topic_hash.clone()));
                    // generate an unsubscribe event to be polled
                    application_event.push(ToSwarm::GenerateEvent(Event::Unsubscribed {
                        peer_id: *propagation_source,
                        topic: topic_hash.clone(),
                    }));
                }
            }
        }

        // remove unsubscribed peers from the mesh and fanout if they exist there.
        for (peer_id, topic_hash) in unsubscribed_peers {
            self.fanout
                .get_mut(&topic_hash)
                .map(|peers| peers.remove(&peer_id));
            self.remove_peer_from_mesh(&peer_id, &topic_hash, None, false, Churn::Unsub);
        }

        // Potentially inform the handler if we have added this peer to a mesh for the first time.
        let topics_joined = topics_to_graft.iter().collect::<Vec<_>>();
        if !topics_joined.is_empty() {
            peer_added_to_mesh(
                *propagation_source,
                topics_joined,
                &self.mesh,
                &mut self.events,
                &self.connected_peers,
            );
        }

        // If we need to send grafts to peer, do so immediately, rather than waiting for the
        // heartbeat.
        for topic_hash in topics_to_graft.into_iter() {
            self.send_message(*propagation_source, RpcOut::Graft(Graft { topic_hash }));
        }

        // Notify the application of the subscriptions
        for event in application_event {
            self.events.push_back(event);
        }

        tracing::trace!(
            source=%propagation_source,
            "Completed handling subscriptions from source"
        );
    }

    /// Applies penalties to peers that did not respond to our IWANT requests.
    fn apply_iwant_penalties(&mut self) {
        if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
            for (peer, count) in gossip_promises.get_broken_promises() {
                peer_score.add_penalty(&peer, count);
                if let Some(metrics) = self.metrics.as_mut() {
                    metrics.register_score_penalty(Penalty::BrokenPromise);
                }
            }
        }
    }

    /// Heartbeat function which shifts the memcache and updates the mesh.
    fn heartbeat(&mut self) {
        tracing::debug!("Starting heartbeat");
        let start = Instant::now();

        // Every heartbeat we sample the send queues to add to our metrics. We do this intentionally
        // before we add all the gossip from this heartbeat in order to gain a true measure of
        // steady-state size of the queues.
        if let Some(m) = &mut self.metrics {
            for sender_queue in self.connected_peers.values().map(|v| &v.sender) {
                m.observe_priority_queue_size(sender_queue.priority_queue_len());
                m.observe_non_priority_queue_size(sender_queue.non_priority_queue_len());
            }
        }

        self.heartbeat_ticks += 1;

        let mut to_graft = HashMap::new();
        let mut to_prune = HashMap::new();
        let mut no_px = HashSet::new();

        // clean up expired backoffs
        self.backoffs.heartbeat();

        // clean up ihave counters
        self.count_sent_iwant.clear();
        self.count_received_ihave.clear();

        // apply iwant penalties
        self.apply_iwant_penalties();

        // check connections to explicit peers
        if self.heartbeat_ticks % self.config.check_explicit_peers_ticks() == 0 {
            for p in self.explicit_peers.clone() {
                self.check_explicit_peer_connection(&p);
            }
        }

        // Cache the scores of all connected peers, and record metrics for current penalties.
        let mut scores = HashMap::with_capacity(self.connected_peers.len());
        if let Some((peer_score, ..)) = &self.peer_score {
            for peer_id in self.connected_peers.keys() {
                scores
                    .entry(peer_id)
                    .or_insert_with(|| peer_score.metric_score(peer_id, self.metrics.as_mut()));
            }
        }

        // maintain the mesh for each topic
        for (topic_hash, peers) in self.mesh.iter_mut() {
            let explicit_peers = &self.explicit_peers;
            let backoffs = &self.backoffs;
            let outbound_peers = &self.outbound_peers;

            // drop all peers with negative score, without PX
            // if there is at some point a stable retain method for BTreeSet the following can be
            // written more efficiently with retain.
            let mut to_remove_peers = Vec::new();
            for peer_id in peers.iter() {
                let peer_score = *scores.get(peer_id).unwrap_or(&0.0);

                // Record the score per mesh
                if let Some(metrics) = self.metrics.as_mut() {
                    metrics.observe_mesh_peers_score(topic_hash, peer_score);
                }

                if peer_score < 0.0 {
                    tracing::debug!(
                        peer=%peer_id,
                        score=%peer_score,
                        topic=%topic_hash,
                        "HEARTBEAT: Prune peer with negative score"
                    );

                    let current_topic = to_prune.entry(*peer_id).or_insert_with(Vec::new);
                    current_topic.push(topic_hash.clone());
                    no_px.insert(*peer_id);
                    to_remove_peers.push(*peer_id);
                }
            }

            if let Some(m) = self.metrics.as_mut() {
                m.peers_removed(topic_hash, Churn::BadScore, to_remove_peers.len())
            }

            for peer_id in to_remove_peers {
                peers.remove(&peer_id);
            }

            // too little peers - add some
            if peers.len() < self.config.mesh_n_low() {
                tracing::debug!(
                    topic=%topic_hash,
                    "HEARTBEAT: Mesh low. Topic contains: {} needs: {}",
                    peers.len(),
                    self.config.mesh_n_low()
                );
                // not enough peers - get mesh_n - current_length more
                let desired_peers = self.config.mesh_n() - peers.len();
                let peer_list =
                    get_random_peers(&self.connected_peers, topic_hash, desired_peers, |peer| {
                        !peers.contains(peer)
                            && !explicit_peers.contains(peer)
                            && !backoffs.is_backoff_with_slack(topic_hash, peer)
                            && *scores.get(peer).unwrap_or(&0.0) >= 0.0
                    });
                for peer in &peer_list {
                    let current_topic = to_graft.entry(*peer).or_insert_with(Vec::new);
                    current_topic.push(topic_hash.clone());
                }
                // update the mesh
                tracing::debug!("Updating mesh, new mesh: {:?}", peer_list);
                if let Some(m) = self.metrics.as_mut() {
                    m.peers_included(topic_hash, Inclusion::Random, peer_list.len())
                }
                peers.extend(peer_list);
            }

            // too many peers - remove some
            if peers.len() > self.config.mesh_n_high() {
                tracing::debug!(
                    topic=%topic_hash,
                    "HEARTBEAT: Mesh high. Topic contains: {} needs: {}",
                    peers.len(),
                    self.config.mesh_n_high()
                );
                let excess_peer_no = peers.len() - self.config.mesh_n();

                // shuffle the peers and then sort by score ascending beginning with the worst
                let mut rng = thread_rng();
                let mut shuffled = peers.iter().copied().collect::<Vec<_>>();
                shuffled.shuffle(&mut rng);
                shuffled.sort_by(|p1, p2| {
                    let score_p1 = *scores.get(p1).unwrap_or(&0.0);
                    let score_p2 = *scores.get(p2).unwrap_or(&0.0);

                    score_p1.partial_cmp(&score_p2).unwrap_or(Ordering::Equal)
                });
                // shuffle everything except the last retain_scores many peers (the best ones)
                shuffled[..peers.len() - self.config.retain_scores()].shuffle(&mut rng);

                // count total number of outbound peers
                let mut outbound = {
                    let outbound_peers = &self.outbound_peers;
                    shuffled
                        .iter()
                        .filter(|p| outbound_peers.contains(*p))
                        .count()
                };

                // remove the first excess_peer_no allowed (by outbound restrictions) peers adding
                // them to to_prune
                let mut removed = 0;
                for peer in shuffled {
                    if removed == excess_peer_no {
                        break;
                    }
                    if self.outbound_peers.contains(&peer) {
                        if outbound <= self.config.mesh_outbound_min() {
                            // do not remove anymore outbound peers
                            continue;
                        }
                        // an outbound peer gets removed
                        outbound -= 1;
                    }

                    // remove the peer
                    peers.remove(&peer);
                    let current_topic = to_prune.entry(peer).or_insert_with(Vec::new);
                    current_topic.push(topic_hash.clone());
                    removed += 1;
                }

                if let Some(m) = self.metrics.as_mut() {
                    m.peers_removed(topic_hash, Churn::Excess, removed)
                }
            }

            // do we have enough outbound peers?
            if peers.len() >= self.config.mesh_n_low() {
                // count number of outbound peers we have
                let outbound = { peers.iter().filter(|p| outbound_peers.contains(*p)).count() };

                // if we have not enough outbound peers, graft to some new outbound peers
                if outbound < self.config.mesh_outbound_min() {
                    let needed = self.config.mesh_outbound_min() - outbound;
                    let peer_list =
                        get_random_peers(&self.connected_peers, topic_hash, needed, |peer| {
                            !peers.contains(peer)
                                && !explicit_peers.contains(peer)
                                && !backoffs.is_backoff_with_slack(topic_hash, peer)
                                && *scores.get(peer).unwrap_or(&0.0) >= 0.0
                                && outbound_peers.contains(peer)
                        });
                    for peer in &peer_list {
                        let current_topic = to_graft.entry(*peer).or_insert_with(Vec::new);
                        current_topic.push(topic_hash.clone());
                    }
                    // update the mesh
                    tracing::debug!("Updating mesh, new mesh: {:?}", peer_list);
                    if let Some(m) = self.metrics.as_mut() {
                        m.peers_included(topic_hash, Inclusion::Outbound, peer_list.len())
                    }
                    peers.extend(peer_list);
                }
            }

            // should we try to improve the mesh with opportunistic grafting?
            if self.heartbeat_ticks % self.config.opportunistic_graft_ticks() == 0
                && peers.len() > 1
                && self.peer_score.is_some()
            {
                if let Some((_, thresholds, _, _)) = &self.peer_score {
                    // Opportunistic grafting works as follows: we check the median score of peers
                    // in the mesh; if this score is below the opportunisticGraftThreshold, we
                    // select a few peers at random with score over the median.
                    // The intention is to (slowly) improve an underperforming mesh by introducing
                    // good scoring peers that may have been gossiping at us. This allows us to
                    // get out of sticky situations where we are stuck with poor peers and also
                    // recover from churn of good peers.

                    // now compute the median peer score in the mesh
                    let mut peers_by_score: Vec<_> = peers.iter().collect();
                    peers_by_score.sort_by(|p1, p2| {
                        let p1_score = *scores.get(p1).unwrap_or(&0.0);
                        let p2_score = *scores.get(p2).unwrap_or(&0.0);
                        p1_score.partial_cmp(&p2_score).unwrap_or(Equal)
                    });

                    let middle = peers_by_score.len() / 2;
                    let median = if peers_by_score.len() % 2 == 0 {
                        let sub_middle_peer = *peers_by_score
                            .get(middle - 1)
                            .expect("middle < vector length and middle > 0 since peers.len() > 0");
                        let sub_middle_score = *scores.get(sub_middle_peer).unwrap_or(&0.0);
                        let middle_peer =
                            *peers_by_score.get(middle).expect("middle < vector length");
                        let middle_score = *scores.get(middle_peer).unwrap_or(&0.0);

                        (sub_middle_score + middle_score) * 0.5
                    } else {
                        *scores
                            .get(*peers_by_score.get(middle).expect("middle < vector length"))
                            .unwrap_or(&0.0)
                    };

                    // if the median score is below the threshold, select a better peer (if any) and
                    // GRAFT
                    if median < thresholds.opportunistic_graft_threshold {
                        let peer_list = get_random_peers(
                            &self.connected_peers,
                            topic_hash,
                            self.config.opportunistic_graft_peers(),
                            |peer_id| {
                                !peers.contains(peer_id)
                                    && !explicit_peers.contains(peer_id)
                                    && !backoffs.is_backoff_with_slack(topic_hash, peer_id)
                                    && *scores.get(peer_id).unwrap_or(&0.0) > median
                            },
                        );
                        for peer in &peer_list {
                            let current_topic = to_graft.entry(*peer).or_insert_with(Vec::new);
                            current_topic.push(topic_hash.clone());
                        }
                        // update the mesh
                        tracing::debug!(
                            topic=%topic_hash,
                            "Opportunistically graft in topic with peers {:?}",
                            peer_list
                        );
                        if let Some(m) = self.metrics.as_mut() {
                            m.peers_included(topic_hash, Inclusion::Random, peer_list.len())
                        }
                        peers.extend(peer_list);
                    }
                }
            }
            // Register the final count of peers in the mesh
            if let Some(m) = self.metrics.as_mut() {
                m.set_mesh_peers(topic_hash, peers.len())
            }
        }

        // remove expired fanout topics
        {
            let fanout = &mut self.fanout; // help the borrow checker
            let fanout_ttl = self.config.fanout_ttl();
            self.fanout_last_pub.retain(|topic_hash, last_pub_time| {
                if *last_pub_time + fanout_ttl < Instant::now() {
                    tracing::debug!(
                        topic=%topic_hash,
                        "HEARTBEAT: Fanout topic removed due to timeout"
                    );
                    fanout.remove(topic_hash);
                    return false;
                }
                true
            });
        }

        // maintain fanout
        // check if our peers are still a part of the topic
        for (topic_hash, peers) in self.fanout.iter_mut() {
            let mut to_remove_peers = Vec::new();
            let publish_threshold = match &self.peer_score {
                Some((_, thresholds, _, _)) => thresholds.publish_threshold,
                _ => 0.0,
            };
            for peer_id in peers.iter() {
                // is the peer still subscribed to the topic?
                let peer_score = *scores.get(peer_id).unwrap_or(&0.0);
                match self.connected_peers.get(peer_id) {
                    Some(peer) => {
                        if !peer.topics.contains(topic_hash) || peer_score < publish_threshold {
                            tracing::debug!(
                                topic=%topic_hash,
                                "HEARTBEAT: Peer removed from fanout for topic"
                            );
                            to_remove_peers.push(*peer_id);
                        }
                    }
                    None => {
                        // remove if the peer has disconnected
                        to_remove_peers.push(*peer_id);
                    }
                }
            }
            for to_remove in to_remove_peers {
                peers.remove(&to_remove);
            }

            // not enough peers
            if peers.len() < self.config.mesh_n() {
                tracing::debug!(
                    "HEARTBEAT: Fanout low. Contains: {:?} needs: {:?}",
                    peers.len(),
                    self.config.mesh_n()
                );
                let needed_peers = self.config.mesh_n() - peers.len();
                let explicit_peers = &self.explicit_peers;
                let new_peers =
                    get_random_peers(&self.connected_peers, topic_hash, needed_peers, |peer_id| {
                        !peers.contains(peer_id)
                            && !explicit_peers.contains(peer_id)
                            && *scores.get(peer_id).unwrap_or(&0.0) < publish_threshold
                    });
                peers.extend(new_peers);
            }
        }

        if self.peer_score.is_some() {
            tracing::trace!("Mesh message deliveries: {:?}", {
                self.mesh
                    .iter()
                    .map(|(t, peers)| {
                        (
                            t.clone(),
                            peers
                                .iter()
                                .map(|p| {
                                    (
                                        *p,
                                        self.peer_score
                                            .as_ref()
                                            .expect("peer_score.is_some()")
                                            .0
                                            .mesh_message_deliveries(p, t)
                                            .unwrap_or(0.0),
                                    )
                                })
                                .collect::<HashMap<PeerId, f64>>(),
                        )
                    })
                    .collect::<HashMap<TopicHash, HashMap<PeerId, f64>>>()
            })
        }

        self.emit_gossip();

        // send graft/prunes
        if !to_graft.is_empty() | !to_prune.is_empty() {
            self.send_graft_prune(to_graft, to_prune, no_px);
        }

        // shift the memcache
        self.mcache.shift();

        // Report expired messages
        for (peer_id, failed_messages) in self.failed_messages.drain() {
            tracing::debug!("Peer couldn't consume messages: {:?}", failed_messages);
            self.events
                .push_back(ToSwarm::GenerateEvent(Event::SlowPeer {
                    peer_id,
                    failed_messages,
                }));
        }
        self.failed_messages.shrink_to_fit();

        tracing::debug!("Completed Heartbeat");
        if let Some(metrics) = self.metrics.as_mut() {
            let duration = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            metrics.observe_heartbeat_duration(duration);
        }
    }

    /// Emits gossip - Send IHAVE messages to a random set of gossip peers. This is applied to mesh
    /// and fanout peers
    fn emit_gossip(&mut self) {
        let mut rng = thread_rng();
        let mut messages = Vec::new();
        for (topic_hash, peers) in self.mesh.iter().chain(self.fanout.iter()) {
            let mut message_ids = self.mcache.get_gossip_message_ids(topic_hash);
            if message_ids.is_empty() {
                continue;
            }

            // if we are emitting more than GossipSubMaxIHaveLength message_ids, truncate the list
            if message_ids.len() > self.config.max_ihave_length() {
                // we do the truncation (with shuffling) per peer below
                tracing::debug!(
                    "too many messages for gossip; will truncate IHAVE list ({} messages)",
                    message_ids.len()
                );
            } else {
                // shuffle to emit in random order
                message_ids.shuffle(&mut rng);
            }

            // dynamic number of peers to gossip based on `gossip_factor` with minimum `gossip_lazy`
            let n_map = |m| {
                max(
                    self.config.gossip_lazy(),
                    (self.config.gossip_factor() * m as f64) as usize,
                )
            };
            // get gossip_lazy random peers
            let to_msg_peers =
                get_random_peers_dynamic(&self.connected_peers, topic_hash, n_map, |peer| {
                    !peers.contains(peer)
                        && !self.explicit_peers.contains(peer)
                        && !self.score_below_threshold(peer, |ts| ts.gossip_threshold).0
                });

            tracing::debug!("Gossiping IHAVE to {} peers", to_msg_peers.len());

            for peer_id in to_msg_peers {
                let mut peer_message_ids = message_ids.clone();

                if peer_message_ids.len() > self.config.max_ihave_length() {
                    // We do this per peer so that we emit a different set for each peer.
                    // we have enough redundancy in the system that this will significantly increase
                    // the message coverage when we do truncate.
                    peer_message_ids.partial_shuffle(&mut rng, self.config.max_ihave_length());
                    peer_message_ids.truncate(self.config.max_ihave_length());
                }

                // send an IHAVE message
                messages.push((
                    peer_id,
                    RpcOut::IHave(IHave {
                        topic_hash: topic_hash.clone(),
                        message_ids: peer_message_ids,
                    }),
                ));
            }
        }
        for (peer_id, message) in messages {
            self.send_message(peer_id, message);
        }
    }

    /// Handles multiple GRAFT/PRUNE messages and coalesces them into chunked gossip control
    /// messages.
    fn send_graft_prune(
        &mut self,
        to_graft: HashMap<PeerId, Vec<TopicHash>>,
        mut to_prune: HashMap<PeerId, Vec<TopicHash>>,
        no_px: HashSet<PeerId>,
    ) {
        // handle the grafts and overlapping prunes per peer
        for (peer_id, topics) in to_graft.into_iter() {
            for topic in &topics {
                // inform scoring of graft
                if let Some((peer_score, ..)) = &mut self.peer_score {
                    peer_score.graft(&peer_id, topic.clone());
                }

                // inform the handler of the peer being added to the mesh
                // If the peer did not previously exist in any mesh, inform the handler
                peer_added_to_mesh(
                    peer_id,
                    vec![topic],
                    &self.mesh,
                    &mut self.events,
                    &self.connected_peers,
                );
            }
            let rpc_msgs = topics.iter().map(|topic_hash| {
                RpcOut::Graft(Graft {
                    topic_hash: topic_hash.clone(),
                })
            });

            // If there are prunes associated with the same peer add them.
            // NOTE: In this case a peer has been added to a topic mesh, and removed from another.
            // It therefore must be in at least one mesh and we do not need to inform the handler
            // of its removal from another.

            // The following prunes are not due to unsubscribing.
            let prune_msgs = to_prune
                .remove(&peer_id)
                .into_iter()
                .flatten()
                .map(|topic_hash| {
                    let prune = self.make_prune(
                        &topic_hash,
                        &peer_id,
                        self.config.do_px() && !no_px.contains(&peer_id),
                        false,
                    );
                    RpcOut::Prune(prune)
                });

            // send the rpc messages
            for msg in rpc_msgs.chain(prune_msgs).collect::<Vec<_>>() {
                self.send_message(peer_id, msg);
            }
        }

        // handle the remaining prunes
        // The following prunes are not due to unsubscribing.
        for (peer_id, topics) in to_prune.iter() {
            for topic_hash in topics {
                let prune = self.make_prune(
                    topic_hash,
                    peer_id,
                    self.config.do_px() && !no_px.contains(peer_id),
                    false,
                );
                self.send_message(*peer_id, RpcOut::Prune(prune));

                // inform the handler
                peer_removed_from_mesh(
                    *peer_id,
                    topic_hash,
                    &self.mesh,
                    &mut self.events,
                    &self.connected_peers,
                );
            }
        }
    }

    /// Helper function which forwards a message to mesh\[topic\] peers.
    ///
    /// Returns true if at least one peer was messaged.
    fn forward_msg(
        &mut self,
        msg_id: &MessageId,
        message: RawMessage,
        propagation_source: Option<&PeerId>,
        originating_peers: HashSet<PeerId>,
    ) -> bool {
        // message is fully validated inform peer_score
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(peer) = propagation_source {
                peer_score.deliver_message(peer, msg_id, &message.topic);
            }
        }

        tracing::debug!(message=%msg_id, "Forwarding message");
        let mut recipient_peers = HashSet::new();

        // Populate the recipient peers mapping

        // Add explicit peers
        for peer_id in &self.explicit_peers {
            let Some(peer) = self.connected_peers.get(peer_id) else {
                continue;
            };
            if Some(peer_id) != propagation_source
                && !originating_peers.contains(peer_id)
                && Some(peer_id) != message.source.as_ref()
                && peer.topics.contains(&message.topic)
            {
                recipient_peers.insert(*peer_id);
            }
        }

        // add mesh peers
        let topic = &message.topic;
        // mesh
        if let Some(mesh_peers) = self.mesh.get(topic) {
            for peer_id in mesh_peers {
                if Some(peer_id) != propagation_source
                    && !originating_peers.contains(peer_id)
                    && Some(peer_id) != message.source.as_ref()
                {
                    recipient_peers.insert(*peer_id);
                }
            }
        }

        if recipient_peers.is_empty() {
            return false;
        }

        // forward the message to peers
        for peer in recipient_peers.iter() {
            let event = RpcOut::Forward {
                message: message.clone(),
                timeout: Delay::new(self.config.forward_queue_duration()),
            };
            tracing::debug!(%peer, message=%msg_id, "Sending message to peer");
            self.send_message(*peer, event);
        }
        tracing::debug!("Completed forwarding message");
        true
    }

    /// Constructs a [`RawMessage`] performing message signing if required.
    pub(crate) fn build_raw_message(
        &mut self,
        topic: TopicHash,
        data: Vec<u8>,
    ) -> Result<RawMessage, PublishError> {
        match &mut self.publish_config {
            PublishConfig::Signing {
                ref keypair,
                author,
                inline_key,
                last_seq_no,
            } => {
                let sequence_number = last_seq_no.next();

                let signature = {
                    let message = proto::Message {
                        from: Some(author.to_bytes()),
                        data: Some(data.clone()),
                        seqno: Some(sequence_number.to_be_bytes().to_vec()),
                        topic: topic.clone().into_string(),
                        signature: None,
                        key: None,
                    };

                    let mut buf = Vec::with_capacity(message.get_size());
                    let mut writer = Writer::new(&mut buf);

                    message
                        .write_message(&mut writer)
                        .expect("Encoding to succeed");

                    // the signature is over the bytes "libp2p-pubsub:<protobuf-message>"
                    let mut signature_bytes = SIGNING_PREFIX.to_vec();
                    signature_bytes.extend_from_slice(&buf);
                    Some(keypair.sign(&signature_bytes)?)
                };

                Ok(RawMessage {
                    source: Some(*author),
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: Some(sequence_number),
                    topic,
                    signature,
                    key: inline_key.clone(),
                    validated: true, // all published messages are valid
                })
            }
            PublishConfig::Author(peer_id) => {
                Ok(RawMessage {
                    source: Some(*peer_id),
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: Some(rand::random()),
                    topic,
                    signature: None,
                    key: None,
                    validated: true, // all published messages are valid
                })
            }
            PublishConfig::RandomAuthor => {
                Ok(RawMessage {
                    source: Some(PeerId::random()),
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: Some(rand::random()),
                    topic,
                    signature: None,
                    key: None,
                    validated: true, // all published messages are valid
                })
            }
            PublishConfig::Anonymous => {
                Ok(RawMessage {
                    source: None,
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: None,
                    topic,
                    signature: None,
                    key: None,
                    validated: true, // all published messages are valid
                })
            }
        }
    }

    /// Send a [`RpcOut`] message to a peer.
    ///
    /// Returns `true` if sending was successful, `false` otherwise.
    /// The method will update the peer score and failed message counter if
    /// sending the message failed due to the channel to the connection handler being
    /// full (which indicates a slow peer).
    fn send_message(&mut self, peer_id: PeerId, rpc: RpcOut) -> bool {
        if let Some(m) = self.metrics.as_mut() {
            if let RpcOut::Publish { ref message, .. } | RpcOut::Forward { ref message, .. } = rpc {
                // register bytes sent on the internal metrics.
                m.msg_sent(&message.topic, message.raw_protobuf_len());
            }
        }

        let Some(peer) = &mut self.connected_peers.get_mut(&peer_id) else {
            tracing::error!(peer = %peer_id,
                    "Could not send rpc to connection handler, peer doesn't exist in connected peer list");
            return false;
        };

        // Try sending the message to the connection handler.
        match peer.sender.send_message(rpc) {
            Ok(()) => true,
            Err(rpc) => {
                // Sending failed because the channel is full.
                tracing::warn!(peer=%peer_id, "Send Queue full. Could not send {:?}.", rpc);

                // Update failed message counter.
                let failed_messages = self.failed_messages.entry(peer_id).or_default();
                match rpc {
                    RpcOut::Publish { .. } => {
                        failed_messages.priority += 1;
                        failed_messages.publish += 1;
                    }
                    RpcOut::Forward { .. } => {
                        failed_messages.non_priority += 1;
                        failed_messages.forward += 1;
                    }
                    RpcOut::IWant(_) | RpcOut::IHave(_) => {
                        failed_messages.non_priority += 1;
                    }
                    RpcOut::Graft(_)
                    | RpcOut::Prune(_)
                    | RpcOut::Subscribe(_)
                    | RpcOut::Unsubscribe(_) => {
                        unreachable!("Channel for highpriority control messages is unbounded and should always be open.")
                    }
                }

                // Update peer score.
                if let Some((peer_score, ..)) = &mut self.peer_score {
                    peer_score.failed_message_slow_peer(&peer_id);
                }

                false
            }
        }
    }

    fn on_connection_established(
        &mut self,
        ConnectionEstablished {
            peer_id,
            endpoint,
            other_established,
            ..
        }: ConnectionEstablished,
    ) {
        // Diverging from the go implementation we only want to consider a peer as outbound peer
        // if its first connection is outbound.

        if endpoint.is_dialer() && other_established == 0 && !self.px_peers.contains(&peer_id) {
            // The first connection is outbound and it is not a peer from peer exchange => mark
            // it as outbound peer
            self.outbound_peers.insert(peer_id);
        }

        // Add the IP to the peer scoring system
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(ip) = get_ip_addr(endpoint.get_remote_address()) {
                peer_score.add_ip(&peer_id, ip);
            } else {
                tracing::trace!(
                    peer=%peer_id,
                    "Couldn't extract ip from endpoint of peer with endpoint {:?}",
                    endpoint
                )
            }
        }

        if other_established > 0 {
            return; // Not our first connection to this peer, hence nothing to do.
        }

        if let Some((peer_score, ..)) = &mut self.peer_score {
            peer_score.add_peer(peer_id);
        }

        // Ignore connections from blacklisted peers.
        if self.blacklisted_peers.contains(&peer_id) {
            tracing::debug!(peer=%peer_id, "Ignoring connection from blacklisted peer");
            return;
        }

        tracing::debug!(peer=%peer_id, "New peer connected");
        // We need to send our subscriptions to the newly-connected node.
        for topic_hash in self.mesh.clone().into_keys() {
            self.send_message(peer_id, RpcOut::Subscribe(topic_hash));
        }
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            connection_id,
            endpoint,
            remaining_established,
            ..
        }: ConnectionClosed,
    ) {
        // Remove IP from peer scoring system
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(ip) = get_ip_addr(endpoint.get_remote_address()) {
                peer_score.remove_ip(&peer_id, &ip);
            } else {
                tracing::trace!(
                    peer=%peer_id,
                    "Couldn't extract ip from endpoint of peer with endpoint {:?}",
                    endpoint
                )
            }
        }

        if remaining_established != 0 {
            // Remove the connection from the list
            if let Some(peer) = self.connected_peers.get_mut(&peer_id) {
                let index = peer
                    .connections
                    .iter()
                    .position(|v| v == &connection_id)
                    .expect("Previously established connection to peer must be present");
                peer.connections.remove(index);

                // If there are more connections and this peer is in a mesh, inform the first
                // connection handler.
                if !peer.connections.is_empty() {
                    for topic in &peer.topics {
                        if let Some(mesh_peers) = self.mesh.get(topic) {
                            if mesh_peers.contains(&peer_id) {
                                self.events.push_back(ToSwarm::NotifyHandler {
                                    peer_id,
                                    event: HandlerIn::JoinedMesh,
                                    handler: NotifyHandler::One(peer.connections[0]),
                                });
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            // remove from mesh, topic_peers, peer_topic and the fanout
            tracing::debug!(peer=%peer_id, "Peer disconnected");
            let Some(connected_peer) = self.connected_peers.get(&peer_id) else {
                tracing::error!(peer_id = %peer_id, "Peer non-existent when handling disconnection");
                return;
            };

            // remove peer from all mappings
            for topic in &connected_peer.topics {
                // check the mesh for the topic
                if let Some(mesh_peers) = self.mesh.get_mut(topic) {
                    // check if the peer is in the mesh and remove it
                    if mesh_peers.remove(&peer_id) {
                        if let Some(m) = self.metrics.as_mut() {
                            m.peers_removed(topic, Churn::Dc, 1);
                            m.set_mesh_peers(topic, mesh_peers.len());
                        }
                    };
                }

                if let Some(m) = self.metrics.as_mut() {
                    m.dec_topic_peers(topic);
                }

                // remove from fanout
                self.fanout
                    .get_mut(topic)
                    .map(|peers| peers.remove(&peer_id));
            }

            // Forget px and outbound status for this peer
            self.px_peers.remove(&peer_id);
            self.outbound_peers.remove(&peer_id);

            // If metrics are enabled, register the disconnection of a peer based on its protocol.
            if let Some(metrics) = self.metrics.as_mut() {
                metrics.peer_protocol_disconnected(connected_peer.kind.clone());
            }

            self.connected_peers.remove(&peer_id);

            if let Some((peer_score, ..)) = &mut self.peer_score {
                peer_score.remove_peer(&peer_id);
            }
        }
    }

    fn on_address_change(
        &mut self,
        AddressChange {
            peer_id,
            old: endpoint_old,
            new: endpoint_new,
            ..
        }: AddressChange,
    ) {
        // Exchange IP in peer scoring system
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(ip) = get_ip_addr(endpoint_old.get_remote_address()) {
                peer_score.remove_ip(&peer_id, &ip);
            } else {
                tracing::trace!(
                    peer=%&peer_id,
                    "Couldn't extract ip from endpoint of peer with endpoint {:?}",
                    endpoint_old
                )
            }
            if let Some(ip) = get_ip_addr(endpoint_new.get_remote_address()) {
                peer_score.add_ip(&peer_id, ip);
            } else {
                tracing::trace!(
                    peer=%peer_id,
                    "Couldn't extract ip from endpoint of peer with endpoint {:?}",
                    endpoint_new
                )
            }
        }
    }
}

fn get_ip_addr(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|p| match p {
        Ip4(addr) => Some(IpAddr::V4(addr)),
        Ip6(addr) => Some(IpAddr::V6(addr)),
        _ => None,
    })
}

impl<C, F> NetworkBehaviour for Behaviour<C, F>
where
    C: Send + 'static + DataTransform,
    F: Send + 'static + TopicSubscriptionFilter,
{
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        // By default we assume a peer is only a floodsub peer.
        //
        // The protocol negotiation occurs once a message is sent/received. Once this happens we
        // update the type of peer that this is in order to determine which kind of routing should
        // occur.
        let connected_peer = self
            .connected_peers
            .entry(peer_id)
            .or_insert(PeerConnections {
                kind: PeerKind::Floodsub,
                connections: vec![],
                sender: Sender::new(self.config.connection_handler_queue_len()),
                topics: Default::default(),
            });
        // Add the new connection
        connected_peer.connections.push(connection_id);

        Ok(Handler::new(
            self.config.protocol_config(),
            connected_peer.sender.new_receiver(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let connected_peer = self
            .connected_peers
            .entry(peer_id)
            .or_insert(PeerConnections {
                kind: PeerKind::Floodsub,
                connections: vec![],
                sender: Sender::new(self.config.connection_handler_queue_len()),
                topics: Default::default(),
            });
        // Add the new connection
        connected_peer.connections.push(connection_id);

        Ok(Handler::new(
            self.config.protocol_config(),
            connected_peer.sender.new_receiver(),
        ))
    }

    fn on_connection_handler_event(
        &mut self,
        propagation_source: PeerId,
        _connection_id: ConnectionId,
        handler_event: THandlerOutEvent<Self>,
    ) {
        match handler_event {
            HandlerEvent::PeerKind(kind) => {
                // We have identified the protocol this peer is using

                if let Some(metrics) = self.metrics.as_mut() {
                    metrics.peer_protocol_connected(kind.clone());
                }

                if let PeerKind::NotSupported = kind {
                    tracing::debug!(
                        peer=%propagation_source,
                        "Peer does not support gossipsub protocols"
                    );
                    self.events
                        .push_back(ToSwarm::GenerateEvent(Event::GossipsubNotSupported {
                            peer_id: propagation_source,
                        }));
                } else if let Some(conn) = self.connected_peers.get_mut(&propagation_source) {
                    // Only change the value if the old value is Floodsub (the default set in
                    // `NetworkBehaviour::on_event` with FromSwarm::ConnectionEstablished).
                    // All other PeerKind changes are ignored.
                    tracing::debug!(
                        peer=%propagation_source,
                        peer_type=%kind,
                        "New peer type found for peer"
                    );
                    if let PeerKind::Floodsub = conn.kind {
                        conn.kind = kind;
                    }
                }
            }
            HandlerEvent::MessageDropped(rpc) => {
                // Account for this in the scoring logic
                if let Some((peer_score, _, _, _)) = &mut self.peer_score {
                    peer_score.failed_message_slow_peer(&propagation_source);
                }

                // Keep track of expired messages for the application layer.
                let failed_messages = self.failed_messages.entry(propagation_source).or_default();
                failed_messages.timeout += 1;
                match rpc {
                    RpcOut::Publish { .. } => {
                        failed_messages.publish += 1;
                    }
                    RpcOut::Forward { .. } => {
                        failed_messages.forward += 1;
                    }
                    _ => {}
                }

                // Record metrics on the failure.
                if let Some(metrics) = self.metrics.as_mut() {
                    match rpc {
                        RpcOut::Publish { message, .. } => {
                            metrics.publish_msg_dropped(&message.topic);
                            metrics.timeout_msg_dropped(&message.topic);
                        }
                        RpcOut::Forward { message, .. } => {
                            metrics.forward_msg_dropped(&message.topic);
                            metrics.timeout_msg_dropped(&message.topic);
                        }
                        _ => {}
                    }
                }
            }
            HandlerEvent::Message {
                rpc,
                invalid_messages,
            } => {
                // Handle the gossipsub RPC

                // Handle subscriptions
                // Update connected peers topics
                if !rpc.subscriptions.is_empty() {
                    self.handle_received_subscriptions(&rpc.subscriptions, &propagation_source);
                }

                // Check if peer is graylisted in which case we ignore the event
                if let (true, _) =
                    self.score_below_threshold(&propagation_source, |pst| pst.graylist_threshold)
                {
                    tracing::debug!(peer=%propagation_source, "RPC Dropped from greylisted peer");
                    return;
                }

                // Handle any invalid messages from this peer
                if self.peer_score.is_some() {
                    for (raw_message, validation_error) in invalid_messages {
                        self.handle_invalid_message(
                            &propagation_source,
                            &raw_message,
                            RejectReason::ValidationError(validation_error),
                        )
                    }
                } else {
                    // log the invalid messages
                    for (message, validation_error) in invalid_messages {
                        tracing::warn!(
                            peer=%propagation_source,
                            source=?message.source,
                            "Invalid message from peer. Reason: {:?}",
                            validation_error,
                        );
                    }
                }

                // Handle messages
                for (count, raw_message) in rpc.messages.into_iter().enumerate() {
                    // Only process the amount of messages the configuration allows.
                    if self.config.max_messages_per_rpc().is_some()
                        && Some(count) >= self.config.max_messages_per_rpc()
                    {
                        tracing::warn!("Received more messages than permitted. Ignoring further messages. Processed: {}", count);
                        break;
                    }
                    self.handle_received_message(raw_message, &propagation_source);
                }

                // Handle control messages
                // group some control messages, this minimises SendEvents (code is simplified to
                // handle each event at a time however)
                let mut ihave_msgs = vec![];
                let mut graft_msgs = vec![];
                let mut prune_msgs = vec![];
                for control_msg in rpc.control_msgs {
                    match control_msg {
                        ControlAction::IHave(IHave {
                            topic_hash,
                            message_ids,
                        }) => {
                            ihave_msgs.push((topic_hash, message_ids));
                        }
                        ControlAction::IWant(IWant { message_ids }) => {
                            self.handle_iwant(&propagation_source, message_ids)
                        }
                        ControlAction::Graft(Graft { topic_hash }) => graft_msgs.push(topic_hash),
                        ControlAction::Prune(Prune {
                            topic_hash,
                            peers,
                            backoff,
                        }) => prune_msgs.push((topic_hash, peers, backoff)),
                    }
                }
                if !ihave_msgs.is_empty() {
                    self.handle_ihave(&propagation_source, ihave_msgs);
                }
                if !graft_msgs.is_empty() {
                    self.handle_graft(&propagation_source, graft_msgs);
                }
                if !prune_msgs.is_empty() {
                    self.handle_prune(&propagation_source, prune_msgs);
                }
            }
        }
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        // update scores
        if let Some((peer_score, _, delay, _)) = &mut self.peer_score {
            if delay.poll_unpin(cx).is_ready() {
                peer_score.refresh_scores();
                delay.reset(peer_score.params.decay_interval);
            }
        }

        if self.heartbeat.poll_unpin(cx).is_ready() {
            self.heartbeat();
            self.heartbeat.reset(self.config.heartbeat_interval());
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                self.on_connection_established(connection_established)
            }
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.on_connection_closed(connection_closed)
            }
            FromSwarm::AddressChange(address_change) => self.on_address_change(address_change),
            _ => {}
        }
    }
}

/// This is called when peers are added to any mesh. It checks if the peer existed
/// in any other mesh. If this is the first mesh they have joined, it queues a message to notify
/// the appropriate connection handler to maintain a connection.
fn peer_added_to_mesh(
    peer_id: PeerId,
    new_topics: Vec<&TopicHash>,
    mesh: &HashMap<TopicHash, BTreeSet<PeerId>>,
    events: &mut VecDeque<ToSwarm<Event, HandlerIn>>,
    connections: &HashMap<PeerId, PeerConnections>,
) {
    // Ensure there is an active connection
    let connection_id = match connections.get(&peer_id) {
        Some(p) => p
            .connections
            .first()
            .expect("There should be at least one connection to a peer."),
        None => {
            tracing::error!(peer_id=%peer_id, "Peer not existent when added to the mesh");
            return;
        }
    };

    if let Some(peer) = connections.get(&peer_id) {
        for topic in &peer.topics {
            if !new_topics.contains(&topic) {
                if let Some(mesh_peers) = mesh.get(topic) {
                    if mesh_peers.contains(&peer_id) {
                        // the peer is already in a mesh for another topic
                        return;
                    }
                }
            }
        }
    }
    // This is the first mesh the peer has joined, inform the handler
    events.push_back(ToSwarm::NotifyHandler {
        peer_id,
        event: HandlerIn::JoinedMesh,
        handler: NotifyHandler::One(*connection_id),
    });
}

/// This is called when peers are removed from a mesh. It checks if the peer exists
/// in any other mesh. If this is the last mesh they have joined, we return true, in order to
/// notify the handler to no longer maintain a connection.
fn peer_removed_from_mesh(
    peer_id: PeerId,
    old_topic: &TopicHash,
    mesh: &HashMap<TopicHash, BTreeSet<PeerId>>,
    events: &mut VecDeque<ToSwarm<Event, HandlerIn>>,
    connections: &HashMap<PeerId, PeerConnections>,
) {
    // Ensure there is an active connection
    let connection_id = match connections.get(&peer_id) {
        Some(p) => p
            .connections
            .first()
            .expect("There should be at least one connection to a peer."),
        None => {
            tracing::error!(peer_id=%peer_id, "Peer not existent when removed from mesh");
            return;
        }
    };

    if let Some(peer) = connections.get(&peer_id) {
        for topic in &peer.topics {
            if topic != old_topic {
                if let Some(mesh_peers) = mesh.get(topic) {
                    if mesh_peers.contains(&peer_id) {
                        // the peer exists in another mesh still
                        return;
                    }
                }
            }
        }
    }
    // The peer is not in any other mesh, inform the handler
    events.push_back(ToSwarm::NotifyHandler {
        peer_id,
        event: HandlerIn::LeftMesh,
        handler: NotifyHandler::One(*connection_id),
    });
}

/// Helper function to get a subset of random gossipsub peers for a `topic_hash`
/// filtered by the function `f`. The number of peers to get equals the output of `n_map`
/// that gets as input the number of filtered peers.
fn get_random_peers_dynamic(
    connected_peers: &HashMap<PeerId, PeerConnections>,
    topic_hash: &TopicHash,
    // maps the number of total peers to the number of selected peers
    n_map: impl Fn(usize) -> usize,
    mut f: impl FnMut(&PeerId) -> bool,
) -> BTreeSet<PeerId> {
    let mut gossip_peers = connected_peers
        .iter()
        .filter(|(_, p)| p.topics.contains(topic_hash))
        .filter(|(peer_id, _)| f(peer_id))
        .filter(|(_, p)| p.kind == PeerKind::Gossipsub || p.kind == PeerKind::Gossipsubv1_1)
        .map(|(peer_id, _)| *peer_id)
        .collect::<Vec<PeerId>>();

    // if we have less than needed, return them
    let n = n_map(gossip_peers.len());
    if gossip_peers.len() <= n {
        tracing::debug!("RANDOM PEERS: Got {:?} peers", gossip_peers.len());
        return gossip_peers.into_iter().collect();
    }

    // we have more peers than needed, shuffle them and return n of them
    let mut rng = thread_rng();
    gossip_peers.partial_shuffle(&mut rng, n);

    tracing::debug!("RANDOM PEERS: Got {:?} peers", n);

    gossip_peers.into_iter().take(n).collect()
}

/// Helper function to get a set of `n` random gossipsub peers for a `topic_hash`
/// filtered by the function `f`.
fn get_random_peers(
    connected_peers: &HashMap<PeerId, PeerConnections>,
    topic_hash: &TopicHash,
    n: usize,
    f: impl FnMut(&PeerId) -> bool,
) -> BTreeSet<PeerId> {
    get_random_peers_dynamic(connected_peers, topic_hash, |_| n, f)
}

/// Validates the combination of signing, privacy and message validation to ensure the
/// configuration will not reject published messages.
fn validate_config(
    authenticity: &MessageAuthenticity,
    validation_mode: &ValidationMode,
) -> Result<(), &'static str> {
    match validation_mode {
        ValidationMode::Anonymous => {
            if authenticity.is_signing() {
                return Err("Cannot enable message signing with an Anonymous validation mode. Consider changing either the ValidationMode or MessageAuthenticity");
            }

            if !authenticity.is_anonymous() {
                return Err("Published messages contain an author but incoming messages with an author will be rejected. Consider adjusting the validation or privacy settings in the config");
            }
        }
        ValidationMode::Strict => {
            if !authenticity.is_signing() {
                return Err(
                    "Messages will be
                published unsigned and incoming unsigned messages will be rejected. Consider adjusting
                the validation or privacy settings in the config"
                );
            }
        }
        _ => {}
    }
    Ok(())
}

impl<C: DataTransform, F: TopicSubscriptionFilter> fmt::Debug for Behaviour<C, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Behaviour")
            .field("config", &self.config)
            .field("events", &self.events.len())
            .field("publish_config", &self.publish_config)
            .field("mesh", &self.mesh)
            .field("fanout", &self.fanout)
            .field("fanout_last_pub", &self.fanout_last_pub)
            .field("mcache", &self.mcache)
            .field("heartbeat", &self.heartbeat)
            .finish()
    }
}

impl fmt::Debug for PublishConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PublishConfig::Signing { author, .. } => {
                f.write_fmt(format_args!("PublishConfig::Signing({author})"))
            }
            PublishConfig::Author(author) => {
                f.write_fmt(format_args!("PublishConfig::Author({author})"))
            }
            PublishConfig::RandomAuthor => f.write_fmt(format_args!("PublishConfig::RandomAuthor")),
            PublishConfig::Anonymous => f.write_fmt(format_args!("PublishConfig::Anonymous")),
        }
    }
}
