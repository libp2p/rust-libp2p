//! This trait allows of extended user-level decoding that can apply to message-data before a
//! message-id is calculated.
//!
//! This is primarily designed to allow applications to implement their own custom compression
//! algorithms that can be topic-specific. Once the raw data is transformed the message-id is then
//! calculated, allowing for applications to employ message-id functions post compression.

use crate::{GossipsubMessage, RawGossipsubMessage, TopicHash};

/// A general trait of transforming a [`RawGossipubMessage`] into a [`GossipsubMessage`]. The
/// [`RawGossipsubMessage`] is obtained from the wire and the [`GossipsubMessage`] is used to
/// calculate the [`MessageId`] of the message and is what is sent to the application.
///
/// The inbound/outbound transforms must be inverses. Applying the inbound transform and then the
/// outbound transform MUST leave the underlying data un-modified.
///
/// By default, this is the identity transform for all fields in [`GossipsubMessage`].
pub trait DataTransform {
    /// Takes a [`RawGossipsubMessage`] received and converts it to a [`GossipsubMessage`].
    fn inbound_transform(
        &self,
        raw_message: RawGossipsubMessage,
    ) -> Result<GossipsubMessage, std::io::Error>;

    /// Takes the data to be published (a topic and associated data) transforms the data. The
    /// transformed data will then be used to create a [`RawGossipsubMessage`] to be sent to peers.
    fn outbound_transform(
        &self,
        topic: &TopicHash,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, std::io::Error>;
}

/// The default transform, the raw data is propagated as is to the application layer gossipsub.
#[derive(Default, Clone)]
pub struct IdentityTransform;

impl DataTransform for IdentityTransform {
    fn inbound_transform(
        &self,
        raw_message: RawGossipsubMessage,
    ) -> Result<GossipsubMessage, std::io::Error> {
        Ok(GossipsubMessage {
            source: raw_message.source,
            data: raw_message.data,
            sequence_number: raw_message.sequence_number,
            topic: raw_message.topic,
        })
    }

    fn outbound_transform(
        &self,
        _topic: &TopicHash,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, std::io::Error> {
        Ok(data)
    }
}
