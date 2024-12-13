use std::fmt;

use libp2p_identity::{Keypair, PublicKey, SigningError};
use quick_protobuf::{BytesReader, Writer};
use unsigned_varint::encode::usize_buffer;

use crate::{proto, DecodeError};

/// A signed envelope contains an arbitrary byte string payload, a signature of the payload, and the
/// public key that can be used to verify the signature.
///
/// For more details see libp2p RFC0002: <https://github.com/libp2p/specs/blob/master/RFC/0002-signed-envelopes.md>
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedEnvelope {
    key: PublicKey,
    payload_type: Vec<u8>,
    payload: Vec<u8>,
    signature: Vec<u8>,
}

impl SignedEnvelope {
    /// Constructs a new [`SignedEnvelope`].
    pub fn new(
        key: &Keypair,
        domain_separation: String,
        payload_type: Vec<u8>,
        payload: Vec<u8>,
    ) -> Result<Self, SigningError> {
        let buffer = signature_payload(domain_separation, &payload_type, &payload);

        let signature = key.sign(&buffer)?;

        Ok(Self {
            key: key.public(),
            payload_type,
            payload,
            signature,
        })
    }

    /// Verify this [`SignedEnvelope`] against the provided domain-separation string.
    #[must_use]
    pub fn verify(&self, domain_separation: String) -> bool {
        let buffer = signature_payload(domain_separation, &self.payload_type, &self.payload);

        self.key.verify(&buffer, &self.signature)
    }

    /// Extract the payload and signing key of this [`SignedEnvelope`].
    ///
    /// You must provide the correct domain-separation string and expected payload type in order to
    /// get the payload. This guards against accidental mis-use of the payload where the
    /// signature was created for a different purpose or payload type.
    ///
    /// It is the caller's responsibility to check that the signing key is what
    /// is expected. For example, checking that the signing key is from a
    /// certain peer.
    pub fn payload_and_signing_key(
        &self,
        domain_separation: String,
        expected_payload_type: &[u8],
    ) -> Result<(&[u8], &PublicKey), ReadPayloadError> {
        if self.payload_type != expected_payload_type {
            return Err(ReadPayloadError::UnexpectedPayloadType {
                expected: expected_payload_type.to_vec(),
                got: self.payload_type.clone(),
            });
        }

        if !self.verify(domain_separation) {
            return Err(ReadPayloadError::InvalidSignature);
        }

        Ok((&self.payload, &self.key))
    }

    /// Encode this [`SignedEnvelope`] using the protobuf encoding specified in the RFC.
    pub fn into_protobuf_encoding(self) -> Vec<u8> {
        use quick_protobuf::MessageWrite;

        let envelope = proto::Envelope {
            public_key: self.key.encode_protobuf(),
            payload_type: self.payload_type,
            payload: self.payload,
            signature: self.signature,
        };

        let mut buf = Vec::with_capacity(envelope.get_size());
        let mut writer = Writer::new(&mut buf);

        envelope
            .write_message(&mut writer)
            .expect("Encoding to succeed");

        buf
    }

    /// Decode a [`SignedEnvelope`] using the protobuf encoding specified in the RFC.
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Self, DecodingError> {
        use quick_protobuf::MessageRead;

        let mut reader = BytesReader::from_bytes(bytes);
        let envelope = proto::Envelope::from_reader(&mut reader, bytes).map_err(DecodeError)?;

        Ok(Self {
            key: PublicKey::try_decode_protobuf(&envelope.public_key)?,
            payload_type: envelope.payload_type.to_vec(),
            payload: envelope.payload.to_vec(),
            signature: envelope.signature.to_vec(),
        })
    }
}

fn signature_payload(domain_separation: String, payload_type: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut domain_sep_length_buffer = usize_buffer();
    let domain_sep_length =
        unsigned_varint::encode::usize(domain_separation.len(), &mut domain_sep_length_buffer);

    let mut payload_type_length_buffer = usize_buffer();
    let payload_type_length =
        unsigned_varint::encode::usize(payload_type.len(), &mut payload_type_length_buffer);

    let mut payload_length_buffer = usize_buffer();
    let payload_length = unsigned_varint::encode::usize(payload.len(), &mut payload_length_buffer);

    let mut buffer = Vec::with_capacity(
        domain_sep_length.len()
            + domain_separation.len()
            + payload_type_length.len()
            + payload_type.len()
            + payload_length.len()
            + payload.len(),
    );

    buffer.extend_from_slice(domain_sep_length);
    buffer.extend_from_slice(domain_separation.as_bytes());
    buffer.extend_from_slice(payload_type_length);
    buffer.extend_from_slice(payload_type);
    buffer.extend_from_slice(payload_length);
    buffer.extend_from_slice(payload);

    buffer
}

/// Errors that occur whilst decoding a [`SignedEnvelope`] from its byte representation.
#[derive(thiserror::Error, Debug)]
pub enum DecodingError {
    /// Decoding the provided bytes as a signed envelope failed.
    #[error("Failed to decode envelope")]
    InvalidEnvelope(#[from] DecodeError),
    /// The public key in the envelope could not be converted to our internal public key type.
    #[error("Failed to convert public key")]
    InvalidPublicKey(#[from] libp2p_identity::DecodingError),
    /// The public key in the envelope could not be converted to our internal public key type.
    #[error("Public key is missing from protobuf struct")]
    MissingPublicKey,
}

/// Errors that occur whilst extracting the payload of a [`SignedEnvelope`].
#[derive(Debug)]
pub enum ReadPayloadError {
    /// The signature on the signed envelope does not verify
    /// with the provided domain separation string.
    InvalidSignature,
    /// The payload contained in the envelope is not of the expected type.
    UnexpectedPayloadType { expected: Vec<u8>, got: Vec<u8> },
}

impl fmt::Display for ReadPayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "Invalid signature"),
            Self::UnexpectedPayloadType { expected, got } => write!(
                f,
                "Unexpected payload type, expected {expected:?} but got {got:?}"
            ),
        }
    }
}

impl std::error::Error for ReadPayloadError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let kp = Keypair::generate_ed25519();
        let payload = "some payload".as_bytes();
        let domain_separation = "domain separation".to_string();
        let payload_type: Vec<u8> = "payload type".into();

        let env = SignedEnvelope::new(
            &kp,
            domain_separation.clone(),
            payload_type.clone(),
            payload.into(),
        )
        .expect("Failed to create envelope");

        let (actual_payload, signing_key) = env
            .payload_and_signing_key(domain_separation, &payload_type)
            .expect("Failed to extract payload and public key");

        assert_eq!(actual_payload, payload);
        assert_eq!(signing_key, &kp.public());
    }
}
