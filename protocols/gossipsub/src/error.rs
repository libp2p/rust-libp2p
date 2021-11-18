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

use libp2p_core::identity::error::SigningError;
use libp2p_core::upgrade::ProtocolError;
use std::fmt;

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
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
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
        write!(f, "{:?}", self)
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

/// Errors that can occur in the protocols handler.
#[derive(Debug)]
pub enum GossipsubHandlerError {
    /// The maximum number of inbound substreams created has been exceeded.
    MaxInboundSubstreams,
    /// The maximum number of outbound substreams created has been exceeded.
    MaxOutboundSubstreams,
    /// The message exceeds the maximum transmission size.
    MaxTransmissionSize,
    /// Protocol negotiation timeout.
    NegotiationTimeout,
    /// Protocol negotiation failed.
    NegotiationProtocolError(ProtocolError),
    /// IO error.
    Io(std::io::Error),
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
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ValidationError {}

impl From<std::io::Error> for GossipsubHandlerError {
    fn from(error: std::io::Error) -> GossipsubHandlerError {
        GossipsubHandlerError::Io(error)
    }
}

impl From<std::io::Error> for PublishError {
    fn from(error: std::io::Error) -> PublishError {
        PublishError::TransformFailed(error)
    }
}

impl fmt::Display for GossipsubHandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for GossipsubHandlerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GossipsubHandlerError::Io(io) => Some(io),
            _ => None,
        }
    }
}
