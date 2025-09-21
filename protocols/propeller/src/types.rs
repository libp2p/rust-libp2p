//! Core types for the Propeller protocol.

use libp2p_identity::PeerId;

use crate::{
    message::{MessageId, Shred},
    ShredId,
};

/// Events emitted by the Propeller behaviour.
#[derive(Debug, Clone)]
pub enum Event {
    /// A shred has been received from a peer.
    ShredReceived {
        /// The peer that sent the shred.
        sender: PeerId,
        /// The received shred.
        shred: Shred,
    },
    /// A complete message has been reconstructed from shreds.
    MessageReceived {
        /// The publisher of the shred.
        publisher: PeerId,
        /// The message id the message belongs to.
        message_id: MessageId,
        /// The reconstructed message data.
        data: Vec<u8>,
    },
    /// Failed to reconstruct a message from shreds.
    MessageReconstructionFailed {
        /// The message id the message belongs to.
        message_id: MessageId,
        /// The publisher of the shred.
        publisher: PeerId,
        /// The error that occurred.
        error: ReconstructionError,
    },
    /// Failed to send a shred to a peer.
    ShredSendFailed {
        /// The peer we sent the shred from.
        sent_from: Option<PeerId>,
        /// The peer we sent the shred to.
        sent_to: Option<PeerId>,
        /// The error that occurred.
        error: ShredPublishError,
    },
    /// Failed to verify shred
    ShredValidationFailed {
        /// The peer we failed to verify the shred from. (The sender of the shred that should
        /// probably be reported)
        sender: PeerId,
        /// The stated publisher of the shred, might not have verified yet.
        shred_id: ShredId,
        /// The specific verification error that occurred.
        error: ShredValidationError,
    },
}

/// Node information for propeller tree topology.
#[derive(Clone, Debug)]
pub struct PropellerNode {
    /// The peer ID of this node.
    pub peer_id: PeerId,
    /// The weight of this node for tree positioning.
    pub weight: u64,
}

// ****************************************************************************

/// Errors that can occur when verifying a shred signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShredSignatureVerificationError {
    NoPublicKeyAvailable(PeerId),
    EmptySignature,
    VerificationFailed,
}

impl std::fmt::Display for ShredSignatureVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShredSignatureVerificationError::NoPublicKeyAvailable(publisher) => {
                write!(f, "No public key available for signer {}", publisher)
            }
            ShredSignatureVerificationError::EmptySignature => {
                write!(f, "Shred has empty signature")
            }
            ShredSignatureVerificationError::VerificationFailed => {
                write!(f, "Shred signature is invalid")
            }
        }
    }
}

impl std::error::Error for ShredSignatureVerificationError {}

// ****************************************************************************

/// Errors that can occur when sending a shred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShredPublishError {
    LocalPeerNotInPeerWeights,
    InvalidDataSize,
    SigningFailed(String),
    ErasureEncodingFailed(String),
    NotConnectedToPeer(PeerId),
    HandlerError(String),
    TreeGenerationError(TreeGenerationError),
}

impl std::fmt::Display for ShredPublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShredPublishError::LocalPeerNotInPeerWeights => {
                write!(f, "Local peer not in peer weights")
            }
            ShredPublishError::InvalidDataSize => {
                write!(f, "Invalid data size for broadcasting, data size must be divisible by number of data shreds")
            }
            ShredPublishError::SigningFailed(e) => {
                write!(f, "Signing failed: {}", e)
            }
            ShredPublishError::ErasureEncodingFailed(e) => {
                write!(f, "Erasure encoding failed: {}", e)
            }
            ShredPublishError::NotConnectedToPeer(peer_id) => {
                write!(f, "Not connected to peer {}", peer_id)
            }
            ShredPublishError::HandlerError(e) => {
                write!(f, "Handler error: {}", e)
            }
            ShredPublishError::TreeGenerationError(e) => {
                write!(f, "Tree generation error: {}", e)
            }
        }
    }
}

impl std::error::Error for ShredPublishError {}

// ****************************************************************************

/// Errors that can occur during message reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconstructionError {
    /// Erasure decoding failed.
    ErasureDecodingFailed(String),
}

impl std::fmt::Display for ReconstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconstructionError::ErasureDecodingFailed(msg) => {
                write!(f, "Erasure decoding failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for ReconstructionError {}

// ****************************************************************************

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeGenerationError {
    PublisherNotFound {
        /// The publisher that was not found in the peer list.
        publisher: PeerId,
    },
    LocalPeerNotInPeerWeights,
}

impl std::fmt::Display for TreeGenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeGenerationError::PublisherNotFound { publisher } => {
                write!(f, "Publisher not found: {}", publisher)
            }
            TreeGenerationError::LocalPeerNotInPeerWeights => {
                write!(f, "Local peer not in peer weights")
            }
        }
    }
}

impl std::error::Error for TreeGenerationError {}

// ****************************************************************************

/// Specific errors that can occur during shred verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShredValidationError {
    /// Publisher should not receive their own shreds (they broadcast them).
    ReceivedPublishedShred,
    /// Shred is already in cache (duplicate).
    DuplicateShred,
    /// Failed to get parent in tree topology.
    TreeError(TreeGenerationError),
    /// Shred failed parent verification in tree topology.
    ParentVerificationFailed {
        /// The expected sender according to tree topology.
        expected_sender: PeerId,
    },
    /// Shred signature verification failed.
    SignatureVerificationFailed(ShredSignatureVerificationError),
}

impl std::fmt::Display for ShredValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShredValidationError::ReceivedPublishedShred => {
                write!(f, "Publisher should not receive their own shred")
            }
            ShredValidationError::DuplicateShred => {
                write!(f, "Received shred that is already in cache")
            }
            ShredValidationError::TreeError(e) => {
                write!(f, "Received shred but error getting parent in tree: {}", e)
            }
            ShredValidationError::ParentVerificationFailed { expected_sender } => {
                write!(
                    f,
                    "Shred failed parent verification (expected sender = {})",
                    expected_sender
                )
            }
            ShredValidationError::SignatureVerificationFailed(e) => {
                write!(f, "Shred failed signature verification: {}", e)
            }
        }
    }
}

impl std::error::Error for ShredValidationError {}

// ****************************************************************************

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerSetError {
    LocalPeerNotInPeerWeights,
    InvalidPublicKey,
}

impl std::fmt::Display for PeerSetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerSetError::LocalPeerNotInPeerWeights => write!(f, "Local peer not in peer weights"),
            PeerSetError::InvalidPublicKey => write!(f, "Invalid public key"),
        }
    }
}

impl std::error::Error for PeerSetError {}
