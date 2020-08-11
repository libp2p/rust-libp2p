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

use crate::config::{GossipsubConfig, ValidationMode};
use crate::error::PublishError;
use crate::handler::GossipsubHandler;
use crate::mcache::MessageCache;
use crate::protocol::{
    GossipsubControlAction, GossipsubMessage, GossipsubSubscription, GossipsubSubscriptionAction,
    MessageId, SIGNING_PREFIX,
};
use crate::rpc_proto;
use crate::topic::{Topic, TopicHash};
use futures::prelude::*;
use libp2p_core::{
    connection::ConnectionId, identity::error::SigningError, identity::Keypair, Multiaddr, PeerId,
};
use libp2p_swarm::{
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, ProtocolsHandler,
};
use log::{debug, error, info, trace, warn};
use lru_time_cache::LruCache;
use prost::Message;
use rand;
use rand::{seq::SliceRandom, thread_rng};
use std::{
    collections::HashSet,
    collections::VecDeque,
    collections::{hash_map::HashMap, BTreeSet},
    fmt, iter,
    sync::Arc,
    task::{Context, Poll},
};
use wasm_timer::{Instant, Interval};

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
    fn is_signing(&self) -> bool {
        match self {
            MessageAuthenticity::Signed(_) => true,
            _ => false,
        }
    }

    fn is_anonymous(&self) -> bool {
        match self {
            MessageAuthenticity::Anonymous => true,
            _ => false,
        }
    }
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
    duplication_cache: LruCache<MessageId, ()>,

    /// A map of all connected peers - A map of topic hash to a list of gossipsub peer Ids.
    topic_peers: HashMap<TopicHash, BTreeSet<PeerId>>,

    /// A map of all connected peers to their subscribed topics.
    peer_topics: HashMap<PeerId, BTreeSet<TopicHash>>,

    /// Overlay network of connected peers - Maps topics to connected gossipsub peers.
    mesh: HashMap<TopicHash, BTreeSet<PeerId>>,

    /// Map of topics to list of peers that we publish to, but don't subscribe to.
    fanout: HashMap<TopicHash, BTreeSet<PeerId>>,

    /// The last publish time for fanout topics.
    fanout_last_pub: HashMap<TopicHash, Instant>,

    /// Message cache for the last few heartbeats.
    mcache: MessageCache,

    /// Heartbeat interval stream.
    heartbeat: Interval,
}

impl Gossipsub {
    /// Creates a `Gossipsub` struct given a set of parameters specified via a `GossipsubConfig`.
    pub fn new(privacy: MessageAuthenticity, config: GossipsubConfig) -> Self {
        // Set up the router given the configuration settings.

        // We do not allow configurations where a published message would also be rejected if it
        // were received locally.
        validate_config(&privacy, &config.validation_mode);

        // Set up message publishing parameters.

        Gossipsub {
            events: VecDeque::new(),
            control_pool: HashMap::new(),
            publish_config: privacy.into(),
            duplication_cache: LruCache::with_expiry_duration(config.duplicate_cache_time),
            topic_peers: HashMap::new(),
            peer_topics: HashMap::new(),
            mesh: HashMap::new(),
            fanout: HashMap::new(),
            fanout_last_pub: HashMap::new(),
            mcache: MessageCache::new(
                config.history_gossip,
                config.history_length,
                config.message_id_fn,
            ),
            heartbeat: Interval::new_at(
                Instant::now() + config.heartbeat_initial_delay,
                config.heartbeat_interval,
            ),
            config,
        }
    }

    /// Subscribe to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        debug!("Subscribing to topic: {}", topic);
        let topic_hash = self.topic_hash(topic.clone());
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
    pub fn unsubscribe(&mut self, topic: Topic) -> bool {
        debug!("Unsubscribing from topic: {}", topic);
        let topic_hash = &self.topic_hash(topic);

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
    pub fn publish(&mut self, topic: &Topic, data: impl Into<Vec<u8>>) -> Result<(), PublishError> {
        self.publish_many(iter::once(topic.clone()), data)
    }

    /// Publishes a message with multiple topics to the network.
    pub fn publish_many(
        &mut self,
        topics: impl IntoIterator<Item = Topic>,
        data: impl Into<Vec<u8>>,
    ) -> Result<(), PublishError> {
        let message = self.build_message(
            topics.into_iter().map(|t| self.topic_hash(t)).collect(),
            data.into(),
        )?;
        let msg_id = (self.config.message_id_fn)(&message);

        // Add published message to the duplicate cache.
        if self.duplication_cache.insert(msg_id.clone(), ()).is_some() {
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

        // Forward the message to mesh peers.
        let mesh_peers_sent = self.forward_msg(message.clone(), None);

        let mut recipient_peers = HashSet::new();
        for topic_hash in &message.topics {
            // If not subscribed to the topic, use fanout peers.
            if self.mesh.get(&topic_hash).is_none() {
                debug!("Topic: {:?} not in the mesh", topic_hash);
                // Build a list of peers to forward the message to
                // if we have fanout peers add them to the map.
                if self.fanout.contains_key(&topic_hash) {
                    for peer in self.fanout.get(&topic_hash).expect("Topic must exist") {
                        recipient_peers.insert(peer.clone());
                    }
                } else {
                    // we have no fanout peers, select mesh_n of them and add them to the fanout
                    let mesh_n = self.config.mesh_n;
                    let new_peers =
                        Self::get_random_peers(&self.topic_peers, &topic_hash, mesh_n, {
                            |_| true
                        });
                    // add the new peers to the fanout and recipient peers
                    self.fanout.insert(topic_hash.clone(), new_peers.clone());
                    for peer in new_peers {
                        debug!("Peer added to fanout: {:?}", peer);
                        recipient_peers.insert(peer.clone());
                    }
                }
                // we are publishing to fanout peers - update the time we published
                self.fanout_last_pub
                    .insert(topic_hash.clone(), Instant::now());
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
        // Send to peers we know are subscribed to the topic.
        for peer_id in recipient_peers.iter() {
            debug!("Sending message to peer: {:?}", peer_id);
            self.send_message(peer_id.clone(), event.clone());
        }

        info!("Published message: {:?}", msg_id);
        Ok(())
    }

    /// This function should be called when `config.validate_messages` is `true` in order to
    /// validate and propagate messages. Messages are stored in the ['Memcache'] and validation is expected to be
    /// fast enough that the messages should still exist in the cache.
    ///
    /// Calling this function will propagate a message stored in the cache, if it still exists.
    /// If the message still exists in the cache, it will be forwarded and this function will return true,
    /// otherwise it will return false.
    ///
    /// The `propagation_source` parameter indicates who the message was received by and will not
    /// be forwarded back to that peer.
    ///
    /// This should only be called once per message.
    pub fn validate_message(
        &mut self,
        message_id: &MessageId,
        propagation_source: &PeerId,
    ) -> bool {
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
        true
    }

    /// Gossipsub JOIN(topic) - adds topic peers to mesh and sends them GRAFT messages.
    fn join(&mut self, topic_hash: &TopicHash) {
        debug!("Running JOIN for topic: {:?}", topic_hash);

        // if we are already in the mesh, return
        if self.mesh.contains_key(topic_hash) {
            info!("JOIN: The topic is already in the mesh, ignoring JOIN");
            return;
        }

        let mut added_peers = vec![];

        // check if we have mesh_n peers in fanout[topic] and add them to the mesh if we do,
        // removing the fanout entry.
        if let Some((_, peers)) = self.fanout.remove_entry(topic_hash) {
            debug!(
                "JOIN: Removing peers from the fanout for topic: {:?}",
                topic_hash
            );
            // add up to mesh_n of them them to the mesh
            // Note: These aren't randomly added, currently FIFO
            let add_peers = std::cmp::min(peers.len(), self.config.mesh_n);
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
        if added_peers.len() < self.config.mesh_n {
            // get the peers
            let new_peers = Self::get_random_peers(
                &self.topic_peers,
                topic_hash,
                self.config.mesh_n - added_peers.len(),
                |peer| !added_peers.contains(peer),
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

    /// Gossipsub LEAVE(topic) - Notifies mesh\[topic\] peers with PRUNE messages.
    fn leave(&mut self, topic_hash: &TopicHash) {
        debug!("Running LEAVE for topic {:?}", topic_hash);

        // if our mesh contains the topic, send prune to peers and delete it from the mesh
        if let Some((_, peers)) = self.mesh.remove_entry(topic_hash) {
            for peer in peers {
                // Send a PRUNE control message
                info!("LEAVE: Sending PRUNE to peer: {:?}", peer);
                Self::control_pool_add(
                    &mut self.control_pool,
                    peer.clone(),
                    GossipsubControlAction::Prune {
                        topic_hash: topic_hash.clone(),
                    },
                );
            }
        }
        debug!("Completed LEAVE for topic: {:?}", topic_hash);
    }

    /// Handles an IHAVE control message. Checks our cache of messages. If the message is unknown,
    /// requests it with an IWANT control message.
    fn handle_ihave(&mut self, peer_id: &PeerId, ihave_msgs: Vec<(TopicHash, Vec<MessageId>)>) {
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
            // Send the list of IWANT control messages
            debug!("IHAVE: Sending IWANT message");
            Self::control_pool_add(
                &mut self.control_pool,
                peer_id.clone(),
                GossipsubControlAction::IWant {
                    message_ids: iwant_ids.iter().cloned().collect(),
                },
            );
        }
        debug!("Completed IHAVE handling for peer: {:?}", peer_id);
    }

    /// Handles an IWANT control message. Checks our cache of messages. If the message exists it is
    /// forwarded to the requesting peer.
    fn handle_iwant(&mut self, peer_id: &PeerId, iwant_msgs: Vec<MessageId>) {
        debug!("Handling IWANT for peer: {:?}", peer_id);
        // build a hashmap of available messages
        let mut cached_messages = HashMap::new();

        for id in iwant_msgs {
            // if we have it, add it do the cached_messages mapping
            if let Some(msg) = self.mcache.get(&id) {
                cached_messages.insert(id.clone(), msg.clone());
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
        debug!("Completed IWANT handling for peer: {:?}", peer_id);
    }

    /// Handles GRAFT control messages. If subscribed to the topic, adds the peer to mesh, if not,
    /// responds with PRUNE messages.
    fn handle_graft(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        debug!("Handling GRAFT message for peer: {:?}", peer_id);

        let mut to_prune_topics = HashSet::new();
        for topic_hash in topics {
            if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                // if we are subscribed, add peer to the mesh, if not already added
                info!(
                    "GRAFT: Mesh link added for peer: {:?} in topic: {:?}",
                    peer_id, topic_hash
                );
                // Duplicates are ignored
                peers.insert(peer_id.clone());
            } else {
                to_prune_topics.insert(topic_hash.clone());
            }
        }

        if !to_prune_topics.is_empty() {
            // build the prune messages to send
            let prune_messages = to_prune_topics
                .iter()
                .map(|t| GossipsubControlAction::Prune {
                    topic_hash: t.clone(),
                })
                .collect();
            // Send the prune messages to the peer
            info!(
                "GRAFT: Not subscribed to topics -  Sending PRUNE to peer: {:?}",
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
        debug!("Completed GRAFT handling for peer: {:?}", peer_id);
    }

    /// Handles PRUNE control messages. Removes peer from the mesh.
    fn handle_prune(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        debug!("Handling PRUNE message for peer: {}", peer_id.to_string());
        for topic_hash in topics {
            if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                // remove the peer if it exists in the mesh
                if peers.remove(peer_id) {
                    info!(
                        "PRUNE: Removing peer: {} from the mesh for topic: {:?}",
                        peer_id.to_string(),
                        topic_hash
                    );
                }
            }
        }
        debug!("Completed PRUNE handling for peer: {}", peer_id.to_string());
    }

    /// Handles a newly received GossipsubMessage.
    /// Forwards the message to all peers in the mesh.
    fn handle_received_message(&mut self, mut msg: GossipsubMessage, propagation_source: &PeerId) {
        let msg_id = (self.config.message_id_fn)(&msg);
        debug!(
            "Handling message: {:?} from peer: {}",
            msg_id,
            propagation_source.to_string()
        );

        // If we are not validating messages, assume this message is validated
        // This will allow the message to be gossiped without explicitly calling
        // `validate_message`.
        if !self.config.validate_messages {
            msg.validated = true;
        }

        // Add the message to the duplication cache and memcache.
        if self.duplication_cache.insert(msg_id.clone(), ()).is_some() {
            debug!("Message already received, ignoring. Message: {:?}", msg_id);
            return;
        }
        self.mcache.put(msg.clone());

        // dispatch the message to the user
        if self.mesh.keys().any(|t| msg.topics.iter().any(|u| t == u)) {
            debug!("Sending received message to user");
            self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                GossipsubEvent::Message(propagation_source.clone(), msg_id, msg.clone()),
            ));
        }

        // forward the message to mesh peers, if no validation is required
        if !self.config.validate_messages {
            let message_id = (self.config.message_id_fn)(&msg);
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
            // get the peers from the mapping, or insert empty lists if topic doesn't exist
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
                    if let Some(peers) = self.mesh.get_mut(&subscription.topic_hash) {
                        if peers.len() < self.config.mesh_n_low {
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
                                grafts.push(GossipsubControlAction::Graft {
                                    topic_hash: subscription.topic_hash.clone(),
                                });
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

    /// Heartbeat function which shifts the memcache and updates the mesh.
    fn heartbeat(&mut self) {
        debug!("Starting heartbeat");

        let mut to_graft = HashMap::new();
        let mut to_prune = HashMap::new();

        // maintain the mesh for each topic
        for (topic_hash, peers) in self.mesh.iter_mut() {
            // too little peers - add some
            if peers.len() < self.config.mesh_n_low {
                debug!(
                    "HEARTBEAT: Mesh low. Topic: {} Contains: {} needs: {}",
                    topic_hash,
                    peers.len(),
                    self.config.mesh_n_low
                );
                // not enough peers - get mesh_n - current_length more
                let desired_peers = self.config.mesh_n - peers.len();
                let peer_list =
                    Self::get_random_peers(&self.topic_peers, topic_hash, desired_peers, {
                        |peer| !peers.contains(peer)
                    });
                for peer in &peer_list {
                    let current_topic = to_graft.entry(peer.clone()).or_insert_with(Vec::new);
                    current_topic.push(topic_hash.clone());
                }
                // update the mesh
                debug!("Updating mesh, new mesh: {:?}", peer_list);
                peers.extend(peer_list);
            }

            // too many peers - remove some
            if peers.len() > self.config.mesh_n_high {
                debug!(
                    "HEARTBEAT: Mesh high. Topic: {} Contains: {} needs: {}",
                    topic_hash,
                    peers.len(),
                    self.config.mesh_n_high
                );
                let excess_peer_no = peers.len() - self.config.mesh_n;
                // shuffle the peers
                let mut rng = thread_rng();
                let mut shuffled = peers.iter().cloned().collect::<Vec<_>>();
                shuffled.shuffle(&mut rng);
                // remove the first excess_peer_no peers adding them to to_prune
                for _ in 0..excess_peer_no {
                    let peer = shuffled
                        .pop()
                        .expect("There should always be enough peers to remove");
                    peers.remove(&peer);
                    let current_topic = to_prune.entry(peer).or_insert_with(Vec::new);
                    current_topic.push(topic_hash.clone());
                }
            }
        }

        // remove expired fanout topics
        {
            let fanout = &mut self.fanout; // help the borrow checker
            let fanout_ttl = self.config.fanout_ttl;
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
            for peer in peers.iter() {
                // is the peer still subscribed to the topic?
                match self.peer_topics.get(peer) {
                    Some(topics) => {
                        if !topics.contains(&topic_hash) {
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
            if peers.len() < self.config.mesh_n {
                debug!(
                    "HEARTBEAT: Fanout low. Contains: {:?} needs: {:?}",
                    peers.len(),
                    self.config.mesh_n
                );
                let needed_peers = self.config.mesh_n - peers.len();
                let new_peers =
                    Self::get_random_peers(&self.topic_peers, topic_hash, needed_peers, |peer| {
                        !peers.contains(peer)
                    });
                peers.extend(new_peers);
            }
        }

        self.emit_gossip();

        // send graft/prunes
        if !to_graft.is_empty() | !to_prune.is_empty() {
            self.send_graft_prune(to_graft, to_prune);
        }

        // piggyback pooled control messages
        self.flush_control_pool();

        // shift the memcache
        self.mcache.shift();
        debug!("Completed Heartbeat");
    }

    /// Emits gossip - Send IHAVE messages to a random set of gossip peers. This is applied to mesh
    /// and fanout peers
    fn emit_gossip(&mut self) {
        for (topic_hash, peers) in self.mesh.iter().chain(self.fanout.iter()) {
            let message_ids = self.mcache.get_gossip_ids(&topic_hash);
            if message_ids.is_empty() {
                return;
            }

            // get gossip_lazy random peers
            let to_msg_peers = Self::get_random_peers(
                &self.topic_peers,
                &topic_hash,
                self.config.gossip_lazy,
                |peer| !peers.contains(peer),
            );

            debug!("Gossiping IHAVE to {} peers.", to_msg_peers.len());

            for peer in to_msg_peers {
                // send an IHAVE message
                Self::control_pool_add(
                    &mut self.control_pool,
                    peer.clone(),
                    GossipsubControlAction::IHave {
                        topic_hash: topic_hash.clone(),
                        message_ids: message_ids.clone(),
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
    ) {
        // handle the grafts and overlapping prunes per peer
        for (peer, topics) in to_graft.iter() {
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
                    .map(|topic_hash| GossipsubControlAction::Prune {
                        topic_hash: topic_hash.clone(),
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
                .map(|topic_hash| GossipsubControlAction::Prune {
                    topic_hash: topic_hash.clone(),
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
    fn forward_msg(&mut self, message: GossipsubMessage, source: Option<&PeerId>) -> bool {
        let msg_id = (self.config.message_id_fn)(&message);
        debug!("Forwarding message: {:?}", msg_id);
        let mut recipient_peers = HashSet::new();

        // add mesh peers
        for topic in &message.topics {
            // mesh
            if let Some(mesh_peers) = self.mesh.get(&topic) {
                for peer_id in mesh_peers {
                    if Some(peer_id) != source {
                        recipient_peers.insert(peer_id.clone());
                    }
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
                        topic_ids: topics.clone().into_iter().map(|t| t.into()).collect(),
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

    /// Helper function to get a set of `n` random gossipsub peers for a `topic_hash`
    /// filtered by the function `f`.
    fn get_random_peers(
        topic_peers: &HashMap<TopicHash, BTreeSet<PeerId>>,
        topic_hash: &TopicHash,
        n: usize,
        mut f: impl FnMut(&PeerId) -> bool,
    ) -> BTreeSet<PeerId> {
        let mut gossip_peers = match topic_peers.get(topic_hash) {
            // if they exist, filter the peers by `f`
            Some(peer_list) => peer_list.iter().cloned().filter(|p| f(p)).collect(),
            None => Vec::new(),
        };

        // if we have less than needed, return them
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

    /// Produces a `TopicHash` for a topic given the gossipsub configuration.
    fn topic_hash(&self, topic: Topic) -> TopicHash {
        if self.config.hash_topics {
            topic.sha256_hash()
        } else {
            topic.no_hash()
        }
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

impl NetworkBehaviour for Gossipsub {
    type ProtocolsHandler = GossipsubHandler;
    type OutEvent = GossipsubEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        GossipsubHandler::new(
            self.config.protocol_id.clone(),
            self.config.max_transmit_size,
            self.config.validation_mode.clone(),
        )
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, id: &PeerId) {
        info!("New peer connected: {:?}", id);
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
                id.clone(),
                GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions,
                    control_msgs: Vec::new(),
                },
            );
        }

        // For the time being assume all gossipsub peers
        self.peer_topics.insert(id.clone(), Default::default());
    }

    fn inject_disconnected(&mut self, id: &PeerId) {
        // remove from mesh, topic_peers, peer_topic and fanout
        debug!("Peer disconnected: {:?}", id);
        {
            let topics = match self.peer_topics.get(id) {
                Some(topics) => (topics),
                None => {
                    warn!("Disconnected node, not in connected nodes");
                    return;
                }
            };

            // remove peer from all mappings
            for topic in topics {
                // check the mesh for the topic
                if let Some(mesh_peers) = self.mesh.get_mut(&topic) {
                    // check if the peer is in the mesh and remove it
                    mesh_peers.remove(id);
                }

                // remove from topic_peers
                if let Some(peer_list) = self.topic_peers.get_mut(&topic) {
                    if !peer_list.remove(id) {
                        // debugging purposes
                        warn!("Disconnected node: {:?} not in topic_peers peer list", &id);
                    }
                } else {
                    warn!(
                        "Disconnected node: {:?} with topic: {:?} not in topic_peers",
                        &id, &topic
                    );
                }

                // remove from fanout
                self.fanout.get_mut(&topic).map(|peers| peers.remove(id));
            }
        }

        // remove peer from peer_topics
        let was_in = self.peer_topics.remove(id);
        debug_assert!(was_in.is_some());
    }

    fn inject_event(&mut self, propagation_source: PeerId, _: ConnectionId, event: GossipsubRpc) {
        // Handle subscriptions
        // Update connected peers topics
        if !event.subscriptions.is_empty() {
            self.handle_received_subscriptions(&event.subscriptions, &propagation_source);
        }

        // Handle messages
        for message in event.messages {
            self.handle_received_message(message, &propagation_source);
        }

        // Handle control messages
        // group some control messages, this minimises SendEvents (code is simplified to handle each event at a time however)
        let mut ihave_msgs = vec![];
        let mut graft_msgs = vec![];
        let mut prune_msgs = vec![];
        for control_msg in event.control_msgs {
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
                GossipsubControlAction::Prune { topic_hash } => prune_msgs.push(topic_hash),
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

        while let Poll::Ready(Some(())) = self.heartbeat.poll_next_unpin(cx) {
            self.heartbeat();
        }

        Poll::Pending
    }
}

/// An RPC received/sent.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GossipsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<GossipsubMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<GossipsubSubscription>,
    /// List of Gossipsub control messages.
    pub control_msgs: Vec<GossipsubControlAction>,
}

impl fmt::Debug for GossipsubRpc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut b = f.debug_struct("GossipsubRpc");
        if !self.messages.is_empty() {
            b.field("messages", &self.messages);
        }
        if !self.subscriptions.is_empty() {
            b.field("subscriptions", &self.subscriptions);
        }
        if !self.control_msgs.is_empty() {
            b.field("control_msgs", &self.control_msgs);
        }
        b.finish()
    }
}

/// Event that can happen on the gossipsub behaviour.
#[derive(Debug)]
pub enum GossipsubEvent {
    /// A message has been received. This contains the PeerId that we received the message from,
    /// the message id (used if the application layer needs to propagate the message) and the
    /// message itself.
    Message(PeerId, MessageId, GossipsubMessage),

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

/// Validates the combination of signing, privacy and message validation to ensure the
/// configuration will not reject published messages.
fn validate_config(authenticity: &MessageAuthenticity, validation_mode: &ValidationMode) {
    match validation_mode {
        ValidationMode::Anonymous => {
            if authenticity.is_signing() {
                panic!("Cannot enable message signing with an Anonymous validation mode. Consider changing either the ValidationMode or MessageAuthenticity");
            }

            if !authenticity.is_anonymous() {
                panic!("Published messages contain an author but incoming messages with an author will be rejected. Consider adjusting the validation or privacy settings in the config");
            }
        }
        ValidationMode::Strict => {
            if !authenticity.is_signing() {
                panic!(
                    "Messages will be
                published unsigned and incoming unsigned messages will be rejected. Consider adjusting
                the validation or privacy settings in the config"
                );
            }
        }
        _ => {}
    }
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
            PublishConfig::Signing { author, .. } => f.write_fmt(format_args!("PublishConfig::Signing({})", author)), 
            PublishConfig::Author(author) => f.write_fmt(format_args!("PublishConfig::Author({})", author)), 
            PublishConfig::RandomAuthor => f.write_fmt(format_args!("PublishConfig::RandomAuthor")), 
            PublishConfig::Anonymous => f.write_fmt(format_args!("PublishConfig::Anonymous")), 
        }
    }
}

