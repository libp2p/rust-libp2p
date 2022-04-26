use crate::identity::error::SigningError;
use crate::identity::Keypair;
use crate::signed_envelope::SignedEnvelope;
use crate::{peer_record_proto, signed_envelope, Multiaddr, PeerId};
use instant::SystemTime;
use std::convert::TryInto;
use std::fmt;

const PAYLOAD_TYPE: &str = "/libp2p/routing-state-record";
const DOMAIN_SEP: &str = "libp2p-routing-state";

/// Represents a peer routing record.
///
/// Peer records are designed to be distributable and carry a signature by being wrapped in a signed envelope.
/// For more information see RFC0003 of the libp2p specifications: <https://github.com/libp2p/specs/blob/master/RFC/0003-routing-records.md>
#[derive(Debug, PartialEq, Clone)]
pub struct PeerRecord {
    peer_id: PeerId,
    seq: u64,
    addresses: Vec<Multiaddr>,

    /// A signed envelope representing this [`PeerRecord`].
    ///
    /// If this [`PeerRecord`] was constructed from a [`SignedEnvelope`], this is the original instance.
    envelope: SignedEnvelope,
}

impl PeerRecord {
    /// Attempt to re-construct a [`PeerRecord`] from a [`SignedEnvelope`].
    ///
    /// If this function succeeds, the [`SignedEnvelope`] contained a peer record with a valid signature and can hence be considered authenticated.
    pub fn from_signed_envelope(envelope: SignedEnvelope) -> Result<Self, FromEnvelopeError> {
        use prost::Message;

        let (payload, signing_key) =
            envelope.payload_and_signing_key(String::from(DOMAIN_SEP), PAYLOAD_TYPE.as_bytes())?;
        let record = peer_record_proto::PeerRecord::decode(payload)?;

        let peer_id = PeerId::from_bytes(&record.peer_id)?;

        if peer_id != signing_key.to_peer_id() {
            return Err(FromEnvelopeError::MismatchedSignature);
        }

        let seq = record.seq;
        let addresses = record
            .addresses
            .into_iter()
            .map(|a| a.multiaddr.try_into())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            peer_id,
            seq,
            addresses,
            envelope,
        })
    }

    /// Construct a new [`PeerRecord`] by authenticating the provided addresses with the given key.
    ///
    /// This is the same key that is used for authenticating every libp2p connection of your application, i.e. what you use when setting up your [`crate::transport::Transport`].
    pub fn new(key: &Keypair, addresses: Vec<Multiaddr>) -> Result<Self, SigningError> {
        use prost::Message;

        let seq = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("now() is never before UNIX_EPOCH")
            .as_secs();
        let peer_id = key.public().to_peer_id();

        let payload = {
            let record = peer_record_proto::PeerRecord {
                peer_id: peer_id.to_bytes(),
                seq,
                addresses: addresses
                    .iter()
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
        };

        let envelope = SignedEnvelope::new(
            key,
            String::from(DOMAIN_SEP),
            PAYLOAD_TYPE.as_bytes().to_vec(),
            payload,
        )?;

        Ok(Self {
            peer_id,
            seq,
            addresses,
            envelope,
        })
    }

    pub fn to_signed_envelope(&self) -> SignedEnvelope {
        self.envelope.clone()
    }

    pub fn into_signed_envelope(self) -> SignedEnvelope {
        self.envelope
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn addresses(&self) -> &[Multiaddr] {
        self.addresses.as_slice()
    }
}

#[derive(Debug)]
pub enum FromEnvelopeError {
    /// Failed to extract the payload from the envelope.
    BadPayload(signed_envelope::ReadPayloadError),
    /// Failed to decode the provided bytes as a [`PeerRecord`].
    InvalidPeerRecord(prost::DecodeError),
    /// Failed to decode the peer ID.
    InvalidPeerId(multihash::Error),
    /// The signer of the envelope is different than the peer id in the record.
    MismatchedSignature,
    /// Failed to decode a multi-address.
    InvalidMultiaddr(multiaddr::Error),
}

impl From<signed_envelope::ReadPayloadError> for FromEnvelopeError {
    fn from(e: signed_envelope::ReadPayloadError) -> Self {
        Self::BadPayload(e)
    }
}

impl From<prost::DecodeError> for FromEnvelopeError {
    fn from(e: prost::DecodeError) -> Self {
        Self::InvalidPeerRecord(e)
    }
}

impl From<multihash::Error> for FromEnvelopeError {
    fn from(e: multihash::Error) -> Self {
        Self::InvalidPeerId(e)
    }
}

impl From<multiaddr::Error> for FromEnvelopeError {
    fn from(e: multiaddr::Error) -> Self {
        Self::InvalidMultiaddr(e)
    }
}

impl fmt::Display for FromEnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPayload(_) => write!(f, "Failed to extract payload from envelope"),
            Self::InvalidPeerRecord(_) => {
                write!(f, "Failed to decode bytes as PeerRecord")
            }
            Self::InvalidPeerId(_) => write!(f, "Failed to decode bytes as PeerId"),
            Self::MismatchedSignature => write!(
                f,
                "The signer of the envelope is different than the peer id in the record"
            ),
            Self::InvalidMultiaddr(_) => {
                write!(f, "Failed to decode bytes as MultiAddress")
            }
        }
    }
}

impl std::error::Error for FromEnvelopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPeerRecord(inner) => Some(inner),
            Self::InvalidPeerId(inner) => Some(inner),
            Self::MismatchedSignature => None,
            Self::InvalidMultiaddr(inner) => Some(inner),
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
        let key = Keypair::generate_ed25519();

        let record = PeerRecord::new(&key, vec![HOME.parse().unwrap()]).unwrap();

        let envelope = record.to_signed_envelope();
        let reconstructed = PeerRecord::from_signed_envelope(envelope).unwrap();

        assert_eq!(reconstructed, record)
    }

    #[test]
    fn mismatched_signature() {
        use prost::Message;

        let addr: Multiaddr = HOME.parse().unwrap();

        let envelope = {
            let identity_a = Keypair::generate_ed25519();
            let identity_b = Keypair::generate_ed25519();

            let payload = {
                let record = peer_record_proto::PeerRecord {
                    peer_id: identity_a.public().to_peer_id().to_bytes(),
                    seq: 0,
                    addresses: vec![peer_record_proto::peer_record::AddressInfo {
                        multiaddr: addr.to_vec(),
                    }],
                };

                let mut buf = Vec::with_capacity(record.encoded_len());
                record
                    .encode(&mut buf)
                    .expect("Vec<u8> provides capacity as needed");
                buf
            };

            SignedEnvelope::new(
                &identity_b,
                String::from(DOMAIN_SEP),
                PAYLOAD_TYPE.as_bytes().to_vec(),
                payload,
            )
            .unwrap()
        };

        assert!(matches!(
            PeerRecord::from_signed_envelope(envelope),
            Err(FromEnvelopeError::MismatchedSignature)
        ));
    }
}
