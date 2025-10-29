//! Message types for the Propeller protocol.

use asynchronous_codec::{Decoder, Encoder};
use bytes::BytesMut;
use libp2p_core::{multihash::Multihash, PeerId};
use rand::Rng;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{generated::propeller::pb as proto, protocol::PropellerCodec};

/// Represents a message identifier for message grouping.
pub type MessageId = u64;

/// Represents a hash of a shred.
pub(crate) type ShredHash = [u8; 32];

/// Represents a shred index within a message.
pub type ShredIndex = u32;

/// Unique identifier for a shred.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ShredId {
    /// The message id this shred belongs to.
    pub message_id: MessageId,
    /// The index of this shred within the message.
    pub index: ShredIndex,
    /// The publisher that created this shred.
    pub publisher: PeerId,
}

/// A shred - the atomic unit of data transmission in Propeller.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Shred {
    /// Unique identifier for this shred.
    pub id: ShredId,
    /// The actual shard payload.
    pub shard: Vec<u8>,
    /// Signature for verification.
    pub signature: Vec<u8>,
}

impl Shred {
    pub fn hash(&self) -> ShredHash {
        let mut hasher = Sha256::new();
        let mut dst = BytesMut::new();
        self.encode(
            &mut dst,
            self.shard.len() + self.signature.len() + size_of::<Self>(),
        );
        hasher.update(dst);
        hasher.finalize().into()
    }

    /// Encode the shred without signature into a byte vector for signing.
    /// This follows a similar pattern to gossipsub's message encoding for signatures.
    pub(crate) fn encode_without_signature(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.id.message_id.to_be_bytes());
        buf.extend_from_slice(&self.id.index.to_be_bytes());
        let publisher_bytes = self.id.publisher.to_bytes();
        buf.push(publisher_bytes.len() as u8);
        buf.extend_from_slice(&publisher_bytes);
        buf.extend_from_slice(&(self.shard.len() as u32).to_be_bytes());
        buf.extend_from_slice(&self.shard);
        // Note: signature is intentionally omitted for signing
        buf
    }

    pub fn random<R: Rng>(rng: &mut R, max_shard_size: usize) -> Self {
        let topic = rng.random::<u64>();
        let index = rng.random::<u32>();
        let peer_id = rng.random::<[u8; 32]>();
        let publisher = PeerId::from_multihash(
            Multihash::wrap(0x0, &peer_id).expect("The digest size is never too large"),
        )
        .unwrap();
        let shard_len = rng.random_range(0..=max_shard_size);
        let shard: Vec<u8> = (0..shard_len).map(|_| rng.random()).collect();
        let sig_len = rng.random_range(0..=256);
        let signature: Vec<u8> = (0..sig_len).map(|_| rng.random()).collect();

        Self {
            id: ShredId {
                message_id: topic,
                index,
                publisher,
            },
            shard,
            signature,
        }
    }
}

impl Shred {
    /// Encode the shred into a byte vector using length-prefixed format.
    pub fn encode(&self, dst: &mut BytesMut, max_shred_size: usize) {
        let mut codec = PropellerCodec::new(max_shred_size);
        codec
            .encode(
                PropellerMessage {
                    shred: self.clone(),
                },
                dst,
            )
            .unwrap();
    }

    /// Decode a shred from a byte buffer using length-prefixed format.
    ///
    /// Returns Some(shred) if a complete message is available,
    /// None if the message is invalid.
    pub fn decode(src: &mut BytesMut, max_shred_size: usize) -> Option<Self> {
        let mut codec = PropellerCodec::new(max_shred_size);
        codec
            .decode(src)
            .ok()
            .flatten()
            .map(|message| message.shred)
    }
}

/// Convert from our Shred type to the protobuf Shred type.
impl From<Shred> for proto::Shred {
    fn from(shred: Shred) -> Self {
        proto::Shred {
            message_id: shred.id.message_id,
            index: shred.id.index,
            publisher: shred.id.publisher.to_bytes(),
            data: shred.shard,
            signature: shred.signature,
        }
    }
}

/// Convert from the protobuf Shred type to our Shred type.
impl TryFrom<proto::Shred> for Shred {
    type Error = String;

    fn try_from(proto_shred: proto::Shred) -> Result<Self, Self::Error> {
        let publisher = PeerId::from_bytes(&proto_shred.publisher)
            .map_err(|e| format!("Invalid publisher PeerId: {}", e))?;

        Ok(Shred {
            id: ShredId {
                message_id: proto_shred.message_id,
                index: proto_shred.index,
                publisher,
            },
            shard: proto_shred.data,
            signature: proto_shred.signature,
        })
    }
}

/// Messages exchanged in the Propeller protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PropellerMessage {
    /// A shred being propagated through the network.
    pub shred: Shred,
}

/// Convert from our PropellerMessage type to the protobuf PropellerMessage type.
impl From<PropellerMessage> for proto::PropellerMessage {
    fn from(msg: PropellerMessage) -> Self {
        proto::PropellerMessage {
            shred: Some(msg.shred.into()),
        }
    }
}

/// Convert from the protobuf PropellerMessage type to our PropellerMessage type.
impl TryFrom<proto::PropellerMessage> for PropellerMessage {
    type Error = String;

    fn try_from(proto_msg: proto::PropellerMessage) -> Result<Self, Self::Error> {
        let shred = proto_msg
            .shred
            .ok_or("Missing shred in PropellerMessage")?
            .try_into()?;

        Ok(PropellerMessage { shred })
    }
}
