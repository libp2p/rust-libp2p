use crate::identity::Keypair;
use crate::signed_envelope::SignedEnvelope;
use crate::{peer_record_proto, signed_envelope, Multiaddr, PeerId};
use std::convert::TryInto;
use std::fmt;

const PAYLOAD_TYPE: &str = "/libp2p/routing-state-record";
const DOMAIN_SEP: &str = "libp2p-routing-state";

// TODO: docs
#[derive(Debug, PartialEq, Clone)]
pub struct PeerRecord {
    pub peer_id: PeerId,
    pub seq: u64,
    pub addresses: Vec<Multiaddr>,
}

impl PeerRecord {
    // TODO: docs
    pub fn into_protobuf_encoding(self) -> Vec<u8> {
        use prost::Message;

        let record = peer_record_proto::PeerRecord {
            peer_id: self.peer_id.to_bytes(),
            seq: self.seq,
            addresses: self
                .addresses
                .into_iter()
                .map(|m| peer_record_proto::peer_record::AddressInfo {
                    multiaddr: m.to_vec(),
                })
                .collect(),
        };

        let mut buf = Vec::with_capacity(record.encoded_len());
        record
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");
        buf
    }

    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Self, DecodingError> {
        use prost::Message;

        let record = peer_record_proto::PeerRecord::decode(bytes)?;

        Ok(Self {
            peer_id: PeerId::from_bytes(&record.peer_id)?,
            seq: record.seq,
            addresses: record
                .addresses
                .into_iter()
                .map(|a| a.multiaddr.try_into())
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    // TODO: docs
    pub fn wrap_in_envelope(self, key: Keypair) -> SignedEnvelope {
        if key.public().into_peer_id() != self.peer_id {
            panic!("bad key")
        }

        let payload = self.into_protobuf_encoding();

        SignedEnvelope::new(
            key,
            String::from(DOMAIN_SEP),
            PAYLOAD_TYPE.as_bytes().to_vec(),
            payload,
        )
        .unwrap() // TODO: Error handling
    }

    pub fn authenticate(self, key: Keypair) -> AuthenticatedPeerRecord {
        AuthenticatedPeerRecord::from_record(key, self)
    }
}

#[derive(Debug)]
pub enum DecodingError {
    /// Failed to decode the provided bytes as a [`PeerRecord`].
    InvalidPeerRecord(prost::DecodeError),
    /// Failed to decode the peer ID.
    InvalidPeerId(multihash::Error),
    /// Failed to decode a multi-address.
    InvalidMultiaddr(multiaddr::Error),
}

impl From<prost::DecodeError> for DecodingError {
    fn from(e: prost::DecodeError) -> Self {
        Self::InvalidPeerRecord(e)
    }
}

impl From<multihash::Error> for DecodingError {
    fn from(e: multihash::Error) -> Self {
        Self::InvalidPeerId(e)
    }
}

impl From<multiaddr::Error> for DecodingError {
    fn from(e: multiaddr::Error) -> Self {
        Self::InvalidMultiaddr(e)
    }
}

impl fmt::Display for DecodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodingError::InvalidPeerRecord(_) => {
                write!(f, "Failed to decode bytes as PeerRecord")
            }
            DecodingError::InvalidPeerId(_) => write!(f, "Failed to decode bytes as PeerId"),
            DecodingError::InvalidMultiaddr(_) => {
                write!(f, "Failed to decode bytes as MultiAddress")
            }
        }
    }
}

impl std::error::Error for DecodingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodingError::InvalidPeerRecord(inner) => Some(inner),
            DecodingError::InvalidPeerId(inner) => Some(inner),
            DecodingError::InvalidMultiaddr(inner) => Some(inner),
        }
    }
}

// TODO: docs
#[derive(Debug)]
pub struct AuthenticatedPeerRecord {
    inner: PeerRecord,

    /// A signed envelope containing the above inner [`PeerRecord`].
    ///
    /// If this [`AuthenticatedPeerRecord`] was constructed from a [`SignedEnvelope`], this is the original instance.
    /// If this [`AuthenticatedPeerRecord`] was created by [`authenticating`](PeerRecord::authenticate) an existing [`PeerRecord`], then this is a pre-computed [`SignedEnvelope`] to make it easier to send an [`AuthenticatedPeerRecord`] across the wire.
    envelope: SignedEnvelope,
}

impl AuthenticatedPeerRecord {
    // TODO: docs
    pub fn from_signed_envelope(envelope: SignedEnvelope) -> Result<Self, FromEnvelopeError> {
        let payload = envelope.payload(String::from(DOMAIN_SEP), PAYLOAD_TYPE.as_bytes())?;
        let record = PeerRecord::from_protobuf_encoding(payload)?;

        Ok(Self {
            inner: record,
            envelope,
        })
    }

    pub fn from_record(key: Keypair, record: PeerRecord) -> Self {
        let envelope = record.clone().wrap_in_envelope(key);

        Self {
            inner: record,
            envelope,
        }
    }

    pub fn to_signed_envelope(&self) -> SignedEnvelope {
        self.envelope.clone()
    }

    pub fn into_signed_envelope(self) -> SignedEnvelope {
        self.envelope
    }

    pub fn peer_id(&self) -> PeerId {
        self.inner.peer_id
    }

    pub fn seq(&self) -> u64 {
        self.inner.seq
    }

    pub fn addresses(&self) -> &[Multiaddr] {
        self.inner.addresses.as_slice()
    }
}

impl PartialEq<PeerRecord> for AuthenticatedPeerRecord {
    fn eq(&self, other: &PeerRecord) -> bool {
        self.inner.eq(other)
    }
}

impl PartialEq<AuthenticatedPeerRecord> for PeerRecord {
    fn eq(&self, other: &AuthenticatedPeerRecord) -> bool {
        other.inner.eq(self)
    }
}

#[derive(Debug)]
pub enum FromEnvelopeError {
    BadPayload(signed_envelope::ReadPayloadError),
    InvalidPeerRecord(DecodingError),
}

impl From<signed_envelope::ReadPayloadError> for FromEnvelopeError {
    fn from(e: signed_envelope::ReadPayloadError) -> Self {
        Self::BadPayload(e)
    }
}

impl From<DecodingError> for FromEnvelopeError {
    fn from(e: DecodingError) -> Self {
        Self::InvalidPeerRecord(e)
    }
}

impl fmt::Display for FromEnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPayload(_) => write!(f, "Failed to extract payload from envelope"),
            Self::InvalidPeerRecord(_) => write!(f, "Failed to decode payload as PeerRecord"),
        }
    }
}

impl std::error::Error for FromEnvelopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPeerRecord(inner) => Some(inner),
            Self::BadPayload(inner) => Some(inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/ip4/127.0.0.1/tcp/1337";

    #[test]
    fn roundtrip_envelope() {
        let identity = Keypair::generate_ed25519();
        let record = PeerRecord {
            peer_id: identity.public().into_peer_id(),
            seq: 0,
            addresses: vec![HOME.parse().unwrap()],
        };

        let envelope = record.clone().wrap_in_envelope(identity);
        let authenticated = AuthenticatedPeerRecord::from_signed_envelope(envelope).unwrap();

        assert_eq!(authenticated, record)
    }
}
