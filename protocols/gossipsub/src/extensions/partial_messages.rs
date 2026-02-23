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
    cmp::max,
    collections::{BTreeSet, HashMap, HashSet},
    fmt::Debug,
};

use libp2p_core::PeerId;
use rand::seq::IteratorRandom;

use crate::{
    types::{RpcOut, SubscriptionOpts},
    PublishError, TopicHash,
};

/// Default TTL for partial messages kept.
pub(crate) const DEFAULT_PARTIAL_TTL: usize = 5;

/// PartialMessage is a message that can be broken up into parts.
/// This trait allows applications to define custom strategies for splitting large messages
/// into parts and reconstructing them from received partial data. It provides the core
/// operations needed for the gossipsub partial messages extension.
///
/// The partial message protocol works as follows:
/// 1. Applications implement this trait to define how messages are split and reconstructed
/// 2. Peers advertise available parts using `available_parts()` metadata in PartialIHAVE
/// 3. Peers request missing parts using `missing_parts()` metadata in PartialIWANT
/// 4. When requests are received, `partial_message_bytes_from_metadata()` generates the response
/// 5. Received partial data is integrated using `extend_from_encoded_partial_message()`
/// 6. The `group_id()` ties all parts of the same logical message together
pub trait Partial: Send + Sync {
    /// Returns the unique identifier for this message group.
    ///
    /// All partial messages belonging to the same logical message should return
    /// the same group ID. This is used to associate partial messages together
    /// during reconstruction.
    fn group_id(&self) -> Vec<u8>;

    /// Returns application defined metadata describing which parts of the message
    /// are available and which parts we want.
    ///
    /// The returned bytes will be sent in partsMetadata field to advertise
    /// available and wanted parts to peers.
    fn metadata(&self) -> Box<dyn Metadata>;

    /// Generates an action from the given metadata.
    ///
    /// When a peer requests specific parts (via PartialIWANT), this method
    /// generates the actual message data to send back. The `metadata` parameter
    /// describes what parts are being requested.
    ///
    /// Returns a [`PartialAction`] for the given metadata, or an error.
    fn partial_action_from_metadata(
        &self,
        peer_id: PeerId,
        metadata: Option<&[u8]>,
    ) -> Result<PartialAction, PartialError>;
}

pub trait Metadata: Debug + Send + Sync {
    /// Return the `Metadata` as a byte slice.
    fn as_slice(&self) -> &[u8];
    /// Attempts to update the `Metadata` with the remote metadata,
    /// Returns `true` if the `Metadata` was updated.
    fn update(&mut self, data: &[u8]) -> Result<bool, PartialError>;
    /// Attempts to update the local `Metadata` with the remote data received.
    /// This method is used to track the metadata that the remote peer believes the local system
    /// has. The default returns [`Ok(())`](Ok) as it is an optimization, indicating that the update
    /// logic has not been triggered in this implementation.
    fn update_from_data(&mut self, _data: &[u8]) -> Result<(), PartialError> {
        Ok(())
    }
}

/// Indicates the action to take for the given metadata.
pub struct PartialAction {
    /// Indicate if we want remote data from the peer.
    pub need: bool,
    /// Indicate if we have data to send for that peer
    pub send: Option<(Vec<u8>, Box<dyn Metadata>)>,
}

/// Partial message state for sent and received messages.
#[derive(Default)]
pub(crate) struct State {
    /// Our subscription options per topic and respective cached partial messages we're publishing.
    pub(crate) subscriptions: HashMap<TopicHash, LocalSubscription>,
    /// Per-peer partial state
    pub(crate) peer_subscriptions: HashMap<TopicHash, HashMap<PeerId, RemoteSubscription>>,
}

impl State {
    /// Called by the [`Behaviour`](crate::Behaviour) when we subscribed to the topic.
    pub(crate) fn subscribe(
        &mut self,
        topic_hash: TopicHash,
        supports_partial: bool,
        requests_partial: bool,
    ) {
        self.subscriptions.insert(
            topic_hash,
            LocalSubscription {
                options: SubscriptionOpts {
                    requests_partial,
                    supports_partial,
                },
                partial_messages: Default::default(),
            },
        );
    }

    /// Called by the [`Behaviour`](crate::Behaviour) when we unsubscribed from the topic.
    pub(crate) fn unsubscribe(&mut self, topic_hash: &TopicHash) {
        self.subscriptions.remove(&topic_hash.clone());
    }

    /// Called by the [`Behaviour`](crate::Behaviour) when a peer has disconnected.
    pub(crate) fn peer_disconnected(&mut self, peer_id: PeerId) {
        for topic_peers in self.peer_subscriptions.values_mut() {
            topic_peers.remove(&peer_id);
        }
    }

    /// Called by the [`Behaviour`](crate::Behaviour) when a remote peer unsubscribed from the
    /// topic.
    pub(crate) fn peer_subscribed(
        &mut self,
        peer_id: &PeerId,
        topic_hash: TopicHash,
        options: SubscriptionOpts,
    ) {
        let topic = self
            .peer_subscriptions
            .entry(topic_hash.clone())
            .or_default();
        topic.insert(
            *peer_id,
            RemoteSubscription {
                options,
                partial_messages: Default::default(),
            },
        );
    }

    pub(crate) fn peer_unsubscribed(&mut self, peer_id: PeerId, topic_hash: &TopicHash) {
        let Some(topic) = self.peer_subscriptions.get_mut(topic_hash) else {
            tracing::error!(topic = %topic_hash, "Peer not subscribed on topic");
            return;
        };
        topic.remove(&peer_id);
    }

    /// Called by the [`Behaviour`](crate::Behaviour) during heartbeat.
    /// Returns the list of the peers and respective partial metadata to send.
    pub(crate) fn heartbeat(
        &mut self,
        mesh: &HashMap<TopicHash, BTreeSet<PeerId>>,
        fanout: &HashMap<TopicHash, BTreeSet<PeerId>>,
        gossip_lazy: usize,
        gossip_factor: f64,
        max_metadata_length: usize,
    ) -> Vec<PublishAction> {
        for peer_state in self.peer_subscriptions.values_mut() {
            for topics in peer_state.values_mut() {
                topics.partial_messages.retain(|_, partial| {
                    partial.ttl -= 1;
                    partial.ttl != 0
                });
            }
        }

        for subscription in self.subscriptions.values_mut() {
            subscription.partial_messages.retain(|_, partial| {
                partial.ttl -= 1;
                partial.ttl != 0
            });
        }

        // Emit gossip.
        let mut actions = vec![];
        let all_topics: HashSet<_> = mesh.keys().chain(fanout.keys()).collect();
        for topic_hash in all_topics {
            let Some(subscription) = self.subscriptions.get(topic_hash) else {
                continue;
            };

            let Some(subscription_peers) = self.peer_subscriptions.get_mut(topic_hash) else {
                tracing::trace!("Skipping sending partials on topic, no peer subscriptions for it");
                continue;
            };

            let mesh_peers = mesh.get(topic_hash);
            let fanout_peers = fanout.get(topic_hash);
            let eligible_peers = subscription_peers
                .iter_mut()
                .filter(|(peer_id, peer_subscription)| {
                    !mesh_peers.is_some_and(|p| p.contains(peer_id))
                        && !fanout_peers.is_some_and(|p| p.contains(peer_id))
                        && peer_subscription.options.supports_partial
                })
                .collect::<Vec<_>>();
            let gossip_amp = max(
                gossip_lazy,
                (gossip_factor * eligible_peers.len() as f64) as usize,
            );
            let to_msg_peers = eligible_peers
                .into_iter()
                .choose_multiple(&mut rand::thread_rng(), gossip_amp);

            for (peer_id, remote_subscription) in to_msg_peers {
                let mut num_messages = 0;
                for (group_id, partial_message) in subscription.partial_messages.iter() {
                    if num_messages == max_metadata_length {
                        break;
                    }

                    let remote_partial = remote_subscription
                        .partial_messages
                        .entry(group_id.clone())
                        .or_default();

                    let Ok(action) = partial_message.content.partial_action_from_metadata(
                        *peer_id,
                        remote_partial.peer_metadata.as_ref().map(|p| p.as_ref()),
                    ) else {
                        tracing::error!(peer = %peer_id, group_id = ?group_id,
                    "Could not reconstruct message bytes for peer metadata");
                        remote_subscription.partial_messages.remove(group_id);
                        actions.push(PublishAction::PenalizePeer {
                            peer_id: *peer_id,
                            topic_hash: topic_hash.clone(),
                        });
                        continue;
                    };

                    // Check if we have new data for the peer.
                    let body = if let Some((body, peer_updated_metadata)) = action.send {
                        // We have something to send, update the peer's metadata.
                        remote_partial.peer_metadata =
                            Some(PeerMetadata::Local(peer_updated_metadata));
                        Some(body)
                    } else if remote_partial.peer_metadata.is_none() || action.need {
                        // We have no data to eagerly send, but we want to transmit our metadata
                        // anyway, to let the peer know of our metadata so
                        // that it sends us its data.
                        None
                    } else {
                        continue;
                    };

                    tracing::debug!(%peer_id, ?group_id, "Gossiping metadata to peer");
                    actions.push(PublishAction::SendMessage {
                        peer_id: *peer_id,
                        rpc: RpcOut::PartialMessage(PartialMessage {
                            group_id: group_id.clone(),
                            topic_hash: topic_hash.clone(),
                            body,
                            metadata: Some(partial_message.content.metadata().as_slice().to_vec()),
                        }),
                    });
                    num_messages += 1;
                }
            }
        }
        actions
    }

    /// Called by the [`Behaviour`](crate::Behaviour) when a partial message is received.
    pub(crate) fn handle_received(
        &mut self,
        peer_id: PeerId,
        message: PartialMessage,
    ) -> Vec<ReceivedAction> {
        // If we don't have any peer yet subscribed to this topic, insert it.
        // We might have received a message from a peer not subscribed to a topic.
        let topic = self
            .peer_subscriptions
            .entry(message.topic_hash.clone())
            .or_default();

        // If the peer has sent us a partial message without a subscription message first,
        // insert it on the list with the defaults, supports_partial = false.
        let peer_subscription = topic.entry(peer_id).or_default();

        let peer_partial = peer_subscription
            .partial_messages
            .entry(message.group_id.clone())
            .or_default();

        // Check if the local partial data we have from the peer is oudated.
        let metadata_updated = match (&mut peer_partial.peer_metadata, &message.metadata) {
            (None, Some(remote_metadata)) => {
                peer_partial.peer_metadata = Some(PeerMetadata::Remote(remote_metadata.clone()));
                true
            }
            (Some(PeerMetadata::Remote(ref metadata)), Some(remote_metadata)) => {
                if metadata != remote_metadata {
                    peer_partial.peer_metadata =
                        Some(PeerMetadata::Remote(remote_metadata.clone()));
                    true
                } else {
                    false
                }
            }
            (Some(PeerMetadata::Local(metadata)), Some(remote_metadata)) => {
                match metadata.update(remote_metadata) {
                    Ok(updated) => updated,
                    Err(err) => {
                        tracing::debug!(
                            peer=%peer_id,
                            topic=%message.topic_hash,
                            group_id=?message.group_id,
                            err=%err,
                            "Error updating Partial metadata"
                        );
                        return vec![ReceivedAction::Publish(PublishAction::PenalizePeer {
                            peer_id,
                            topic_hash: message.topic_hash.clone(),
                        })];
                    }
                }
            }
            (Some(_), None) | (None, None) => false,
        };

        // Check whether there is anything to send or receive.
        if !metadata_updated && message.body.is_none() {
            return vec![];
        }

        // Check if we already have this partial,
        // if not, just return it to the application layer.
        let Some(local_partial) = self
            .subscriptions
            .get_mut(&message.topic_hash)
            .and_then(|t| t.partial_messages.get(&message.group_id))
        else {
            return vec![ReceivedAction::EmitEvent {
                topic_hash: message.topic_hash,
                peer_id,
                group_id: message.group_id,
                message: message.body,
                metadata: message.metadata,
            }];
        };

        // Update the `Metadata` as seen by the remote peer, reflecting the current local state.
        if let (Some(metadata), Some(body)) = (&mut peer_partial.metadata, &message.body) {
            if let Err(err) = metadata.update_from_data(body) {
                tracing::debug!(peer = %peer_id, group_id = ?message.group_id,err = %err,
                    "Could update the metadata as seen by the remote peer");
                return vec![ReceivedAction::Publish(PublishAction::PenalizePeer {
                    peer_id,
                    topic_hash: message.topic_hash.clone(),
                })];
            }
        }

        let received_action = match local_partial
            .content
            .partial_action_from_metadata(peer_id, message.metadata.as_deref())
        {
            Ok(action) => action,
            Err(err) => {
                tracing::debug!(peer = %peer_id, group_id = ?message.group_id,err = %err,
                    "Could not reconstruct message bytes for peer metadata from a received partial");
                // Should we remove the partial from the peer?
                peer_subscription.partial_messages.remove(&message.group_id);
                return vec![ReceivedAction::Publish(PublishAction::PenalizePeer {
                    peer_id,
                    topic_hash: message.topic_hash.clone(),
                })];
            }
        };

        let mut actions = vec![];
        if received_action.need {
            actions.push(ReceivedAction::EmitEvent {
                topic_hash: message.topic_hash.clone(),
                peer_id,
                group_id: message.group_id.clone(),
                message: message.body,
                metadata: message.metadata,
            });
        }

        let Some((body, peer_updated_metadata)) = received_action.send else {
            return actions;
        };

        peer_partial.peer_metadata = Some(PeerMetadata::Local(peer_updated_metadata));

        let cached_metadata = local_partial.content.metadata().as_slice().to_vec();
        actions.push(ReceivedAction::Publish(PublishAction::SendMessage {
            peer_id,
            rpc: RpcOut::PartialMessage(PartialMessage {
                body: Some(body),
                metadata: Some(cached_metadata),
                group_id: message.group_id.clone(),
                topic_hash: message.topic_hash.clone(),
            }),
        }));

        actions
    }

    /// Check if the peer requests partial messages for the topic.
    pub(crate) fn requests_partial(&self, peer_id: &PeerId, topic_hash: &TopicHash) -> bool {
        self.peer_subscriptions
            .get(topic_hash)
            .and_then(|topic| topic.get(peer_id))
            .map(|s| s.options.requests_partial)
            .unwrap_or(false)
    }

    /// Check if the peer supports partial messages for the topic.
    pub(crate) fn supports_partial(&self, peer_id: &PeerId, topic_hash: &TopicHash) -> bool {
        self.peer_subscriptions
            .get(topic_hash)
            .and_then(|topic| topic.get(peer_id))
            .map(|s| s.options.supports_partial)
            .unwrap_or(false)
    }

    /// Get our partial opts for a topic (used by Behaviour when sending Subscribe)
    pub(crate) fn opts(&self, topic: &TopicHash) -> Option<SubscriptionOpts> {
        self.subscriptions
            .get(topic)
            .map(|subscription| subscription.options)
    }

    /// Check if the peer has sent us message on the provided topic and group_id.
    pub(crate) fn group_id(
        &self,
        peer_id: &PeerId,
        topic_hash: &TopicHash,
        group_id: &[u8],
    ) -> bool {
        self.peer_subscriptions
            .get(topic_hash)
            .and_then(|topic| topic.get(peer_id))
            .and_then(|subscription| subscription.partial_messages.get(group_id))
            .is_some()
    }

    /// Determines the actions to take based on the provided recipients and partial.
    /// Returns the actions for the Behaviour to execute.
    pub(crate) fn handle_publish<P: Partial + 'static>(
        &mut self,
        topic_hash: TopicHash,
        partial_message: P,
        recipients: Vec<PeerId>,
    ) -> Result<Vec<PublishAction>, PublishError> {
        if recipients.is_empty() {
            tracing::debug!(topic = %topic_hash, "Recipient list for publishing partial message is empty");
            return Err(PublishError::NoPeersSubscribedToTopic);
        }

        let mut actions = vec![];
        let group_id = partial_message.group_id();
        let publish_metadata = partial_message.metadata().as_slice().to_vec();
        let Some(topic_peers) = self.peer_subscriptions.get_mut(&topic_hash) else {
            tracing::error!(topic = %topic_hash, "No peers subscribed to topic");
            return Err(PublishError::NoPeersSubscribedToTopic);
        };

        for peer_id in recipients {
            let Some(subscription) = topic_peers.get_mut(&peer_id) else {
                tracing::error!(peer = %peer_id,
                    "Could not get partial subscripion from peer which subscribed for partial messages");
                continue;
            };

            let remote_partial = subscription
                .partial_messages
                .entry(group_id.clone())
                .or_default();

            // Peer `supports_partial` but doesn't `requests_partial`.
            // We assume peer requests_partial is true as that has already been filtered
            // by the behavior.
            if !subscription.options.requests_partial {
                actions.push(PublishAction::SendMessage {
                    peer_id,
                    rpc: RpcOut::PartialMessage(PartialMessage {
                        body: None,
                        metadata: Some(publish_metadata.clone()),
                        group_id: group_id.clone(),
                        topic_hash: topic_hash.clone(),
                    }),
                });
                continue;
            }

            let Ok(action) = partial_message.partial_action_from_metadata(
                peer_id,
                remote_partial.peer_metadata.as_ref().map(|p| p.as_ref()),
            ) else {
                tracing::error!(peer = %peer_id, group_id = ?group_id,
                    "Could not reconstruct message bytes for peer metadata");
                subscription.partial_messages.remove(&group_id);
                actions.push(PublishAction::PenalizePeer {
                    peer_id,
                    topic_hash: topic_hash.clone(),
                });
                continue;
            };

            // Check if we have new parts for the peer,
            // and update the peer metadata if so.
            let body = if let Some((body, peer_updated_metadata)) = action.send {
                remote_partial.peer_metadata = Some(PeerMetadata::Local(peer_updated_metadata));
                Some(body)
            } else {
                None
            };

            // Determine whether the local `Metadata` needs to be sent, based on changes
            // in the remote peer's view of our metadata.
            match &mut remote_partial.metadata {
                Some(metadata) => {
                    let updated = match metadata.update(&publish_metadata) {
                        Ok(updated) => updated,
                        Err(error) => {
                            actions.push(PublishAction::PenalizePeer {
                                peer_id,
                                topic_hash: topic_hash.clone(),
                            });
                            tracing::debug!(%error, "Error updating peers view of the metdata");
                            continue;
                        }
                    };
                    if !updated {
                        continue;
                    }
                }
                None => remote_partial.metadata = Some(partial_message.metadata()),
            }

            actions.push(PublishAction::SendMessage {
                peer_id,
                rpc: RpcOut::PartialMessage(PartialMessage {
                    group_id: group_id.clone(),
                    topic_hash: topic_hash.clone(),
                    body,
                    metadata: Some(publish_metadata.clone()),
                }),
            });
        }

        // Cache the sent partial
        let topic_partials = self.subscriptions.entry(topic_hash).or_default();
        topic_partials.partial_messages.insert(
            partial_message.group_id(),
            LocalPartial {
                content: Box::new(partial_message),
                ttl: DEFAULT_PARTIAL_TTL,
            },
        );

        Ok(actions)
    }
}

/// Action returned by `State::partial_action`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum PublishAction {
    /// Send a partial message RPC to a peer
    SendMessage { peer_id: PeerId, rpc: RpcOut },
    /// Penalize peer for invalid partial
    PenalizePeer {
        peer_id: PeerId,
        topic_hash: TopicHash,
    },
}

/// Action returned by `State::handle_received`.
#[derive(Debug)]
pub(crate) enum ReceivedAction {
    /// Emit a Partial event to the application
    EmitEvent {
        topic_hash: TopicHash,
        peer_id: PeerId,
        group_id: Vec<u8>,
        message: Option<Vec<u8>>,
        metadata: Option<Vec<u8>>,
    },
    Publish(PublishAction),
}

/// A Partial message sent and received from remote peers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartialMessage {
    /// The group ID that identifies the complete logical message.
    pub group_id: Vec<u8>,
    /// The topic ID this partial message belongs to.
    pub topic_hash: TopicHash,
    /// The partial message itself.
    pub body: Option<Vec<u8>>,
    /// The partial metadata we have and want.
    pub metadata: Option<Vec<u8>>,
}

/// Stored `Metadata` for a peer,
/// `Remote` or `Local` depends on who last updated it.
#[derive(Debug)]
pub(crate) enum PeerMetadata {
    /// The metadata was updated with data from a remote peer.
    Remote(Vec<u8>),
    /// The metadata was updated by us when publishing a partial message.
    Local(Box<dyn crate::extensions::partial_messages::Metadata>),
}

impl AsRef<[u8]> for PeerMetadata {
    fn as_ref(&self) -> &[u8] {
        match self {
            PeerMetadata::Remote(metadata) => metadata,
            PeerMetadata::Local(metadata) => metadata.as_slice(),
        }
    }
}

/// Partial options when subscribing a topic.
#[derive(Default)]
pub(crate) struct LocalSubscription {
    /// Subscription options, None if we have not subscribe to the topic.
    options: SubscriptionOpts,
    /// Partial messages we have sent us on the topic subscription.
    partial_messages: HashMap<Vec<u8>, LocalPartial>,
}

/// A topic subscribed by a remote peer.
#[derive(Debug, Default)]
pub(crate) struct RemoteSubscription {
    /// Subscription options, None if peer has not subscribe to the topic.
    options: SubscriptionOpts,
    /// Partial messages that the peer has sent us on the topic subscription.
    partial_messages: HashMap<Vec<u8>, RemotePartial>,
}

/// a local cached sent partial message.
struct LocalPartial {
    content: Box<dyn Partial>,
    ttl: usize,
}

/// A partial message data the peer has sent us.
#[derive(Debug)]
struct RemotePartial {
    /// Our view of the peers' partial metadata.
    peer_metadata: Option<PeerMetadata>,
    /// The peers view of our partial metadata.
    metadata: Option<Box<dyn Metadata>>,
    /// The remaining heartbeats for this message to be deleted.
    ttl: usize,
}

impl Default for RemotePartial {
    fn default() -> Self {
        Self {
            peer_metadata: Default::default(),
            metadata: Default::default(),
            ttl: DEFAULT_PARTIAL_TTL,
        }
    }
}

/// Errors that can occur during partial message processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialError {
    /// The received data is too short to contain required headers/metadata.
    InsufficientData {
        /// Expected minimum number of bytes.
        expected: usize,
        /// Actual number of bytes received.
        received: usize,
    },

    /// The data format is invalid or corrupted.
    InvalidFormat,

    /// The partial data doesn't belong to this message group.
    WrongGroup {
        /// Group Id of the received message.
        received: Vec<u8>,
    },

    /// The partial data is a duplicate of already received data.
    DuplicateData(Vec<u8>),

    /// The partial data is out of the expected range or sequence.
    OutOfRange,

    /// The message is already complete and cannot accept more data.
    AlreadyComplete,

    /// Application-specific validation failed.
    ValidationFailed,
}

impl std::error::Error for PartialError {}

impl std::fmt::Display for PartialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientData { expected, received } => {
                write!(
                    f,
                    "Insufficient data: expected at least {} bytes, got {}",
                    expected, received
                )
            }
            Self::InvalidFormat => {
                write!(f, "Invalid data format")
            }
            Self::WrongGroup { received } => {
                write!(f, "Wrong group ID: got {:?}", received)
            }
            Self::DuplicateData(part_id) => {
                write!(f, "Duplicate data for part {:?}", part_id)
            }
            Self::OutOfRange => {
                write!(f, "Data out of range")
            }
            Self::AlreadyComplete => {
                write!(f, "Message is already complete")
            }
            Self::ValidationFailed => {
                write!(f, "Validation failed")
            }
        }
    }
}
