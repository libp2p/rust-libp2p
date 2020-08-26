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

use std::cmp::{max, Ordering};
use std::collections::hash_map::Entry;
use std::iter::FromIterator;
use std::net::IpAddr;
use std::time::Duration;
use std::{
    collections::HashSet,
    collections::VecDeque,
    collections::{hash_map::HashMap, BTreeSet},
    fmt, iter,
    sync::Arc,
    task::{Context, Poll},
};

use futures::StreamExt;
use log::{debug, error, info, trace, warn};
use prost::Message;
use rand;
use rand::{seq::SliceRandom, thread_rng};
use wasm_timer::{Instant, Interval};

use libp2p_core::{
    connection::ConnectionId, identity::error::SigningError, identity::Keypair,
    multiaddr::Protocol::Ip4, multiaddr::Protocol::Ip6, ConnectedPoint, Multiaddr, PeerId,
};
use libp2p_swarm::{
    DialPeerCondition, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters,
    ProtocolsHandler,
};

use crate::config::{GossipsubConfig, ValidationMode};
use crate::error::PublishError;
use crate::gossip_promises::GossipPromises;
use crate::handler::{GossipsubHandler, HandlerEvent};
use crate::mcache::MessageCache;
use crate::peer_score::{PeerScore, PeerScoreParams, PeerScoreThresholds, RejectReason};
use crate::protocol::SIGNING_PREFIX;
use crate::time_cache::DuplicateCache;
use crate::topic::{Hasher, Topic, TopicHash};
use crate::types::{
    GossipsubControlAction, GossipsubMessage, GossipsubSubscription, GossipsubSubscriptionAction,
    MessageAcceptance, MessageId, PeerInfo,
};
use crate::types::{GossipsubRpc, PeerKind};
use crate::{rpc_proto, TopicScoreParams};
use std::cmp::Ordering::Equal;

mod tests;

/// Determines if published messages should be signed or not.
///
/// Without signing, a number of privacy preserving modes can be selected.
///
/// NOTE: The default validation settings are to require signatures. The [`ValidationMode`]
/// should be updated in the [`GossipsubConfig`] to allow for unsigned messages.
#[derive(Clone)]
pub enum MessageAuthenticity {
    /// Message signing is enabled. The author will be the owner of the key and the sequence number
    /// will be a random number.
    Signed(Keypair),
    /// Message signing is disabled.
    ///
    /// The specified `PeerId` will be used as the author of all published messages. The sequence
    /// number will be randomized.
    Author(PeerId),
    /// Message signing is disabled.
    ///
    /// A random `PeerId` will be used when publishing each message. The sequence number will be
    /// randomized.
    RandomAuthor,
    /// Message signing is disabled.
    ///
    /// The author of the message and the sequence numbers are excluded from the message.
    ///
    /// NOTE: Excluding these fields may make these messages invalid by other nodes who
    /// enforce validation of these fields. See [`ValidationMode`] in the `GossipsubConfig`
    /// for how to customise this for rust-libp2p gossipsub.  A custom `message_id`
    /// function will need to be set to prevent all messages from a peer being filtered
    /// as duplicates.
    Anonymous,
}

impl MessageAuthenticity {
    /// Returns true if signing is enabled.
    pub fn is_signing(&self) -> bool {
        match self {
            MessageAuthenticity::Signed(_) => true,
            _ => false,
        }
    }

    pub fn is_anonymous(&self) -> bool {
        match self {
            MessageAuthenticity::Anonymous => true,
            _ => false,
        }
    }
}

/// Event that can happen on the gossipsub behaviour.
#[derive(Debug)]
pub enum GossipsubEvent {
    /// A message has been received.    
    Message {
        /// The peer that forwarded us this message.
        propagation_source: PeerId,
        /// The `MessageId` of the message. This should be referenced by the application when
        /// validating a message (if required).
        message_id: MessageId,
        /// The message itself.
        message: GossipsubMessage,
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
}

/// A data structure for storing configuration for publishing messages. See [`MessageAuthenticity`]
/// for further details.
enum PublishConfig {
    Signing {
        keypair: Keypair,
        author: PeerId,
        inline_key: Option<Vec<u8>>,
    },
    Author(PeerId),
    RandomAuthor,
    Anonymous,
}

impl PublishConfig {
    pub fn get_own_id(&self) -> Option<&PeerId> {
        match self {
            Self::Signing { author, .. } => Some(&author),
            Self::Author(author) => Some(&author),
            _ => None,
        }
    }
}

impl From<MessageAuthenticity> for PublishConfig {
    fn from(authenticity: MessageAuthenticity) -> Self {
        match authenticity {
            MessageAuthenticity::Signed(keypair) => {
                let public_key = keypair.public();
                let key_enc = public_key.clone().into_protobuf_encoding();
                let key = if key_enc.len() <= 42 {
                    // The public key can be inlined in [`rpc_proto::Message::from`], so we don't include it
                    // specifically in the [`rpc_proto::Message::key`] field.
                    None
                } else {
                    // Include the protobuf encoding of the public key in the message.
                    Some(key_enc)
                };

                PublishConfig::Signing {
                    keypair,
                    author: public_key.into_peer_id(),
                    inline_key: key,
                }
            }
            MessageAuthenticity::Author(peer_id) => PublishConfig::Author(peer_id),
            MessageAuthenticity::RandomAuthor => PublishConfig::RandomAuthor,
            MessageAuthenticity::Anonymous => PublishConfig::Anonymous,
        }
    }
}

/// Stores backoffs in an efficient manner.
struct BackoffStorage {
    /// Stores backoffs and the index in backoffs_by_heartbeat per peer per topic.
    backoffs: HashMap<TopicHash, HashMap<PeerId, (Instant, usize)>>,
    /// Stores peer topic pairs per heartbeat (this is cyclic the current index is
    /// heartbeat_index).
    backoffs_by_heartbeat: Vec<HashSet<(TopicHash, PeerId)>>,
    /// The index in the backoffs_by_heartbeat vector corresponding to the current heartbeat.
    heartbeat_index: usize,
    /// The heartbeat interval duration from the config.
    heartbeat_interval: Duration,
    /// Backoff slack from the config.
    backoff_slack: u32,
}

impl BackoffStorage {
    fn heartbeats(d: &Duration, heartbeat_interval: &Duration) -> usize {
        ((d.as_nanos() + heartbeat_interval.as_nanos() - 1) / heartbeat_interval.as_nanos())
            as usize
    }

    pub fn new(
        prune_backoff: &Duration,
        heartbeat_interval: Duration,
        backoff_slack: u32,
    ) -> BackoffStorage {
        // We add one additional slot for partial heartbeat
        let max_heartbeats =
            Self::heartbeats(prune_backoff, &heartbeat_interval) + backoff_slack as usize + 1;
        BackoffStorage {
            backoffs: HashMap::new(),
            backoffs_by_heartbeat: vec![HashSet::new(); max_heartbeats],
            heartbeat_index: 0,
            heartbeat_interval,
            backoff_slack,
        }
    }

    /// Updates the backoff for a peer (if there is already a more restrictive backoff than this call
    /// doesn't change anything).
    pub fn update_backoff(&mut self, topic: &TopicHash, peer: &PeerId, time: Duration) {
        let instant = Instant::now() + time;
        let insert_into_backoffs_by_heartbeat =
            |heartbeat_index: usize,
             backoffs_by_heartbeat: &mut Vec<HashSet<_>>,
             heartbeat_interval,
             backoff_slack| {
                let pair = (topic.clone(), peer.clone());
                let index = (heartbeat_index
                    + Self::heartbeats(&time, heartbeat_interval)
                    + backoff_slack as usize)
                    % backoffs_by_heartbeat.len();
                backoffs_by_heartbeat[index].insert(pair);
                index
            };
        match self
            .backoffs
            .entry(topic.clone())
            .or_insert_with(HashMap::new)
            .entry(peer.clone())
        {
            Entry::Occupied(mut o) => {
                let &(backoff, index) = o.get();
                if backoff < instant {
                    let pair = (topic.clone(), peer.clone());
                    if let Some(s) = self.backoffs_by_heartbeat.get_mut(index) {
                        s.remove(&pair);
                    }
                    let index = insert_into_backoffs_by_heartbeat(
                        self.heartbeat_index,
                        &mut self.backoffs_by_heartbeat,
                        &self.heartbeat_interval,
                        self.backoff_slack,
                    );
                    o.insert((instant, index));
                }
            }
            Entry::Vacant(v) => {
                let index = insert_into_backoffs_by_heartbeat(
                    self.heartbeat_index,
                    &mut self.backoffs_by_heartbeat,
                    &self.heartbeat_interval,
                    self.backoff_slack,
                );
                v.insert((instant, index));
            }
        };
    }

    /// Checks if a given peer is backoffed for the given topic. This method respects the
    /// configured BACKOFF_SLACK and may return true even if the backup is already over.
    /// It is guaranteed to return false if the backoff is not over and eventually if enough time
    /// passed true if the backoff is over.
    ///
    /// This method should be used for deciding if we can already send a GRAFT to a previously
    /// backoffed peer.
    pub fn is_backoff_with_slack(&self, topic: &TopicHash, peer: &PeerId) -> bool {
        self.backoffs
            .get(topic)
            .map_or(false, |m| m.contains_key(peer))
    }

    pub fn get_backoff_time(&self, topic: &TopicHash, peer: &PeerId) -> Option<Instant> {
        Self::get_backoff_time_from_backoffs(&self.backoffs, topic, peer)
    }

    fn get_backoff_time_from_backoffs(
        backoffs: &HashMap<TopicHash, HashMap<PeerId, (Instant, usize)>>,
        topic: &TopicHash,
        peer: &PeerId,
    ) -> Option<Instant> {
        backoffs
            .get(topic)
            .and_then(|m| m.get(peer).map(|(i, _)| *i))
    }

    /// Applies a heartbeat. That should be called regularly in intervals of length
    /// `heartbeat_interval`.
    pub fn heartbeat(&mut self) {
        // Clean up backoffs_by_heartbeat
        if let Some(s) = self.backoffs_by_heartbeat.get_mut(self.heartbeat_index) {
            let backoffs = &mut self.backoffs;
            let slack = self.heartbeat_interval * self.backoff_slack;
            let now = Instant::now();
            s.retain(|(topic, peer)| {
                let keep = match Self::get_backoff_time_from_backoffs(backoffs, topic, peer) {
                    Some(backoff_time) => backoff_time + slack > now,
                    None => false,
                };
                if !keep {
                    //remove from backoffs
                    if let Entry::Occupied(mut m) = backoffs.entry(topic.clone()) {
                        if m.get_mut().remove(peer).is_some() && m.get().is_empty() {
                            m.remove();
                        }
                    }
                }

                keep
            });
        }

        // Increase heartbeat index
        self.heartbeat_index = (self.heartbeat_index + 1) % self.backoffs_by_heartbeat.len();
    }
}

/// Network behaviour that handles the gossipsub protocol.
///
/// NOTE: Initialisation requires a [`MessageAuthenticity`] and [`GossipsubConfig`] instance. If message signing is
/// disabled, the [`ValidationMode`] in the config should be adjusted to an appropriate level to
/// accept unsigned messages.
pub struct Gossipsub {
    /// Configuration providing gossipsub performance parameters.
    config: GossipsubConfig,

    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<Arc<GossipsubRpc>, GossipsubEvent>>,

    /// Pools non-urgent control messages between heartbeats.
    control_pool: HashMap<PeerId, Vec<GossipsubControlAction>>,

    /// Information used for publishing messages.
    publish_config: PublishConfig,

    /// An LRU Time cache for storing seen messages (based on their ID). This cache prevents
    /// duplicates from being propagated to the application and on the network.
    duplication_cache: DuplicateCache<MessageId>,

    /// A map of peers to their protocol kind. This is to identify different kinds of gossipsub
    /// peers.
    peer_protocols: HashMap<PeerId, PeerKind>,

    /// A map of all connected peers - A map of topic hash to a list of gossipsub peer Ids.
    topic_peers: HashMap<TopicHash, BTreeSet<PeerId>>,

    /// A map of all connected peers to their subscribed topics.
    peer_topics: HashMap<PeerId, BTreeSet<TopicHash>>,

    /// A set of all explicit peers.
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

    ///Storage for backoffs
    backoffs: BackoffStorage,

    /// Message cache for the last few heartbeats.
    mcache: MessageCache,

    /// Heartbeat interval stream.
    heartbeat: Interval,

    /// number of heartbeats since the beginning of time; this allows us to amortize some resource
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
    peer_score: Option<(PeerScore, PeerScoreThresholds, Interval, GossipPromises)>,

    /// Counts the number of `IHAVE` received from each peer since the last heartbeat.
    count_peer_have: HashMap<PeerId, usize>,

    /// Counts the number of `IWANT` that we sent the each peer since the last heartbeat.
    count_iasked: HashMap<PeerId, usize>,
}

impl Gossipsub {
    /// Creates a `Gossipsub` struct given a set of parameters specified via a `GossipsubConfig`.
    pub fn new(
        privacy: MessageAuthenticity,
        config: GossipsubConfig,
    ) -> Result<Self, &'static str> {
        // Set up the router given the configuration settings.

        // We do not allow configurations where a published message would also be rejected if it
        // were received locally.
        validate_config(&privacy, &config.validation_mode())?;

        // Set up message publishing parameters.

        Ok(Gossipsub {
            events: VecDeque::new(),
            control_pool: HashMap::new(),
            publish_config: privacy.into(),
            duplication_cache: DuplicateCache::new(config.duplicate_cache_time()),
            topic_peers: HashMap::new(),
            peer_topics: HashMap::new(),
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
            mcache: MessageCache::new(
                config.history_gossip(),
                config.history_length(),
                config.message_id_fn(),
            ),
            heartbeat: Interval::new_at(
                Instant::now() + config.heartbeat_initial_delay(),
                config.heartbeat_interval(),
            ),
            heartbeat_ticks: 0,
            config,
            px_peers: HashSet::new(),
            outbound_peers: HashSet::new(),
            peer_score: None,
            count_peer_have: HashMap::new(),
            count_iasked: HashMap::new(),
            peer_protocols: HashMap::new(),
        })
    }

    /// Subscribe to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe<H: Hasher>(&mut self, topic: Topic<H>) -> bool {
        debug!("Subscribing to topic: {}", topic);
        let topic_hash = topic.hash();
        if self.mesh.get(&topic_hash).is_some() {
            debug!("Topic: {} is already in the mesh.", topic);
            return false;
        }

        // send subscription request to all peers
        let peer_list = self.peer_topics.keys().cloned().collect::<Vec<_>>();
        if !peer_list.is_empty() {
            let event = Arc::new(GossipsubRpc {
                messages: Vec::new(),
                subscriptions: vec![GossipsubSubscription {
                    topic_hash: topic_hash.clone(),
                    action: GossipsubSubscriptionAction::Subscribe,
                }],
                control_msgs: Vec::new(),
            });

            for peer in peer_list {
                debug!("Sending SUBSCRIBE to peer: {:?}", peer);
                self.send_message(peer, event.clone());
            }
        }

        // call JOIN(topic)
        // this will add new peers to the mesh for the topic
        self.join(&topic_hash);
        info!("Subscribed to topic: {}", topic);
        true
    }

    /// Unsubscribes from a topic.
    ///
    /// Returns true if we were subscribed to this topic.
    pub fn unsubscribe<H: Hasher>(&mut self, topic: Topic<H>) -> bool {
        debug!("Unsubscribing from topic: {}", topic);
        let topic_hash = &topic.hash();

        if self.mesh.get(topic_hash).is_none() {
            debug!("Already unsubscribed from topic: {:?}", topic_hash);
            // we are not subscribed
            return false;
        }

        // announce to all peers
        let peer_list = self.peer_topics.keys().cloned().collect::<Vec<_>>();
        if !peer_list.is_empty() {
            let event = Arc::new(GossipsubRpc {
                messages: Vec::new(),
                subscriptions: vec![GossipsubSubscription {
                    topic_hash: topic_hash.clone(),
                    action: GossipsubSubscriptionAction::Unsubscribe,
                }],
                control_msgs: Vec::new(),
            });

            for peer in peer_list {
                debug!("Sending UNSUBSCRIBE to peer: {}", peer.to_string());
                self.send_message(peer, event.clone());
            }
        }

        // call LEAVE(topic)
        // this will remove the topic from the mesh
        self.leave(&topic_hash);

        info!("Unsubscribed from topic: {:?}", topic_hash);
        true
    }

    /// Publishes a message to the network.
    pub fn publish<H: Hasher>(
        &mut self,
        topic: Topic<H>,
        data: impl Into<Vec<u8>>,
    ) -> Result<(), PublishError> {
        self.publish_many(iter::once(topic), data)
    }

    /// Publishes a message with multiple topics to the network.
    pub fn publish_many<H: Hasher>(
        &mut self,
        topics: impl IntoIterator<Item = Topic<H>>,
        data: impl Into<Vec<u8>>,
    ) -> Result<(), PublishError> {
        let message =
            self.build_message(topics.into_iter().map(|t| t.hash()).collect(), data.into())?;
        let msg_id = (self.config.message_id_fn())(&message);

        // Add published message to the duplicate cache.
        if !self.duplication_cache.insert(msg_id.clone()) {
            // This message has already been seen. We don't re-publish messages that have already
            // been published on the network.
            warn!(
                "Not publishing a message that has already been published. Msg-id {}",
                msg_id
            );
            return Err(PublishError::Duplicate);
        }

        // If the message isn't a duplicate add it to the memcache.
        self.mcache.put(message.clone());

        debug!("Publishing message: {:?}", msg_id);

        // If we are not flood publishing forward the message to mesh peers.
        let mesh_peers_sent =
            !self.config.flood_publish() && self.forward_msg(message.clone(), None);

        let mut recipient_peers = HashSet::new();
        for topic_hash in &message.topics {
            if let Some(set) = self.topic_peers.get(&topic_hash) {
                if self.config.flood_publish() {
                    // Forward to all peers above score and all explicit peers
                    recipient_peers.extend(
                        set.iter()
                            .filter(|p| {
                                self.explicit_peers.contains(*p)
                                    || !self.score_below_threshold(*p, |ts| ts.publish_threshold).0
                            })
                            .map(|p| p.clone()),
                    );
                    continue;
                }

                // Explicit peers
                for peer in &self.explicit_peers {
                    if set.contains(peer) {
                        recipient_peers.insert(peer.clone());
                    }
                }

                // Floodsub peers
                for (peer, kind) in &self.peer_protocols {
                    if kind == &PeerKind::Floodsub
                        && !self
                            .score_below_threshold(peer, |ts| ts.publish_threshold)
                            .0
                    {
                        recipient_peers.insert(peer.clone());
                    }
                }

                // Gossipsub peers
                if self.mesh.get(&topic_hash).is_none() {
                    debug!("Topic: {:?} not in the mesh", topic_hash);
                    // If we have fanout peers add them to the map.
                    if self.fanout.contains_key(&topic_hash) {
                        for peer in self.fanout.get(&topic_hash).expect("Topic must exist") {
                            recipient_peers.insert(peer.clone());
                        }
                    } else {
                        // We have no fanout peers, select mesh_n of them and add them to the fanout
                        let mesh_n = self.config.mesh_n();
                        let new_peers = Self::get_random_peers(
                            &self.topic_peers,
                            &self.peer_protocols,
                            &topic_hash,
                            mesh_n,
                            {
                                |p| {
                                    !self.explicit_peers.contains(p)
                                        && !self
                                            .score_below_threshold(p, |pst| pst.publish_threshold)
                                            .0
                                }
                            },
                        );
                        // Add the new peers to the fanout and recipient peers
                        self.fanout.insert(topic_hash.clone(), new_peers.clone());
                        for peer in new_peers {
                            debug!("Peer added to fanout: {:?}", peer);
                            recipient_peers.insert(peer.clone());
                        }
                    }
                    // We are publishing to fanout peers - update the time we published
                    self.fanout_last_pub
                        .insert(topic_hash.clone(), Instant::now());
                }
            }
        }

        if recipient_peers.is_empty() && !mesh_peers_sent {
            return Err(PublishError::InsufficientPeers);
        }

        let event = Arc::new(GossipsubRpc {
            subscriptions: Vec::new(),
            messages: vec![message],
            control_msgs: Vec::new(),
        });

        // check that the size doesn't exceed the max transmission size
        if event.size() > self.config.max_transmit_size() {
            // NOTE: The size limit can be reached by excessive topics or an excessive message.
            // This is an estimate that should be within 10% of the true encoded value. It is
            // possible to have a message that exceeds the RPC limit and is not caught here. A
            // warning log will be emitted in this case.
            return Err(PublishError::MessageTooLarge);
        }

        // Send to peers we know are subscribed to the topic.
        for peer_id in recipient_peers.iter() {
            debug!("Sending message to peer: {:?}", peer_id);
            self.send_message(peer_id.clone(), event.clone());
        }

        info!("Published message: {:?}", msg_id);
        Ok(())
    }

    /// This function should be called when `config.validate_messages()` is `true` after the
    /// message got validated by the caller. Messages are stored in the
    /// ['Memcache'] and validation is expected to be fast enough that the messages should still
    /// exist in the cache. There are three possible validation outcomes and the outcome is given
    /// in acceptance.
    ///
    /// If acceptance = Accept the message will get propagated to the network. The
    /// `propagation_source` parameter indicates who the message was received by and will not
    ///  be forwarded back to that peer.
    ///
    /// If acceptance = Reject the message will be deleted from the memcache and the P₄ penalty
    /// will be applied to the `propagation_source`.
    ///
    /// If acceptance = Ignore the message will be deleted from the memcache but no P₄ penalty
    /// will be applied.
    ///
    /// This function will return true if the message was found in the cache and false if was not
    /// in the cache anymore.
    ///
    /// This should only be called once per message.
    pub fn report_message_validation_result(
        &mut self,
        message_id: &MessageId,
        propagation_source: &PeerId,
        acceptance: MessageAcceptance,
    ) -> bool {
        let reject_reason = match acceptance {
            MessageAcceptance::Accept => {
                let message = match self.mcache.validate(message_id) {
                    Some(message) => message.clone(),
                    None => {
                        warn!(
                            "Message not in cache. Ignoring forwarding. Message Id: {}",
                            message_id
                        );
                        return false;
                    }
                };
                self.forward_msg(message, Some(propagation_source));
                return true;
            }
            MessageAcceptance::Reject => RejectReason::ValidationFailed,
            MessageAcceptance::Ignore => RejectReason::ValidationIgnored,
        };

        if let Some(message) = self.mcache.remove(message_id) {
            // Tell peer_score about reject
            if let Some((peer_score, ..)) = &mut self.peer_score {
                peer_score.reject_message(propagation_source, &message, reject_reason);
            }
            true
        } else {
            warn!("Rejected message not in cache. Message Id: {}", message_id);
            false
        }
    }

    /// Adds a new peer to the list of explicitly connected peers.
    pub fn add_explicit_peer(&mut self, peer_id: &PeerId) {
        debug!("Adding explicit peer {}", peer_id);

        self.explicit_peers.insert(peer_id.clone());

        self.check_explicit_peer_connection(peer_id);
    }

    /// This removes the peer from explicitly connected peers, note that this does not disconnect
    /// the peer.
    pub fn remove_explicit_peer(&mut self, peer_id: &PeerId) {
        debug!("Removing explicit peer {}", peer_id);
        self.explicit_peers.remove(peer_id);
    }

    /// Blacklists a peer. All messages from this peer will be rejected and any message that was
    /// created by this peer will be rejected.
    pub fn blacklist_peer(&mut self, peer_id: &PeerId) {
        if self.blacklisted_peers.insert(peer_id.clone()) {
            debug!("Peer has been blacklisted: {}", peer_id);
        }
    }

    /// Removes a blacklisted peer if it has previously been blacklisted.
    pub fn remove_blacklisted_peer(&mut self, peer_id: &PeerId) {
        if self.blacklisted_peers.remove(peer_id) {
            debug!("Peer has been removed from the blacklist: {}", peer_id);
        }
    }

    /// Activates the peer scoring system with the given parameters. This will reset all scores
    /// if there was already another peer scoring system activated. Returns an error if the
    /// params are not valid.
    pub fn with_peer_score(
        &mut self,
        params: PeerScoreParams,
        threshold: PeerScoreThresholds,
    ) -> Result<(), String> {
        params.validate()?;
        threshold.validate()?;

        let interval = Interval::new(params.decay_interval);
        let peer_score = PeerScore::new(params, self.config.message_id_fn());
        self.peer_score = Some((peer_score, threshold, interval, GossipPromises::default()));
        Ok(())
    }

    /// Sets scoring parameters for a topic.
    ///
    /// The `with_peer_score()` must first be called to initialise peer scoring.
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
        debug!("Running JOIN for topic: {:?}", topic_hash);

        // if we are already in the mesh, return
        if self.mesh.contains_key(topic_hash) {
            info!("JOIN: The topic is already in the mesh, ignoring JOIN");
            return;
        }

        let mut added_peers = HashSet::new();

        // check if we have mesh_n peers in fanout[topic] and add them to the mesh if we do,
        // removing the fanout entry.
        if let Some((_, mut peers)) = self.fanout.remove_entry(topic_hash) {
            debug!(
                "JOIN: Removing peers from the fanout for topic: {:?}",
                topic_hash
            );

            // remove explicit peers and peers with negative scores
            peers = peers
                .into_iter()
                .filter(|p| {
                    !self.explicit_peers.contains(p) && !self.score_below_threshold(p, |_| 0.0).0
                })
                .collect();

            // Add up to mesh_n of them them to the mesh
            // NOTE: These aren't randomly added, currently FIFO
            let add_peers = std::cmp::min(peers.len(), self.config.mesh_n());
            debug!(
                "JOIN: Adding {:?} peers from the fanout for topic: {:?}",
                add_peers, topic_hash
            );
            added_peers.extend(peers.iter().cloned().take(add_peers));
            self.mesh.insert(
                topic_hash.clone(),
                peers.into_iter().take(add_peers).collect(),
            );
            // remove the last published time
            self.fanout_last_pub.remove(topic_hash);
        }

        // check if we need to get more peers, which we randomly select
        if added_peers.len() < self.config.mesh_n() {
            // get the peers
            let new_peers = Self::get_random_peers(
                &self.topic_peers,
                &self.peer_protocols,
                topic_hash,
                self.config.mesh_n() - added_peers.len(),
                |peer| {
                    !added_peers.contains(peer)
                        && !self.explicit_peers.contains(peer)
                        && !self.score_below_threshold(peer, |_| 0.0).0
                },
            );
            added_peers.extend(new_peers.clone());
            // add them to the mesh
            debug!(
                "JOIN: Inserting {:?} random peers into the mesh",
                new_peers.len()
            );
            let mesh_peers = self
                .mesh
                .entry(topic_hash.clone())
                .or_insert_with(Default::default);
            mesh_peers.extend(new_peers);
        }

        for peer_id in added_peers {
            // Send a GRAFT control message
            info!("JOIN: Sending Graft message to peer: {:?}", peer_id);
            if let Some((peer_score, ..)) = &mut self.peer_score {
                peer_score.graft(&peer_id, topic_hash.clone());
            }
            Self::control_pool_add(
                &mut self.control_pool,
                peer_id.clone(),
                GossipsubControlAction::Graft {
                    topic_hash: topic_hash.clone(),
                },
            );
        }
        debug!("Completed JOIN for topic: {:?}", topic_hash);
    }

    /// Creates a PRUNE gossipsub action.
    fn make_prune(
        &mut self,
        topic_hash: &TopicHash,
        peer: &PeerId,
        do_px: bool,
    ) -> GossipsubControlAction {
        if let Some((peer_score, ..)) = &mut self.peer_score {
            peer_score.prune(peer, topic_hash.clone());
        }

        match self.peer_protocols.get(peer) {
            Some(PeerKind::Floodsub) => {
                error!("Attempted to prune a Floodsub peer");
            }
            Some(PeerKind::Gossipsub) => {
                // GossipSub v1.0 -- no peer exchange, the peer won't be able to parse it anyway
                return GossipsubControlAction::Prune {
                    topic_hash: topic_hash.clone(),
                    peers: Vec::new(),
                    backoff: None,
                };
            }
            None => {
                error!("Attempted to Prune an unknown peer");
            }
            _ => {} // Gossipsub 1.1 peer perform the `Prune`
        }

        // Select peers for peer exchange
        let peers = if do_px {
            Self::get_random_peers(
                &self.topic_peers,
                &self.peer_protocols,
                &topic_hash,
                self.config.prune_peers(),
                |p| p != peer && !self.score_below_threshold(p, |_| 0.0).0,
            )
            .into_iter()
            .map(|p| PeerInfo { peer_id: Some(p) })
            .collect()
        } else {
            Vec::new()
        };

        // update backoff
        self.backoffs
            .update_backoff(topic_hash, peer, self.config.prune_backoff());

        GossipsubControlAction::Prune {
            topic_hash: topic_hash.clone(),
            peers,
            backoff: Some(self.config.prune_backoff().as_secs()),
        }
    }

    /// Gossipsub LEAVE(topic) - Notifies mesh\[topic\] peers with PRUNE messages.
    fn leave(&mut self, topic_hash: &TopicHash) {
        debug!("Running LEAVE for topic {:?}", topic_hash);

        // If our mesh contains the topic, send prune to peers and delete it from the mesh
        if let Some((_, peers)) = self.mesh.remove_entry(topic_hash) {
            for peer in peers {
                // Send a PRUNE control message
                info!("LEAVE: Sending PRUNE to peer: {:?}", peer);
                let control = self.make_prune(topic_hash, &peer, self.config.do_px());
                Self::control_pool_add(&mut self.control_pool, peer.clone(), control);
            }
        }
        debug!("Completed LEAVE for topic: {:?}", topic_hash);
    }

    /// Checks if the given peer is still connected and if not dials the peer again.
    fn check_explicit_peer_connection(&mut self, peer_id: &PeerId) {
        if !self.peer_topics.contains_key(peer_id) {
            // Connect to peer
            debug!("Connecting to explicit peer {:?}", peer_id);
            self.events.push_back(NetworkBehaviourAction::DialPeer {
                peer_id: peer_id.clone(),
                condition: DialPeerCondition::Disconnected,
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
        if let Some((peer_score, thresholds, ..)) = &self.peer_score {
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
            debug!(
                "IHAVE: ignoring peer {:?} with score below threshold [score = {}]",
                peer_id, score
            );
            return;
        }

        // IHAVE flood protection
        let peer_have = self.count_peer_have.entry(peer_id.clone()).or_insert(0);
        *peer_have += 1;
        if *peer_have > self.config.max_ihave_messages() {
            debug!(
                "IHAVE: peer {} has advertised too many times ({}) within this heartbeat \
            interval; ignoring",
                peer_id, *peer_have
            );
            return;
        }

        if let Some(iasked) = self.count_iasked.get(peer_id) {
            if *iasked >= self.config.max_ihave_length() {
                debug!(
                    "IHAVE: peer {} has already advertised too many messages ({}); ignoring",
                    peer_id, *iasked
                );
                return;
            }
        }

        debug!("Handling IHAVE for peer: {:?}", peer_id);

        // use a hashset to avoid duplicates efficiently
        let mut iwant_ids = HashSet::new();

        for (topic, ids) in ihave_msgs {
            // only process the message if we are subscribed
            if !self.mesh.contains_key(&topic) {
                debug!(
                    "IHAVE: Ignoring IHAVE - Not subscribed to topic: {:?}",
                    topic
                );
                continue;
            }

            for id in ids {
                if self.mcache.get(&id).is_none() {
                    // have not seen this message, request it
                    iwant_ids.insert(id);
                }
            }
        }

        if !iwant_ids.is_empty() {
            let iasked = self.count_iasked.entry(peer_id.clone()).or_insert(0);
            let mut iask = iwant_ids.len();
            if *iasked + iask > self.config.max_ihave_length() {
                iask = self.config.max_ihave_length().saturating_sub(*iasked);
            }

            // Send the list of IWANT control messages
            debug!(
                "IHAVE: Asking for {} out of {} messages from {}",
                iask,
                iwant_ids.len(),
                peer_id
            );

            //ask in random order
            let mut iwant_ids_vec: Vec<_> = iwant_ids.iter().collect();
            let mut rng = thread_rng();
            iwant_ids_vec.partial_shuffle(&mut rng, iask as usize);

            iwant_ids_vec.truncate(iask as usize);
            *iasked += iask;

            let message_ids = iwant_ids_vec.into_iter().cloned().collect::<Vec<_>>();
            if let Some((_, _, _, gossip_promises)) = &mut self.peer_score {
                gossip_promises.add_promise(
                    peer_id.clone(),
                    &message_ids,
                    Instant::now() + self.config.iwant_followup_time(),
                );
            }

            Self::control_pool_add(
                &mut self.control_pool,
                peer_id.clone(),
                GossipsubControlAction::IWant { message_ids },
            );
        }
        debug!("Completed IHAVE handling for peer: {:?}", peer_id);
    }

    /// Handles an IWANT control message. Checks our cache of messages. If the message exists it is
    /// forwarded to the requesting peer.
    fn handle_iwant(&mut self, peer_id: &PeerId, iwant_msgs: Vec<MessageId>) {
        // We ignore IWANT gossip from any peer whose score is below the gossip threshold
        if let (true, score) = self.score_below_threshold(peer_id, |pst| pst.gossip_threshold) {
            debug!(
                "IWANT: ignoring peer {:?} with score below threshold [score = {}]",
                peer_id, score
            );
            return;
        }

        debug!("Handling IWANT for peer: {:?}", peer_id);
        // build a hashmap of available messages
        let mut cached_messages = HashMap::new();

        for id in iwant_msgs {
            // If we have it and the IHAVE count is not above the threshold, add it do the
            // cached_messages mapping
            if let Some((msg, count)) = self.mcache.get_with_iwant_counts(&id, peer_id) {
                if count > self.config.gossip_retransimission() {
                    debug!(
                        "IWANT: Peer {} has asked for message {} too many times; ignoring \
                    request",
                        peer_id, &id
                    );
                } else {
                    cached_messages.insert(id.clone(), msg.clone());
                }
            }
        }

        if !cached_messages.is_empty() {
            debug!("IWANT: Sending cached messages to peer: {:?}", peer_id);
            // Send the messages to the peer
            let message_list = cached_messages.into_iter().map(|entry| entry.1).collect();
            self.send_message(
                peer_id.clone(),
                GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: message_list,
                    control_msgs: Vec::new(),
                },
            );
        }
        debug!("Completed IWANT handling for peer: {}", peer_id);
    }

    /// Handles GRAFT control messages. If subscribed to the topic, adds the peer to mesh, if not,
    /// responds with PRUNE messages.
    fn handle_graft(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        debug!("Handling GRAFT message for peer: {}", peer_id);

        let mut to_prune_topics = HashSet::new();

        let mut do_px = self.config.do_px();

        // we don't GRAFT to/from explicit peers; complain loudly if this happens
        if self.explicit_peers.contains(peer_id) {
            warn!("GRAFT: ignoring request from direct peer {}", peer_id);
            // this is possibly a bug from non-reciprocal configuration; send a PRUNE for all topics
            to_prune_topics = HashSet::from_iter(topics.into_iter());
            // but don't PX
            do_px = false
        } else {
            let (below_zero, score) = self.score_below_threshold(peer_id, |_| 0.0);
            let now = Instant::now();
            for topic_hash in topics {
                if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                    // if the peer is already in the mesh ignore the graft
                    if peers.contains(peer_id) {
                        continue;
                    }

                    // make sure we are not backing off that peer
                    if let Some(backoff_time) = self.backoffs.get_backoff_time(&topic_hash, peer_id)
                    {
                        if backoff_time > now {
                            warn!(
                                "GRAFT: peer attempted graft within backoff time, penalizing {}",
                                peer_id
                            );
                            // add behavioural penalty
                            if let Some((peer_score, ..)) = &mut self.peer_score {
                                peer_score.add_penalty(peer_id, 1);

                                // check the flood cutoff
                                let flood_cutoff = (backoff_time
                                    + self.config.graft_flood_threshold())
                                    - self.config.prune_backoff();
                                if flood_cutoff > now {
                                    //extra penalty
                                    peer_score.add_penalty(peer_id, 1);
                                }
                            }
                            //no PX
                            do_px = false;

                            to_prune_topics.insert(topic_hash.clone());
                            continue;
                        }
                    }

                    //check the score
                    if below_zero {
                        // we don't GRAFT peers with negative score
                        debug!(
                            "GRAFT: ignoring peer {:?} with negative score [score = {}, \
                        topic = {}]",
                            peer_id, score, topic_hash
                        );
                        // we do send them PRUNE however, because it's a matter of protocol correctness
                        to_prune_topics.insert(topic_hash.clone());
                        // but we won't PX to them
                        do_px = false;
                        continue;
                    }

                    // check mesh upper bound and only allow graft if the upper bound is not reached or
                    // if it is an outbound peer
                    if peers.len() >= self.config.mesh_n_high()
                        && !self.outbound_peers.contains(peer_id)
                    {
                        to_prune_topics.insert(topic_hash.clone());
                        continue;
                    }

                    // add peer to the mesh
                    info!(
                        "GRAFT: Mesh link added for peer: {:?} in topic: {:?}",
                        peer_id, &topic_hash
                    );
                    peers.insert(peer_id.clone());

                    if let Some((peer_score, ..)) = &mut self.peer_score {
                        peer_score.graft(peer_id, topic_hash);
                    }
                } else {
                    // don't do PX when there is an unknown topic to avoid leaking our peers
                    do_px = false;
                    // spam hardening: ignore GRAFTs for unknown topics
                    continue;
                }
            }
        }

        if !to_prune_topics.is_empty() {
            // build the prune messages to send
            let prune_messages = to_prune_topics
                .iter()
                .map(|t| self.make_prune(t, peer_id, do_px))
                .collect();
            // Send the prune messages to the peer
            info!(
                "GRAFT: Not subscribed to topics -  Sending PRUNE to peer: {}",
                peer_id
            );
            self.send_message(
                peer_id.clone(),
                GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: prune_messages,
                },
            );
        }
        debug!("Completed GRAFT handling for peer: {}", peer_id);
    }

    /// Handles PRUNE control messages. Removes peer from the mesh.
    fn handle_prune(
        &mut self,
        peer_id: &PeerId,
        prune_data: Vec<(TopicHash, Vec<PeerInfo>, Option<u64>)>,
    ) {
        debug!("Handling PRUNE message for peer: {}", peer_id);
        let (below_threshold, score) =
            self.score_below_threshold(peer_id, |pst| pst.accept_px_threshold);
        for (topic_hash, px, backoff) in prune_data {
            if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                // remove the peer if it exists in the mesh
                if peers.remove(peer_id) {
                    info!(
                        "PRUNE: Removing peer: {} from the mesh for topic: {}",
                        peer_id.to_string(),
                        topic_hash
                    );
                }

                if let Some((peer_score, ..)) = &mut self.peer_score {
                    peer_score.prune(peer_id, topic_hash.clone());
                }

                // is there a backoff specified by the peer? if so obey it.
                self.backoffs.update_backoff(
                    &topic_hash,
                    peer_id,
                    if let Some(backoff) = backoff {
                        Duration::from_secs(backoff)
                    } else {
                        self.config.prune_backoff()
                    },
                );

                //connect to px peers
                if !px.is_empty() {
                    // we ignore PX from peers with insufficient score
                    if below_threshold {
                        debug!(
                            "PRUNE: ignoring PX from peer {:?} with insufficient score \
                        [score ={} topic = {}]",
                            peer_id, score, topic_hash
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
        debug!("Completed PRUNE handling for peer: {}", peer_id.to_string());
    }

    fn px_connect(&mut self, mut px: Vec<PeerInfo>) {
        let n = self.config.prune_peers();
        // Ignore peerInfo with no ID
        //TODO: Once signed records are spec'd: Can we use peerInfo without any IDs if they have a signed peer record?
        px = px.into_iter().filter(|p| p.peer_id.is_some()).collect();
        if px.len() > n {
            // only use at most prune_peers many random peers
            let mut rng = thread_rng();
            px.partial_shuffle(&mut rng, n);
            px = px.into_iter().take(n).collect();
        }

        for p in px {
            // TODO: Once signed records are spec'd: extract signed peer record if given and handle it, see
            // https://github.com/libp2p/specs/pull/217
            if let Some(peer_id) = p.peer_id {
                // mark as px peer
                self.px_peers.insert(peer_id.clone());

                // dial peer
                self.events.push_back(NetworkBehaviourAction::DialPeer {
                    peer_id,
                    condition: DialPeerCondition::Disconnected,
                });
            }
        }
    }

    /// Handles a newly received GossipsubMessage.
    /// Forwards the message to all peers in the mesh.
    fn handle_received_message(&mut self, mut msg: GossipsubMessage, propagation_source: &PeerId) {
        let msg_id = (self.config.message_id_fn())(&msg);
        debug!(
            "Handling message: {:?} from peer: {}",
            msg_id,
            propagation_source.to_string()
        );

        // Reject any message from a blacklisted peer
        if self.blacklisted_peers.contains(propagation_source) {
            debug!(
                "Rejecting message from blacklisted peer: {}",
                propagation_source
            );
            if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
                peer_score.reject_message(propagation_source, &msg, RejectReason::BlackListedPeer);
                gossip_promises.reject_message(&msg_id, &RejectReason::BlackListedPeer);
            }
            return;
        }

        // Also reject any message that originated from a blacklisted peer
        if let Some(source) = msg.source.as_ref() {
            if self.blacklisted_peers.contains(source) {
                if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
                    peer_score.reject_message(
                        propagation_source,
                        &msg,
                        RejectReason::BlackListedSource,
                    );
                    gossip_promises.reject_message(&msg_id, &RejectReason::BlackListedSource);
                }
                return;
            }
        }

        // If we are not validating messages, assume this message is validated
        // This will allow the message to be gossiped without explicitly calling
        // `validate_message`.
        if !self.config.validate_messages() {
            msg.validated = true;
        }

        // reject messages claiming to be from ourselves but not locally published
        if let Some(own_id) = self.publish_config.get_own_id() {
            if !self.config.allow_self_origin()
                && own_id != propagation_source
                && msg.source.as_ref().map_or(false, |s| s == own_id)
            {
                debug!(
                    "Dropping message {} claiming to be from self but forwarded from {}",
                    msg_id, propagation_source
                );
                if let Some((peer_score, _, _, gossip_promises)) = &mut self.peer_score {
                    peer_score.reject_message(propagation_source, &msg, RejectReason::SelfOrigin);
                    gossip_promises.reject_message(&msg_id, &RejectReason::SelfOrigin);
                }
                return;
            }
        }

        // Add the message to the duplication cache and memcache.
        if !self.duplication_cache.insert(msg_id.clone()) {
            debug!("Message already received, ignoring. Message: {}", msg_id);
            if let Some((peer_score, ..)) = &mut self.peer_score {
                peer_score.duplicated_message(propagation_source, &msg);
            }
            return;
        }

        // Tells score that message arrived (but is maybe not fully validated yet)
        // Consider message as delivered for gossip promises
        if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
            peer_score.validate_message(propagation_source, &msg);
            gossip_promises.deliver_message(&msg_id);
        }

        // Add the message to our memcache
        self.mcache.put(msg.clone());

        // Dispatch the message to the user if we are subscribed to any of the topics
        if self.mesh.keys().any(|t| msg.topics.iter().any(|u| t == u)) {
            debug!("Sending received message to user");
            self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                GossipsubEvent::Message {
                    propagation_source: propagation_source.clone(),
                    message_id: msg_id,
                    message: msg.clone(),
                },
            ));
        } else {
            debug!(
                "Received message on a topic we are not subscribed to. Topics {:?}",
                msg.topics.iter().collect::<Vec<_>>()
            );
            return;
        }

        // forward the message to mesh peers, if no validation is required
        if !self.config.validate_messages() {
            let message_id = (self.config.message_id_fn())(&msg);
            self.forward_msg(msg, Some(propagation_source));
            debug!("Completed message handling for message: {:?}", message_id);
        }
    }

    /// Handles received subscriptions.
    fn handle_received_subscriptions(
        &mut self,
        subscriptions: &[GossipsubSubscription],
        propagation_source: &PeerId,
    ) {
        debug!(
            "Handling subscriptions: {:?}, from source: {}",
            subscriptions,
            propagation_source.to_string()
        );
        let subscribed_topics = match self.peer_topics.get_mut(propagation_source) {
            Some(topics) => topics,
            None => {
                error!(
                    "Subscription by unknown peer: {}",
                    propagation_source.to_string()
                );
                return;
            }
        };

        // Collect potential graft messages for the peer.
        let mut grafts = Vec::new();

        // Notify the application about the subscription, after the grafts are sent.
        let mut application_event = Vec::new();

        for subscription in subscriptions {
            // get the peers from the mapping, or insert empty lists if the topic doesn't exist
            let peer_list = self
                .topic_peers
                .entry(subscription.topic_hash.clone())
                .or_insert_with(Default::default);

            match subscription.action {
                GossipsubSubscriptionAction::Subscribe => {
                    if peer_list.insert(propagation_source.clone()) {
                        debug!(
                            "SUBSCRIPTION: Adding gossip peer: {} to topic: {:?}",
                            propagation_source.to_string(),
                            subscription.topic_hash
                        );
                    }

                    // add to the peer_topics mapping
                    subscribed_topics.insert(subscription.topic_hash.clone());

                    // if the mesh needs peers add the peer to the mesh
                    if !self.explicit_peers.contains(propagation_source)
                        && match self.peer_protocols.get(propagation_source) {
                            Some(PeerKind::Gossipsubv1_1) => true,
                            Some(PeerKind::Gossipsub) => true,
                            _ => false,
                        }
                    {
                        if let Some(peers) = self.mesh.get_mut(&subscription.topic_hash) {
                            if peers.len() < self.config.mesh_n_low() {
                                if peers.insert(propagation_source.clone()) {
                                    debug!(
                                        "SUBSCRIPTION: Adding peer {} to the mesh for topic {:?}",
                                        propagation_source.to_string(),
                                        subscription.topic_hash
                                    );
                                    // send graft to the peer
                                    debug!(
                                        "Sending GRAFT to peer {} for topic {:?}",
                                        propagation_source.to_string(),
                                        subscription.topic_hash
                                    );
                                    if let Some((peer_score, ..)) = &mut self.peer_score {
                                        peer_score.graft(
                                            propagation_source,
                                            subscription.topic_hash.clone(),
                                        );
                                    }
                                    grafts.push(GossipsubControlAction::Graft {
                                        topic_hash: subscription.topic_hash.clone(),
                                    });
                                }
                            }
                        }
                    }
                    // generates a subscription event to be polled
                    application_event.push(NetworkBehaviourAction::GenerateEvent(
                        GossipsubEvent::Subscribed {
                            peer_id: propagation_source.clone(),
                            topic: subscription.topic_hash.clone(),
                        },
                    ));
                }
                GossipsubSubscriptionAction::Unsubscribe => {
                    if peer_list.remove(propagation_source) {
                        info!(
                            "SUBSCRIPTION: Removing gossip peer: {} from topic: {:?}",
                            propagation_source.to_string(),
                            subscription.topic_hash
                        );
                    }
                    // remove topic from the peer_topics mapping
                    subscribed_topics.remove(&subscription.topic_hash);
                    // remove the peer from the mesh if it exists
                    if let Some(peers) = self.mesh.get_mut(&subscription.topic_hash) {
                        peers.remove(propagation_source);
                        // the peer requested the unsubscription so we don't need to send a PRUNE.
                    }

                    // generate an unsubscribe event to be polled
                    application_event.push(NetworkBehaviourAction::GenerateEvent(
                        GossipsubEvent::Unsubscribed {
                            peer_id: propagation_source.clone(),
                            topic: subscription.topic_hash.clone(),
                        },
                    ));
                }
            }
        }

        // If we need to send grafts to peer, do so immediately, rather than waiting for the
        // heartbeat.
        if !grafts.is_empty() {
            self.send_message(
                propagation_source.clone(),
                GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: grafts,
                },
            );
        }

        // Notify the application of the subscriptions
        for event in application_event {
            self.events.push_back(event);
        }

        trace!(
            "Completed handling subscriptions from source: {:?}",
            propagation_source
        );
    }

    /// Applies penalties to peers that did not respond to our IWANT requests.
    fn apply_iwant_penalties(&mut self) {
        if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
            for (peer, count) in gossip_promises.get_broken_promises() {
                peer_score.add_penalty(&peer, count);
            }
        }
    }

    /// Heartbeat function which shifts the memcache and updates the mesh.
    fn heartbeat(&mut self) {
        debug!("Starting heartbeat");

        self.heartbeat_ticks += 1;

        let mut to_graft = HashMap::new();
        let mut to_prune = HashMap::new();
        let mut no_px = HashSet::new();

        // clean up expired backoffs
        self.backoffs.heartbeat();

        // clean up ihave counters
        self.count_iasked.clear();
        self.count_peer_have.clear();

        // apply iwant penalties
        self.apply_iwant_penalties();

        // check connections to explicit peers
        if self.heartbeat_ticks % self.config.check_explicit_peers_ticks() == 0 {
            for p in self.explicit_peers.clone() {
                self.check_explicit_peer_connection(&p);
            }
        }

        // cache scores throughout the heartbeat
        let mut scores = HashMap::new();
        let peer_score = &self.peer_score;
        let mut score = |p: &PeerId| match peer_score {
            Some((peer_score, ..)) => *scores
                .entry(p.clone())
                .or_insert_with(|| peer_score.score(p)),
            _ => 0.0,
        };

        // maintain the mesh for each topic
        for (topic_hash, peers) in self.mesh.iter_mut() {
            let explicit_peers = &self.explicit_peers;
            let backoffs = &self.backoffs;
            let topic_peers = &self.topic_peers;
            let outbound_peers = &self.outbound_peers;

            // drop all peers with negative score, without PX
            // if there is at some point a stable retain method for BTreeSet the following can be
            // written more efficiently with retain.
            let to_remove: Vec<_> = peers
                .iter()
                .filter(|&p| {
                    if score(p) < 0.0 {
                        debug!(
                            "HEARTBEAT: Prune peer {:?} with negative score [score = {}, topic = \
                    {}]",
                            p,
                            score(p),
                            topic_hash
                        );

                        let current_topic = to_prune.entry(p.clone()).or_insert_with(Vec::new);
                        current_topic.push(topic_hash.clone());
                        no_px.insert(p.clone());
                        true
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();
            for peer in to_remove {
                peers.remove(&peer);
            }

            // too little peers - add some
            if peers.len() < self.config.mesh_n_low() {
                debug!(
                    "HEARTBEAT: Mesh low. Topic: {} Contains: {} needs: {}",
                    topic_hash,
                    peers.len(),
                    self.config.mesh_n_low()
                );
                // not enough peers - get mesh_n - current_length more
                let desired_peers = self.config.mesh_n() - peers.len();
                let peer_list = Self::get_random_peers(
                    topic_peers,
                    &self.peer_protocols,
                    topic_hash,
                    desired_peers,
                    |peer| {
                        !peers.contains(peer)
                            && !explicit_peers.contains(peer)
                            && !backoffs.is_backoff_with_slack(topic_hash, peer)
                            && score(peer) >= 0.0
                    },
                );
                for peer in &peer_list {
                    let current_topic = to_graft.entry(peer.clone()).or_insert_with(Vec::new);
                    current_topic.push(topic_hash.clone());
                }
                // update the mesh
                debug!("Updating mesh, new mesh: {:?}", peer_list);
                peers.extend(peer_list);
            }

            // too many peers - remove some
            if peers.len() > self.config.mesh_n_high() {
                debug!(
                    "HEARTBEAT: Mesh high. Topic: {} Contains: {} needs: {}",
                    topic_hash,
                    peers.len(),
                    self.config.mesh_n_high()
                );
                let excess_peer_no = peers.len() - self.config.mesh_n();

                // shuffle the peers and then sort by score ascending beginning with the worst
                let mut rng = thread_rng();
                let mut shuffled = peers.iter().cloned().collect::<Vec<_>>();
                shuffled.shuffle(&mut rng);
                shuffled
                    .sort_by(|p1, p2| score(p1).partial_cmp(&score(p2)).unwrap_or(Ordering::Equal));
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
                            //do not remove anymore outbound peers
                            continue;
                        } else {
                            //an outbound peer gets removed
                            outbound -= 1;
                        }
                    }

                    //remove the peer
                    peers.remove(&peer);
                    let current_topic = to_prune.entry(peer).or_insert_with(Vec::new);
                    current_topic.push(topic_hash.clone());
                    removed += 1;
                }
            }

            // do we have enough outbound peers?
            if peers.len() >= self.config.mesh_n_low() {
                // count number of outbound peers we have
                let outbound = { peers.iter().filter(|p| outbound_peers.contains(*p)).count() };

                // if we have not enough outbound peers, graft to some new outbound peers
                if outbound < self.config.mesh_outbound_min() {
                    let needed = self.config.mesh_outbound_min() - outbound;
                    let peer_list = Self::get_random_peers(
                        topic_peers,
                        &self.peer_protocols,
                        topic_hash,
                        needed,
                        |peer| {
                            !peers.contains(peer)
                                && !explicit_peers.contains(peer)
                                && !backoffs.is_backoff_with_slack(topic_hash, peer)
                                && score(peer) >= 0.0
                                && outbound_peers.contains(peer)
                        },
                    );
                    for peer in &peer_list {
                        let current_topic = to_graft.entry(peer.clone()).or_insert_with(Vec::new);
                        current_topic.push(topic_hash.clone());
                    }
                    // update the mesh
                    debug!("Updating mesh, new mesh: {:?}", peer_list);
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
                    peers_by_score
                        .sort_by(|p1, p2| score(p1).partial_cmp(&score(p2)).unwrap_or(Equal));

                    let middle = peers_by_score.len() / 2;
                    let median = if peers_by_score.len() % 2 == 0 {
                        (score(
                            *peers_by_score.get(middle - 1).expect(
                                "middle < vector length and middle > 0 since peers.len() > 0",
                            ),
                        ) + score(*peers_by_score.get(middle).expect("middle < vector length")))
                            * 0.5
                    } else {
                        score(*peers_by_score.get(middle).expect("middle < vector length"))
                    };

                    // if the median score is below the threshold, select a better peer (if any) and
                    // GRAFT
                    if median < thresholds.opportunistic_graft_threshold {
                        let peer_list = Self::get_random_peers(
                            topic_peers,
                            &self.peer_protocols,
                            topic_hash,
                            self.config.opportunistic_graft_peers(),
                            |peer| {
                                !peers.contains(peer)
                                    && !explicit_peers.contains(peer)
                                    && !backoffs.is_backoff_with_slack(topic_hash, peer)
                                    && score(peer) > median
                            },
                        );
                        for peer in &peer_list {
                            let current_topic =
                                to_graft.entry(peer.clone()).or_insert_with(Vec::new);
                            current_topic.push(topic_hash.clone());
                        }
                        // update the mesh
                        debug!(
                            "Opportunistically graft in topic {} with peers {:?}",
                            topic_hash, peer_list
                        );
                        peers.extend(peer_list);
                    }
                }
            }
        }

        // remove expired fanout topics
        {
            let fanout = &mut self.fanout; // help the borrow checker
            let fanout_ttl = self.config.fanout_ttl();
            self.fanout_last_pub.retain(|topic_hash, last_pub_time| {
                if *last_pub_time + fanout_ttl < Instant::now() {
                    debug!(
                        "HEARTBEAT: Fanout topic removed due to timeout. Topic: {:?}",
                        topic_hash
                    );
                    fanout.remove(&topic_hash);
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
            for peer in peers.iter() {
                // is the peer still subscribed to the topic?
                match self.peer_topics.get(peer) {
                    Some(topics) => {
                        if !topics.contains(&topic_hash) || score(peer) < publish_threshold {
                            debug!(
                                "HEARTBEAT: Peer removed from fanout for topic: {:?}",
                                topic_hash
                            );
                            to_remove_peers.push(peer.clone());
                        }
                    }
                    None => {
                        // remove if the peer has disconnected
                        to_remove_peers.push(peer.clone());
                    }
                }
            }
            for to_remove in to_remove_peers {
                peers.remove(&to_remove);
            }

            // not enough peers
            if peers.len() < self.config.mesh_n() {
                debug!(
                    "HEARTBEAT: Fanout low. Contains: {:?} needs: {:?}",
                    peers.len(),
                    self.config.mesh_n()
                );
                let needed_peers = self.config.mesh_n() - peers.len();
                let explicit_peers = &self.explicit_peers;
                let new_peers = Self::get_random_peers(
                    &self.topic_peers,
                    &self.peer_protocols,
                    topic_hash,
                    needed_peers,
                    |peer| {
                        !peers.contains(peer)
                            && !explicit_peers.contains(peer)
                            && score(peer) < publish_threshold
                    },
                );
                peers.extend(new_peers);
            }
        }

        self.emit_gossip();

        // send graft/prunes
        if !to_graft.is_empty() | !to_prune.is_empty() {
            self.send_graft_prune(to_graft, to_prune, no_px);
        }

        // piggyback pooled control messages
        self.flush_control_pool();

        // shift the memcache
        self.mcache.shift();

        debug!("Completed Heartbeat");
        debug!("peer_scores: {:?}", scores);
    }

    /// Emits gossip - Send IHAVE messages to a random set of gossip peers. This is applied to mesh
    /// and fanout peers
    fn emit_gossip(&mut self) {
        let mut rng = thread_rng();
        for (topic_hash, peers) in self.mesh.iter().chain(self.fanout.iter()) {
            let mut message_ids = self.mcache.get_gossip_ids(&topic_hash);
            if message_ids.is_empty() {
                return;
            }

            // if we are emitting more than GossipSubMaxIHaveLength message_ids, truncate the list
            if message_ids.len() > self.config.max_ihave_length() {
                // we do the truncation (with shuffling) per peer below
                debug!(
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
            let to_msg_peers = Self::get_random_peers_dynamic(
                &self.topic_peers,
                &self.peer_protocols,
                &topic_hash,
                n_map,
                |peer| {
                    !peers.contains(peer)
                        && !self.explicit_peers.contains(peer)
                        && !self.score_below_threshold(peer, |ts| ts.gossip_threshold).0
                },
            );

            debug!("Gossiping IHAVE to {} peers.", to_msg_peers.len());

            for peer in to_msg_peers {
                let mut peer_message_ids = message_ids.clone();

                if peer_message_ids.len() > self.config.max_ihave_length() {
                    // We do this per peer so that we emit a different set for each peer.
                    // we have enough redundancy in the system that this will significantly increase
                    // the message coverage when we do truncate.
                    peer_message_ids.partial_shuffle(&mut rng, self.config.max_ihave_length());
                    peer_message_ids.truncate(self.config.max_ihave_length());
                }

                // send an IHAVE message
                Self::control_pool_add(
                    &mut self.control_pool,
                    peer.clone(),
                    GossipsubControlAction::IHave {
                        topic_hash: topic_hash.clone(),
                        message_ids: peer_message_ids,
                    },
                );
            }
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
        for (peer, topics) in to_graft.iter() {
            for topic in topics {
                //inform scoring of graft
                if let Some((peer_score, ..)) = &mut self.peer_score {
                    peer_score.graft(peer, topic.clone());
                }
            }
            let mut control_msgs: Vec<GossipsubControlAction> = topics
                .iter()
                .map(|topic_hash| GossipsubControlAction::Graft {
                    topic_hash: topic_hash.clone(),
                })
                .collect();

            // If there are prunes associated with the same peer add them.
            if let Some(topics) = to_prune.remove(peer) {
                let mut prunes = topics
                    .iter()
                    .map(|topic_hash| {
                        self.make_prune(
                            topic_hash,
                            peer,
                            self.config.do_px() && !no_px.contains(peer),
                        )
                    })
                    .collect::<Vec<_>>();
                control_msgs.append(&mut prunes);
            }

            // send the control messages
            self.send_message(
                peer.clone(),
                GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs,
                },
            );
        }

        // handle the remaining prunes
        for (peer, topics) in to_prune.iter() {
            let remaining_prunes = topics
                .iter()
                .map(|topic_hash| {
                    self.make_prune(
                        topic_hash,
                        peer,
                        self.config.do_px() && !no_px.contains(peer),
                    )
                })
                .collect();
            self.send_message(
                peer.clone(),
                GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: remaining_prunes,
                },
            );
        }
    }

    /// Helper function which forwards a message to mesh\[topic\] peers.
    /// Returns true if at least one peer was messaged.
    fn forward_msg(
        &mut self,
        message: GossipsubMessage,
        propagation_source: Option<&PeerId>,
    ) -> bool {
        let msg_id = (self.config.message_id_fn())(&message);

        // message is fully validated inform peer_score
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(peer) = propagation_source {
                peer_score.deliver_message(peer, &message);
            }
        }

        debug!("Forwarding message: {:?}", msg_id);
        let mut recipient_peers = HashSet::new();

        // add mesh peers
        for topic in &message.topics {
            // mesh
            if let Some(mesh_peers) = self.mesh.get(&topic) {
                for peer_id in mesh_peers {
                    if Some(peer_id) != propagation_source
                        && Some(peer_id) != message.source.as_ref()
                    {
                        recipient_peers.insert(peer_id.clone());
                    }
                }
            }
        }

        // Add explicit peers
        for p in &self.explicit_peers {
            if let Some(topics) = self.peer_topics.get(p) {
                if Some(p) != propagation_source
                    && Some(p) != message.source.as_ref()
                    && message.topics.iter().any(|t| topics.contains(t))
                {
                    recipient_peers.insert(p.clone());
                }
            }
        }

        // forward the message to peers
        if !recipient_peers.is_empty() {
            let event = Arc::new(GossipsubRpc {
                subscriptions: Vec::new(),
                messages: vec![message.clone()],
                control_msgs: Vec::new(),
            });

            for peer in recipient_peers.iter() {
                debug!("Sending message: {:?} to peer {:?}", msg_id, peer);
                self.send_message(peer.clone(), event.clone());
            }
            debug!("Completed forwarding message");
            true
        } else {
            false
        }
    }

    /// Constructs a `GossipsubMessage` performing message signing if required.
    pub(crate) fn build_message(
        &self,
        topics: Vec<TopicHash>,
        data: Vec<u8>,
    ) -> Result<GossipsubMessage, SigningError> {
        match &self.publish_config {
            PublishConfig::Signing {
                ref keypair,
                author,
                inline_key,
            } => {
                // Build and sign the message
                let sequence_number: u64 = rand::random();

                let signature = {
                    let message = rpc_proto::Message {
                        from: Some(author.clone().into_bytes()),
                        data: Some(data.clone()),
                        seqno: Some(sequence_number.to_be_bytes().to_vec()),
                        topic_ids: topics
                            .clone()
                            .into_iter()
                            .map(TopicHash::into_string)
                            .collect(),
                        signature: None,
                        key: None,
                    };

                    let mut buf = Vec::with_capacity(message.encoded_len());
                    message
                        .encode(&mut buf)
                        .expect("Buffer has sufficient capacity");

                    // the signature is over the bytes "libp2p-pubsub:<protobuf-message>"
                    let mut signature_bytes = SIGNING_PREFIX.to_vec();
                    signature_bytes.extend_from_slice(&buf);
                    Some(keypair.sign(&signature_bytes)?)
                };

                Ok(GossipsubMessage {
                    source: Some(author.clone()),
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: Some(sequence_number),
                    topics,
                    signature,
                    key: inline_key.clone(),
                    validated: true, // all published messages are valid
                })
            }
            PublishConfig::Author(peer_id) => {
                Ok(GossipsubMessage {
                    source: Some(peer_id.clone()),
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: Some(rand::random()),
                    topics,
                    signature: None,
                    key: None,
                    validated: true, // all published messages are valid
                })
            }
            PublishConfig::RandomAuthor => {
                Ok(GossipsubMessage {
                    source: Some(PeerId::random()),
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: Some(rand::random()),
                    topics,
                    signature: None,
                    key: None,
                    validated: true, // all published messages are valid
                })
            }
            PublishConfig::Anonymous => {
                Ok(GossipsubMessage {
                    source: None,
                    data,
                    // To be interoperable with the go-implementation this is treated as a 64-bit
                    // big-endian uint.
                    sequence_number: None,
                    topics,
                    signature: None,
                    key: None,
                    validated: true, // all published messages are valid
                })
            }
        }
    }

    /// Helper function to get a subset of random gossipsub peers for a `topic_hash`
    /// filtered by the function `f`. The number of peers to get equals the output of `n_map`
    /// that gets as input the number of filtered peers.
    fn get_random_peers_dynamic(
        topic_peers: &HashMap<TopicHash, BTreeSet<PeerId>>,
        peer_protocols: &HashMap<PeerId, PeerKind>,
        topic_hash: &TopicHash,
        // maps the number of total peers to the number of selected peers
        n_map: impl Fn(usize) -> usize,
        mut f: impl FnMut(&PeerId) -> bool,
    ) -> BTreeSet<PeerId> {
        let mut gossip_peers = match topic_peers.get(topic_hash) {
            // if they exist, filter the peers by `f`
            Some(peer_list) => peer_list
                .iter()
                .cloned()
                .filter(|p| {
                    f(p) && match peer_protocols.get(p) {
                        Some(PeerKind::Gossipsub) => true,
                        Some(PeerKind::Gossipsubv1_1) => true,
                        _ => false,
                    }
                })
                .collect(),
            None => Vec::new(),
        };

        // if we have less than needed, return them
        let n = n_map(gossip_peers.len());
        if gossip_peers.len() <= n {
            debug!("RANDOM PEERS: Got {:?} peers", gossip_peers.len());
            return gossip_peers.into_iter().collect();
        }

        // we have more peers than needed, shuffle them and return n of them
        let mut rng = thread_rng();
        gossip_peers.partial_shuffle(&mut rng, n);

        debug!("RANDOM PEERS: Got {:?} peers", n);

        gossip_peers.into_iter().take(n).collect()
    }

    /// Helper function to get a set of `n` random gossipsub peers for a `topic_hash`
    /// filtered by the function `f`.
    fn get_random_peers(
        topic_peers: &HashMap<TopicHash, BTreeSet<PeerId>>,
        peer_protocols: &HashMap<PeerId, PeerKind>,
        topic_hash: &TopicHash,
        n: usize,
        f: impl FnMut(&PeerId) -> bool,
    ) -> BTreeSet<PeerId> {
        Self::get_random_peers_dynamic(topic_peers, peer_protocols, topic_hash, |_| n, f)
    }

    // adds a control action to control_pool
    fn control_pool_add(
        control_pool: &mut HashMap<PeerId, Vec<GossipsubControlAction>>,
        peer: PeerId,
        control: GossipsubControlAction,
    ) {
        control_pool
            .entry(peer.clone())
            .or_insert_with(Vec::new)
            .push(control);
    }

    /// Takes each control action mapping and turns it into a message
    fn flush_control_pool(&mut self) {
        for (peer, controls) in self.control_pool.drain().collect::<Vec<_>>() {
            self.send_message(
                peer,
                GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: controls,
                },
            );
        }
    }

    /// Send a GossipsubRpc message to a peer. This will wrap the message in an arc if it
    /// is not already an arc.
    fn send_message(&mut self, peer_id: PeerId, message: impl Into<Arc<GossipsubRpc>>) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id,
                event: message.into(),
                handler: NotifyHandler::Any,
            })
    }
}

fn get_remote_addr(endpoint: &ConnectedPoint) -> &Multiaddr {
    match endpoint {
        ConnectedPoint::Dialer { address } => address,
        ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr,
    }
}

fn get_ip_addr(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|p| match p {
        Ip4(addr) => Some(IpAddr::V4(addr)),
        Ip6(addr) => Some(IpAddr::V6(addr)),
        _ => None,
    })
}

impl NetworkBehaviour for Gossipsub {
    type ProtocolsHandler = GossipsubHandler;
    type OutEvent = GossipsubEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        GossipsubHandler::new(
            self.config.protocol_id_prefix().clone(),
            self.config.max_transmit_size(),
            self.config.validation_mode().clone(),
            self.config.support_floodsub(),
        )
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        // Ignore connections from blacklisted peers.
        if self.blacklisted_peers.contains(peer_id) {
            debug!("Ignoring connection from blacklisted peer: {}", peer_id);
            return;
        }

        info!("New peer connected: {}", peer_id);
        // We need to send our subscriptions to the newly-connected node.
        let mut subscriptions = vec![];
        for topic_hash in self.mesh.keys() {
            subscriptions.push(GossipsubSubscription {
                topic_hash: topic_hash.clone(),
                action: GossipsubSubscriptionAction::Subscribe,
            });
        }

        if !subscriptions.is_empty() {
            // send our subscriptions to the peer
            self.send_message(
                peer_id.clone(),
                GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions,
                    control_msgs: Vec::new(),
                },
            );
        }

        // Insert an empty set of the topics of this peer until known.
        self.peer_topics.insert(peer_id.clone(), Default::default());

        // By default we assume a peer is only a floodsub peer.
        //
        // The protocol negotiation occurs once a message is sent/received. Once this happens we
        // update the type of peer that this is in order to determine which kind of routing should
        // occur.
        self.peer_protocols
            .entry(peer_id.clone())
            .or_insert(PeerKind::Floodsub);

        if let Some((peer_score, ..)) = &mut self.peer_score {
            peer_score.add_peer(peer_id.clone());
        }
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        // remove from mesh, topic_peers, peer_topic and the fanout
        debug!("Peer disconnected: {}", peer_id);
        {
            let topics = match self.peer_topics.get(peer_id) {
                Some(topics) => (topics),
                None => {
                    if !self.blacklisted_peers.contains(peer_id) {
                        warn!("Disconnected node, not in connected nodes");
                    }
                    return;
                }
            };

            // remove peer from all mappings
            for topic in topics {
                // check the mesh for the topic
                if let Some(mesh_peers) = self.mesh.get_mut(&topic) {
                    // check if the peer is in the mesh and remove it
                    mesh_peers.remove(peer_id);
                }

                // remove from topic_peers
                if let Some(peer_list) = self.topic_peers.get_mut(&topic) {
                    if !peer_list.remove(peer_id) {
                        // debugging purposes
                        warn!(
                            "Disconnected node: {} not in topic_peers peer list",
                            peer_id
                        );
                    }
                } else {
                    warn!(
                        "Disconnected node: {} with topic: {:?} not in topic_peers",
                        &peer_id, &topic
                    );
                }

                // remove from fanout
                self.fanout
                    .get_mut(&topic)
                    .map(|peers| peers.remove(peer_id));
            }

            //forget px and outbound status for this peer
            self.px_peers.remove(peer_id);
            self.outbound_peers.remove(peer_id);
        }

        // Remove peer from peer_topics and peer_protocols
        // NOTE: It is possible the peer has already been removed from all mappings if it does not
        // support the protocol.
        self.peer_topics.remove(peer_id);
        self.peer_protocols.remove(peer_id);

        if let Some((peer_score, ..)) = &mut self.peer_score {
            peer_score.remove_peer(peer_id);
        }
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        _: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        // Ignore connections from blacklisted peers.
        if self.blacklisted_peers.contains(peer_id) {
            return;
        }

        // Check if the peer is an outbound peer
        if let ConnectedPoint::Dialer { .. } = endpoint {
            // Diverging from the go implementation we only want to consider a peer as outbound peer
            // if its first connection is outbound. To check if this connection is the first we
            // check if the peer isn't connected yet. This only works because the
            // `inject_connection_established` event for the first connection gets called immediately
            // before `inject_connected` gets called.
            if !self.peer_topics.contains_key(peer_id) && !self.px_peers.contains(peer_id) {
                // The first connection is outbound and it is not a peer from peer exchange => mark
                // it as outbound peer
                self.outbound_peers.insert(peer_id.clone());
            }
        }

        // Add the IP to the peer scoring system
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(ip) = get_ip_addr(get_remote_addr(endpoint)) {
                peer_score.add_ip(&peer_id, ip);
            }
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        _: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        // Remove IP from peer scoring system
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(ip) = get_ip_addr(get_remote_addr(endpoint)) {
                peer_score.remove_ip(peer, &ip);
            }
        }
    }

    fn inject_address_change(
        &mut self,
        peer: &PeerId,
        _: &ConnectionId,
        endpoint_old: &ConnectedPoint,
        endpoint_new: &ConnectedPoint,
    ) {
        // Exchange IP in peer scoring system
        if let Some((peer_score, ..)) = &mut self.peer_score {
            if let Some(ip) = get_ip_addr(get_remote_addr(endpoint_old)) {
                peer_score.remove_ip(peer, &ip);
            }
            if let Some(ip) = get_ip_addr(get_remote_addr(endpoint_new)) {
                peer_score.add_ip(&peer, ip);
            }
        }
    }

    fn inject_event(
        &mut self,
        propagation_source: PeerId,
        _: ConnectionId,
        handler_event: HandlerEvent,
    ) {
        match handler_event {
            HandlerEvent::PeerKind(kind) => {
                // We have identified the protocol this peer is using
                if let PeerKind::NotSupported = kind {
                    // We treat this peer as disconnected
                    self.inject_disconnected(&propagation_source);
                } else if let Some(old_kind) = self.peer_protocols.get_mut(&propagation_source) {
                    // Only change the value if the old value is Floodsub (the default set in
                    // inject_connected). All other PeerKind changes are ignored.
                    if let PeerKind::Floodsub = *old_kind {
                        *old_kind = kind;
                    }
                }
            }
            HandlerEvent::Message {
                rpc,
                invalid_messages,
            } => {
                // Handle any invalid messages from this peer
                if let Some((peer_score, .., gossip_promises)) = &mut self.peer_score {
                    let id_fn = self.config.message_id_fn();
                    for (message, validation_error) in invalid_messages {
                        warn!("Message rejected. Reason: {:?}", validation_error);
                        let reason = RejectReason::ValidationError(validation_error);
                        peer_score.reject_message(&propagation_source, &message, reason);
                        gossip_promises.reject_message(&id_fn(&message), &reason);
                    }
                }

                // Handle the Gossipsub RPC

                // Check if peer is graylisted in which case we ignore the event
                if let (true, _) =
                    self.score_below_threshold(&propagation_source, |pst| pst.graylist_threshold)
                {
                    debug!("RPC Dropped from greylisted peer {}", propagation_source);
                    return;
                }

                // Handle subscriptions
                // Update connected peers topics
                if !rpc.subscriptions.is_empty() {
                    self.handle_received_subscriptions(&rpc.subscriptions, &propagation_source);
                }

                // Handle messages
                for message in rpc.messages {
                    self.handle_received_message(message, &propagation_source);
                }

                // Handle control messages
                // group some control messages, this minimises SendEvents (code is simplified to handle each event at a time however)
                let mut ihave_msgs = vec![];
                let mut graft_msgs = vec![];
                let mut prune_msgs = vec![];
                for control_msg in rpc.control_msgs {
                    match control_msg {
                        GossipsubControlAction::IHave {
                            topic_hash,
                            message_ids,
                        } => {
                            ihave_msgs.push((topic_hash, message_ids));
                        }
                        GossipsubControlAction::IWant { message_ids } => {
                            self.handle_iwant(&propagation_source, message_ids)
                        }
                        GossipsubControlAction::Graft { topic_hash } => graft_msgs.push(topic_hash),
                        GossipsubControlAction::Prune {
                            topic_hash,
                            peers,
                            backoff,
                        } => prune_msgs.push((topic_hash, peers, backoff)),
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

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(match event {
                NetworkBehaviourAction::NotifyHandler {
                    peer_id,
                    handler,
                    event: send_event,
                } => {
                    // clone send event reference if others references are present
                    let event = Arc::try_unwrap(send_event).unwrap_or_else(|e| (*e).clone());
                    NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        event,
                        handler,
                    }
                }
                NetworkBehaviourAction::GenerateEvent(e) => {
                    NetworkBehaviourAction::GenerateEvent(e)
                }
                NetworkBehaviourAction::DialAddress { address } => {
                    NetworkBehaviourAction::DialAddress { address }
                }
                NetworkBehaviourAction::DialPeer { peer_id, condition } => {
                    NetworkBehaviourAction::DialPeer { peer_id, condition }
                }
                NetworkBehaviourAction::ReportObservedAddr { address } => {
                    NetworkBehaviourAction::ReportObservedAddr { address }
                }
            });
        }

        // update scores
        if let Some((peer_score, _, interval, _)) = &mut self.peer_score {
            while let Poll::Ready(Some(())) = interval.poll_next_unpin(cx) {
                peer_score.refresh_scores();
            }
        }

        while let Poll::Ready(Some(())) = self.heartbeat.poll_next_unpin(cx) {
            self.heartbeat();
        }

        Poll::Pending
    }
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

impl fmt::Debug for Gossipsub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gossipsub")
            .field("config", &self.config)
            .field("events", &self.events)
            .field("control_pool", &self.control_pool)
            .field("publish_config", &self.publish_config)
            .field("topic_peers", &self.topic_peers)
            .field("peer_topics", &self.peer_topics)
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
                f.write_fmt(format_args!("PublishConfig::Signing({})", author))
            }
            PublishConfig::Author(author) => {
                f.write_fmt(format_args!("PublishConfig::Author({})", author))
            }
            PublishConfig::RandomAuthor => f.write_fmt(format_args!("PublishConfig::RandomAuthor")),
            PublishConfig::Anonymous => f.write_fmt(format_args!("PublishConfig::Anonymous")),
        }
    }
}
