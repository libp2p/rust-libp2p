use libp2p_identity::{Keypair, PeerId, SigningError};
use quick_protobuf::{BytesReader, Writer};
use web_time::SystemTime;

use crate::{proto, signed_envelope, signed_envelope::SignedEnvelope, DecodeError, Multiaddr};

// Legacy constants for backward compatibility with existing Rust libp2p deployments.
const LEGACY_PAYLOAD_TYPE: &str = "/libp2p/routing-state-record";
const LEGACY_DOMAIN_SEP: &str = "libp2p-routing-state";

// Standard constants for cross-implementation compatibility with Go/JS libp2p.
// Defined in https://github.com/multiformats/multicodec/blob/master/table.csv
// and https://github.com/libp2p/specs/blob/master/RFC/0002-signed-envelopes.md.
const STANDARD_PAYLOAD_TYPE: &[u8] = &[0x03, 0x01];
const STANDARD_DOMAIN_SEP: &str = "libp2p-peer-record";

/// Represents a peer routing record.
///
/// Peer records are designed to be distributable and carry a signature by being wrapped in a signed
/// envelope. For more information see RFC0003 of the libp2p specifications: <https://github.com/libp2p/specs/blob/master/RFC/0003-routing-records.md>
///
/// ## Cross-Implementation Compatibility
///
/// This implementation provides two formats:
/// - **Legacy format** (default methods): Compatible with existing Rust libp2p deployments
/// - **Standard format** (`*_interop` methods): Compatible with Go and JavaScript implementations
///
/// Use the `*_interop` variants (e.g., [`PeerRecord::new_interop`],
/// [`PeerRecord::from_signed_envelope_interop`]) when you need to exchange peer records with
/// non-Rust libp2p implementations.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PeerRecord {
    peer_id: PeerId,
    seq: u64,
    addresses: Vec<Multiaddr>,

    /// A signed envelope representing this [`PeerRecord`].
    ///
    /// If this [`PeerRecord`] was constructed from a [`SignedEnvelope`], this is the original
    /// instance.
    envelope: SignedEnvelope,
}

impl PeerRecord {
    /// Attempt to re-construct a [`PeerRecord`] from a [`SignedEnvelope`] using legacy format.
    ///
    /// Uses the legacy routing-state-record format for backward compatibility with existing
    /// Rust libp2p deployments.
    ///
    /// If this function succeeds, the [`SignedEnvelope`] contained a peer record with a valid
    /// signature and can hence be considered authenticated.
    ///
    /// For cross-implementation compatibility with Go/JS libp2p, use
    /// [`Self::from_signed_envelope_interop`].
    pub fn from_signed_envelope(envelope: SignedEnvelope) -> Result<Self, FromEnvelopeError> {
        Self::from_signed_envelope_impl(envelope, LEGACY_DOMAIN_SEP, LEGACY_PAYLOAD_TYPE.as_bytes())
    }

    /// Attempt to re-construct a [`PeerRecord`] from a [`SignedEnvelope`] using standard interop
    /// format.
    ///
    /// Uses the standard libp2p-peer-record format for cross-implementation compatibility
    /// with Go and JavaScript libp2p implementations.
    ///
    /// If this function succeeds, the [`SignedEnvelope`] contained a peer record with a valid
    /// signature and can hence be considered authenticated.
    pub fn from_signed_envelope_interop(
        envelope: SignedEnvelope,
    ) -> Result<Self, FromEnvelopeError> {
        Self::from_signed_envelope_impl(envelope, STANDARD_DOMAIN_SEP, STANDARD_PAYLOAD_TYPE)
    }

    fn from_signed_envelope_impl(
        envelope: SignedEnvelope,
        domain: &str,
        payload_type: &[u8],
    ) -> Result<Self, FromEnvelopeError> {
        use quick_protobuf::MessageRead;

        let (payload, signing_key) =
            envelope.payload_and_signing_key(String::from(domain), payload_type)?;
        let mut reader = BytesReader::from_bytes(payload);
        let record = proto::PeerRecord::from_reader(&mut reader, payload).map_err(DecodeError)?;

        let peer_id = PeerId::from_bytes(&record.peer_id)?;

        if peer_id != signing_key.to_peer_id() {
            return Err(FromEnvelopeError::MismatchedSignature);
        }

        let seq = record.seq;
        let addresses = record
            .addresses
            .into_iter()
            .map(|a| a.multiaddr.to_vec().try_into())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            peer_id,
            seq,
            addresses,
            envelope,
        })
    }

    /// Construct a new [`PeerRecord`] by authenticating the provided addresses with the given key
    /// using legacy format.
    ///
    /// Uses the legacy routing-state-record format for backward compatibility with existing
    /// Rust libp2p deployments.
    ///
    /// This is the same key that is used for authenticating every libp2p connection of your
    /// application, i.e. what you use when setting up your [`crate::transport::Transport`].
    ///
    /// For cross-implementation compatibility with Go/JS libp2p, use [`Self::new_interop`].
    pub fn new(key: &Keypair, addresses: Vec<Multiaddr>) -> Result<Self, SigningError> {
        Self::new_impl(
            key,
            addresses,
            LEGACY_DOMAIN_SEP,
            LEGACY_PAYLOAD_TYPE.as_bytes(),
        )
    }

    /// Construct a new [`PeerRecord`] by authenticating the provided addresses with the given key
    /// using standard interop format.
    ///
    /// Uses the standard libp2p-peer-record format for cross-implementation compatibility
    /// with Go and JavaScript libp2p implementations.
    ///
    /// This is the same key that is used for authenticating every libp2p connection of your
    /// application, i.e. what you use when setting up your [`crate::transport::Transport`].
    pub fn new_interop(key: &Keypair, addresses: Vec<Multiaddr>) -> Result<Self, SigningError> {
        Self::new_impl(key, addresses, STANDARD_DOMAIN_SEP, STANDARD_PAYLOAD_TYPE)
    }

    fn new_impl(
        key: &Keypair,
        addresses: Vec<Multiaddr>,
        domain: &str,
        payload_type: &[u8],
    ) -> Result<Self, SigningError> {
        use quick_protobuf::MessageWrite;

        let seq = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("now() is never before UNIX_EPOCH")
            .as_secs();
        let peer_id = key.public().to_peer_id();

        let payload = {
            let record = proto::PeerRecord {
                peer_id: peer_id.to_bytes(),
                seq,
                addresses: addresses
                    .iter()
                    .map(|m| proto::AddressInfo {
                        multiaddr: m.to_vec(),
                    })
                    .collect(),
            };

            let mut buf = Vec::with_capacity(record.get_size());
            let mut writer = Writer::new(&mut buf);
            record
                .write_message(&mut writer)
                .expect("Encoding to succeed");

            buf
        };

        let envelope =
            SignedEnvelope::new(key, String::from(domain), payload_type.to_vec(), payload)?;

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

#[derive(thiserror::Error, Debug)]
pub enum FromEnvelopeError {
    /// Failed to extract the payload from the envelope.
    #[error("Failed to extract payload from envelope")]
    BadPayload(#[from] signed_envelope::ReadPayloadError),
    /// Failed to decode the provided bytes as a [`PeerRecord`].
    #[error("Failed to decode bytes as PeerRecord")]
    InvalidPeerRecord(#[from] DecodeError),
    /// Failed to decode the peer ID.
    #[error("Failed to decode bytes as PeerId")]
    InvalidPeerId(#[from] libp2p_identity::ParseError),
    /// The signer of the envelope is different than the peer id in the record.
    #[error("The signer of the envelope is different than the peer id in the record")]
    MismatchedSignature,
    /// Failed to decode a multi-address.
    #[error("Failed to decode bytes as MultiAddress")]
    InvalidMultiaddr(#[from] multiaddr::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/ip4/127.0.0.1/tcp/1337";

    #[test]
    fn roundtrip_envelope_legacy() {
        let key = Keypair::generate_ed25519();

        let record = PeerRecord::new(&key, vec![HOME.parse().unwrap()]).unwrap();

        let envelope = record.to_signed_envelope();
        let reconstructed = PeerRecord::from_signed_envelope(envelope).unwrap();

        assert_eq!(reconstructed, record)
    }

    #[test]
    fn roundtrip_envelope_interop() {
        let key = Keypair::generate_ed25519();

        let record = PeerRecord::new_interop(&key, vec![HOME.parse().unwrap()]).unwrap();

        let envelope = record.to_signed_envelope();
        let reconstructed = PeerRecord::from_signed_envelope_interop(envelope).unwrap();

        assert_eq!(reconstructed, record)
    }

    #[test]
    fn mismatched_signature_legacy() {
        use quick_protobuf::MessageWrite;

        let addr: Multiaddr = HOME.parse().unwrap();

        let envelope = {
            let identity_a = Keypair::generate_ed25519();
            let identity_b = Keypair::generate_ed25519();

            let payload = {
                let record = proto::PeerRecord {
                    peer_id: identity_a.public().to_peer_id().to_bytes(),
                    seq: 0,
                    addresses: vec![proto::AddressInfo {
                        multiaddr: addr.to_vec(),
                    }],
                };

                let mut buf = Vec::with_capacity(record.get_size());
                let mut writer = Writer::new(&mut buf);
                record
                    .write_message(&mut writer)
                    .expect("Encoding to succeed");

                buf
            };

            SignedEnvelope::new(
                &identity_b,
                String::from(LEGACY_DOMAIN_SEP),
                LEGACY_PAYLOAD_TYPE.as_bytes().to_vec(),
                payload,
            )
            .unwrap()
        };

        assert!(matches!(
            PeerRecord::from_signed_envelope(envelope),
            Err(FromEnvelopeError::MismatchedSignature)
        ));
    }

    #[test]
    fn mismatched_signature_interop() {
        use quick_protobuf::MessageWrite;

        let addr: Multiaddr = HOME.parse().unwrap();

        let envelope = {
            let identity_a = Keypair::generate_ed25519();
            let identity_b = Keypair::generate_ed25519();

            let payload = {
                let record = proto::PeerRecord {
                    peer_id: identity_a.public().to_peer_id().to_bytes(),
                    seq: 0,
                    addresses: vec![proto::AddressInfo {
                        multiaddr: addr.to_vec(),
                    }],
                };

                let mut buf = Vec::with_capacity(record.get_size());
                let mut writer = Writer::new(&mut buf);
                record
                    .write_message(&mut writer)
                    .expect("Encoding to succeed");

                buf
            };

            SignedEnvelope::new(
                &identity_b,
                String::from(STANDARD_DOMAIN_SEP),
                STANDARD_PAYLOAD_TYPE.to_vec(),
                payload,
            )
            .unwrap()
        };

        assert!(matches!(
            PeerRecord::from_signed_envelope_interop(envelope),
            Err(FromEnvelopeError::MismatchedSignature)
        ));
    }
}
