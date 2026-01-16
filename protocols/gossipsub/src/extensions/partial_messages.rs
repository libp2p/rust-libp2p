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
    collections::{HashMap, HashSet},
    fmt::Debug,
};

use libp2p_core::PeerId;

use crate::{types::RpcOut, TopicHash};

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
    fn metadata(&self) -> Vec<u8>;

    /// Generates an action from the given metadata.
    ///
    /// When a peer requests specific parts (via PartialIWANT), this method
    /// generates the actual message data to send back. The `metadata` parameter
    /// describes what parts are being requested.
    ///
    /// Returns a [`PublishAction`] for the given metadata, or an error.
    fn partial_action_from_metadata(
        &self,
        peer_id: PeerId,
        metadata: Option<&[u8]>,
    ) -> Result<PartialAction, PartialError>;
}

pub trait Metadata: Debug + Send + Sync {
    /// Return the `Metadata` as a byte slice.
    fn as_slice(&self) -> &[u8];
    /// try to Update the `Metadata` with the remote data,
    /// return true if it was updated.
    fn update(&mut self, data: &[u8]) -> Result<bool, PartialError>;
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
    /// Our subscription options per topic
    pub(crate) opts: HashMap<TopicHash, SubscriptionOpts>,
    /// Cached partial messages we're publishing
    pub(crate) cached: HashMap<TopicHash, HashMap<Vec<u8>, Box<dyn Partial>>>,
    /// Per-peer partial state
    pub(crate) peers: HashMap<PeerId, PeerPartialState>,
}

/// Partial state of the remote peers.
#[derive(Default)]
pub(crate) struct PeerPartialState {
    pub(crate) partial_opts: HashMap<TopicHash, SubscriptionOpts>,
    pub(crate) partial_messages: HashMap<TopicHash, HashMap<Vec<u8>, PartialData>>,
}

impl State {
    /// Called by the [`Behaviour`] when we subscribed to the topic.
    pub(crate) fn subscribe(&mut self, topic_hash: TopicHash, requests_partial: bool) {
        self.opts.insert(
            topic_hash,
            SubscriptionOpts {
                requests_partial,
                supports_partial: true,
            },
        );
    }

    /// Called by the [`Behaviour`] when we unsubscribed from the topic.
    pub(crate) fn unsubscribe(&mut self, topic_hash: &TopicHash) {
        self.opts.remove(&topic_hash.clone());
    }

    /// Called by the [`Behaviour`] when a peer has connected.
    pub(crate) fn peer_connected(&mut self, peer_id: PeerId) {
        self.peers.insert(peer_id, Default::default());
    }

    /// Called by the [`Behaviour`] when a peer has disconnected.
    pub(crate) fn peer_disconnected(&mut self, peer_id: PeerId) {
        self.peers.remove(&peer_id);
    }

    /// Called by the [`Behaviour`] when a remote peer unsubscribed from the topic.
    pub(crate) fn peer_subscribed(
        &mut self,
        peer_id: &PeerId,
        topic_hash: TopicHash,
        requests_partial: bool,
        supports_partial: bool,
    ) {
        let Some(peer) = self.peers.get_mut(peer_id) else {
            tracing::error!(
                %peer_id,
                "Partial subscription by unknown peer"
            );
            return;
        };
        peer.partial_opts.insert(
            topic_hash,
            SubscriptionOpts {
                requests_partial,
                supports_partial,
            },
        );
    }

    pub(crate) fn peer_unsubscribed(&mut self, peer_id: PeerId, topic_hash: &TopicHash) {
        let Some(peer) = self.peers.get_mut(&peer_id) else {
            tracing::error!(
                %peer_id,
                "Partial unsubscription by unknown peer"
            );
            return;
        };
        peer.partial_opts.remove(topic_hash);
    }

    /// Called by the [`Behaviour`] during heartbeat.
    pub(crate) fn heartbeat(&mut self) {
        for peer_state in self.peers.values_mut() {
            for topics in peer_state.partial_messages.values_mut() {
                topics.retain(|_, partial| {
                    partial.ttl -= 1;
                    partial.ttl == 0
                });
            }
        }
    }

    /// Called by the [`Behaviour`] when a partial message is rceived.
    pub(crate) fn handle_received(
        &mut self,
        peer_id: PeerId,
        message: PartialMessage,
    ) -> Vec<ReceivedAction> {
        let mut results = vec![];
        let peer_partials = self.peers.entry(peer_id).or_default();
        let topic_partials = peer_partials
            .partial_messages
            .entry(message.topic_id.clone())
            .or_default();

        let group_partials = topic_partials.entry(message.group_id.clone()).or_default();

        // Check if the local partial data we have from the peer is oudated.
        let metadata_updated = match (&mut group_partials.metadata, &message.metadata) {
            (None, Some(remote_metadata)) => {
                group_partials.metadata = Some(PeerMetadata::Remote(remote_metadata.clone()));
                true
            }
            (Some(PeerMetadata::Remote(ref metadata)), Some(remote_metadata)) => {
                if metadata != remote_metadata {
                    group_partials.metadata = Some(PeerMetadata::Remote(remote_metadata.clone()));
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
                            topic=%message.topic_id,
                            group_id=?message.group_id,
                            err=%err,
                            "Error updating Partial metadata"
                        );
                        results.push(ReceivedAction::Publish(PublishAction::PenalizePeer {
                            peer_id,
                            topic_hash: message.topic_id.clone(),
                        }));
                        false
                    }
                }
            }
            (Some(_), None) | (None, None) => false,
        };

        if !metadata_updated && message.body.is_none() {
            return results;
        }

        // We may have already received other partials from this and other peers,
        // but haven't responded to them yet, in those situations just return
        // the partial to the application layer.
        let Some(local_partial) = self
            .cached
            .get_mut(&message.topic_id)
            .and_then(|t| t.get(&message.group_id))
        else {
            results.push(ReceivedAction::EmitEvent {
                topic_hash: message.topic_id,
                peer_id,
                group_id: message.group_id,
                message: message.body,
                metadata: message.metadata,
            });
            return results;
        };

        let action = match local_partial
            .partial_action_from_metadata(peer_id, message.metadata.as_deref())
        {
            Ok(action) => action,
            Err(err) => {
                tracing::debug!(peer = %peer_id, group_id = ?message.group_id,err = %err,
                    "Could not reconstruct message bytes for peer metadata from a received partial");
                // Should we remove the partial from the peer?
                topic_partials.remove(&message.group_id);
                results.push(ReceivedAction::Publish(PublishAction::PenalizePeer {
                    peer_id,
                    topic_hash: message.topic_id.clone(),
                }));
                return results;
            }
        };

        // We have new data for that peer.
        if let Some((body, peer_updated_metadata)) = action.send {
            group_partials.metadata = Some(PeerMetadata::Local(peer_updated_metadata));

            let cached_metadata = local_partial.metadata().as_slice().to_vec();
            results.push(ReceivedAction::Publish(PublishAction::SendMessage {
                peer_id,
                rpc: RpcOut::PartialMessage {
                    message: Some(body),
                    metadata: cached_metadata,
                    group_id: message.group_id.clone(),
                    topic_id: message.topic_id.clone(),
                },
            }));
        }

        results
    }

    /// Check if the peer requests partial messages for the topic.
    pub(crate) fn requests_partial(&self, peer_id: &PeerId, topic_hash: &TopicHash) -> bool {
        self.peers
            .get(peer_id)
            .and_then(|p| p.partial_opts.get(topic_hash))
            .map(|opts| opts.requests_partial)
            .unwrap_or(false)
    }

    /// Check if the peer supports partial messages for the topic.
    pub(crate) fn supports_partial(&self, peer_id: &PeerId, topic_hash: &TopicHash) -> bool {
        self.peers
            .get(peer_id)
            .and_then(|p| p.partial_opts.get(topic_hash))
            .map(|opts| opts.supports_partial)
            .unwrap_or(false)
    }

    /// Determines the actions to take based on the provided recipients and partial.
    /// Returns the actions for the Behaviour to execute.
    pub(crate) fn handle_publish<P: Partial + 'static>(
        &mut self,
        topic_hash: TopicHash,
        partial_message: P,
        recipients: HashSet<PeerId>,
    ) -> Vec<PublishAction> {
        let mut actions = vec![];
        let group_id = partial_message.group_id();

        let publish_metadata = partial_message.metadata();
        for peer_id in recipients {
            let Some(partial_opts) = self
                .peers
                .get(&peer_id)
                .and_then(|peer| peer.partial_opts.get(&topic_hash))
                .copied()
            else {
                tracing::error!(peer = %peer_id,
                    "Could not get partial subscripion options from peer which subscribed for partial messages");
                continue;
            };

            let peer_partials = self.peers.entry(peer_id).or_default();
            let topic_partials = peer_partials
                .partial_messages
                .entry(topic_hash.clone())
                .or_default();
            let group_partials = topic_partials.entry(group_id.clone()).or_default();

            // Peer `supports_partial` but doesn't `requests_partial`.
            if !partial_opts.requests_partial {
                actions.push(PublishAction::SendMessage {
                    peer_id,
                    rpc: RpcOut::PartialMessage {
                        message: None,
                        metadata: publish_metadata.clone(),
                        group_id: group_id.clone(),
                        topic_id: topic_hash.clone(),
                    },
                });
                continue;
            }

            let Ok(action) = partial_message.partial_action_from_metadata(
                peer_id,
                group_partials.metadata.as_ref().map(|p| p.as_ref()),
            ) else {
                tracing::error!(peer = %peer_id, group_id = ?group_id,
                    "Could not reconstruct message bytes for peer metadata");
                topic_partials.remove(&group_id);
                actions.push(PublishAction::PenalizePeer {
                    peer_id,
                    topic_hash: topic_hash.clone(),
                });
                continue;
            };

            // Check if we have new data for the peer.
            let message = if let Some((message, peer_updated_metadata)) = action.send {
                // We have something to send, update the peer's metadata.
                group_partials.metadata = Some(PeerMetadata::Local(peer_updated_metadata));
                Some(message)
            } else if group_partials.metadata.is_none() || action.need {
                // We have no data to eagerly send, but we want to transmit our metadata anyway, to
                // let the peer know of our metadata so that it sends us its data.
                None
            } else {
                continue;
            };

            actions.push(PublishAction::SendMessage {
                peer_id,
                rpc: RpcOut::PartialMessage {
                    group_id: group_id.clone(),
                    topic_id: topic_hash.clone(),
                    message,
                    metadata: publish_metadata.clone(),
                },
            });
        }
        actions
    }
    /// Get our partial opts for a topic (used by Behaviour when sending Subscribe)
    pub(crate) fn opts(&self, topic: &TopicHash) -> Option<&SubscriptionOpts> {
        self.opts.get(topic)
    }
}

/// Action returned by `State::partial_action`.
#[allow(clippy::large_enum_variant)]
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

/// A received partial message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartialMessage {
    /// The topic ID this partial message belongs to.
    pub topic_id: TopicHash,
    /// The group ID that identifies the complete logical message.
    pub group_id: Vec<u8>,
    /// The partial metadata we have and we want.
    pub metadata: Option<Vec<u8>>,
    /// The partial message itself.
    pub body: Option<Vec<u8>>,
}

/// Partial options when subscribing a topic.
#[derive(Debug, Clone, Copy, Default, Eq, Hash, PartialEq)]
pub(crate) struct SubscriptionOpts {
    pub(crate) requests_partial: bool,
    pub(crate) supports_partial: bool,
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

/// The partial message data the peer has.
#[derive(Debug)]
pub(crate) struct PartialData {
    /// The current peer partial metadata.
    pub(crate) metadata: Option<PeerMetadata>,
    /// The remaining heartbeats for this message to be deleted.
    pub(crate) ttl: usize,
}

impl Default for PartialData {
    fn default() -> Self {
        Self {
            metadata: Default::default(),
            ttl: 5,
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
