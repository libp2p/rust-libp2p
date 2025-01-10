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

//! Error types that can result from gossipsub.

use libp2p_identity::SigningError;

/// Error associated with publishing a gossipsub message.
#[derive(Debug)]
pub enum PublishError {
    /// This message has already been published.
    Duplicate,
    /// An error occurred whilst signing the message.
    SigningError(SigningError),
    /// There were no peers to send this message to.
    InsufficientPeers,
    /// The overall message was too large. This could be due to excessive topics or an excessive
    /// message size.
    MessageTooLarge,
    /// The compression algorithm failed.
    TransformFailed(std::io::Error),
    /// Messages could not be sent because the queues for all peers were full. The usize represents
    /// the number of peers that were attempted.
    AllQueuesFull(usize),
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SigningError(err) => Some(err),
            Self::TransformFailed(err) => Some(err),
            _ => None,
        }
    }
}

/// Error associated with subscribing to a topic.
#[derive(Debug)]
pub enum SubscriptionError {
    /// Couldn't publish our subscription
    PublishError(PublishError),
    /// We are not allowed to subscribe to this topic by the subscription filter
    NotAllowed,
}

impl std::fmt::Display for SubscriptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SubscriptionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PublishError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<SigningError> for PublishError {
    fn from(error: SigningError) -> Self {
        PublishError::SigningError(error)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ValidationError {
    /// The message has an invalid signature,
    InvalidSignature,
    /// The sequence number was empty, expected a value.
    EmptySequenceNumber,
    /// The sequence number was the incorrect size
    InvalidSequenceNumber,
    /// The PeerId was invalid
    InvalidPeerId,
    /// Signature existed when validation has been sent to
    /// [`crate::behaviour::MessageAuthenticity::Anonymous`].
    SignaturePresent,
    /// Sequence number existed when validation has been sent to
    /// [`crate::behaviour::MessageAuthenticity::Anonymous`].
    SequenceNumberPresent,
    /// Message source existed when validation has been sent to
    /// [`crate::behaviour::MessageAuthenticity::Anonymous`].
    MessageSourcePresent,
    /// The data transformation failed.
    TransformFailed,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ValidationError {}

impl From<std::io::Error> for PublishError {
    fn from(error: std::io::Error) -> PublishError {
        PublishError::TransformFailed(error)
    }
}

/// Error associated with Config building.
#[derive(Debug)]
pub enum ConfigBuilderError {
    /// Maximum transmission size is too small.
    MaxTransmissionSizeTooSmall,
    /// History length less than history gossip length.
    HistoryLengthTooSmall,
    /// The ineauality doesn't hold mesh_outbound_min <= mesh_n_low <= mesh_n <= mesh_n_high
    MeshParametersInvalid,
    /// The inequality doesn't hold mesh_outbound_min <= self.config.mesh_n / 2
    MeshOutboundInvalid,
    /// unsubscribe_backoff is zero
    UnsubscribeBackoffIsZero,
    /// Invalid protocol
    InvalidProtocol,
}

impl std::error::Error for ConfigBuilderError {}

impl std::fmt::Display for ConfigBuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::MaxTransmissionSizeTooSmall => {
                write!(f, "Maximum transmission size is too small")
            }
            Self::HistoryLengthTooSmall => write!(f, "History length less than history gossip length"),
            Self::MeshParametersInvalid => write!(f, "The ineauality doesn't hold mesh_outbound_min <= mesh_n_low <= mesh_n <= mesh_n_high"),
            Self::MeshOutboundInvalid => write!(f, "The inequality doesn't hold mesh_outbound_min <= self.config.mesh_n / 2"),
            Self::UnsubscribeBackoffIsZero => write!(f, "unsubscribe_backoff is zero"),
            Self::InvalidProtocol => write!(f, "Invalid protocol"),
        }
    }
}
