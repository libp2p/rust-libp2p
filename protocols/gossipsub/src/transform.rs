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

//! This trait allows of extended user-level decoding that can apply to message-data before a
//! message-id is calculated.
//!
//! This is primarily designed to allow applications to implement their own custom compression
//! algorithms that can be topic-specific. Once the raw data is transformed the message-id is then
//! calculated, allowing for applications to employ message-id functions post compression.

use crate::{GossipsubMessage, RawGossipsubMessage, TopicHash};

/// A general trait of transforming a [`RawGossipsubMessage`] into a [`GossipsubMessage`]. The
/// [`RawGossipsubMessage`] is obtained from the wire and the [`GossipsubMessage`] is used to
/// calculate the [`crate::MessageId`] of the message and is what is sent to the application.
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
    /// transformed data will then be used to create a [`crate::RawGossipsubMessage`] to be sent to peers.
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
