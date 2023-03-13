// Copyright 2023 Protocol Labs.
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

use crate::error::{DecodingError, SigningError};
use crate::proto;
use quick_protobuf::{BytesReader, Writer};
use std::convert::TryFrom;

#[cfg(feature = "ed25519")]
use crate::ed25519;

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
use crate::rsa;

#[cfg(feature = "secp256k1")]
use crate::secp256k1;

#[cfg(feature = "ecdsa")]
use crate::ecdsa;

/// Identity keypair of a node.
///
/// # Example: Generating RSA keys with OpenSSL
///
/// ```text
/// openssl genrsa -out private.pem 2048
/// openssl pkcs8 -in private.pem -inform PEM -topk8 -out private.pk8 -outform DER -nocrypt
/// rm private.pem      # optional
/// ```
///
/// Loading the keys:
///
/// ```text
/// let mut bytes = std::fs::read("private.pk8").unwrap();
/// let keypair = Keypair::rsa_from_pkcs8(&mut bytes);
/// ```
///
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Keypair {
    /// An Ed25519 keypair.
    #[cfg(feature = "ed25519")]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `Keypair::into_ed25519` instead."
    )]
    Ed25519(ed25519::Keypair),
    /// An RSA keypair.
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `Keypair::into_rsa` instead."
    )]
    Rsa(rsa::Keypair),
    /// A Secp256k1 keypair.
    #[cfg(feature = "secp256k1")]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `Keypair::into_secp256k1` instead."
    )]
    Secp256k1(secp256k1::Keypair),
    /// An ECDSA keypair.
    #[cfg(feature = "ecdsa")]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `Keypair::into_ecdsa` instead."
    )]
    Ecdsa(ecdsa::Keypair),
}

impl Keypair {
    /// Generate a new Ed25519 keypair.
    #[cfg(feature = "ed25519")]
    pub fn generate_ed25519() -> Keypair {
        #[allow(deprecated)]
        Keypair::Ed25519(ed25519::Keypair::generate())
    }

    /// Generate a new Secp256k1 keypair.
    #[cfg(feature = "secp256k1")]
    pub fn generate_secp256k1() -> Keypair {
        #[allow(deprecated)]
        Keypair::Secp256k1(secp256k1::Keypair::generate())
    }

    /// Generate a new ECDSA keypair.
    #[cfg(feature = "ecdsa")]
    pub fn generate_ecdsa() -> Keypair {
        #[allow(deprecated)]
        Keypair::Ecdsa(ecdsa::Keypair::generate())
    }

    #[cfg(feature = "ed25519")]
    pub fn into_ed25519(self) -> Option<ed25519::Keypair> {
        #[allow(deprecated)]
        match self {
            Keypair::Ed25519(inner) => Some(inner),
            _ => None,
        }
    }

    #[cfg(feature = "secp256k1")]
    pub fn into_secp256k1(self) -> Option<secp256k1::Keypair> {
        #[allow(deprecated)]
        match self {
            Keypair::Secp256k1(inner) => Some(inner),
            _ => None,
        }
    }

    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    pub fn into_rsa(self) -> Option<rsa::Keypair> {
        #[allow(deprecated)]
        match self {
            Keypair::Rsa(inner) => Some(inner),
            _ => None,
        }
    }

    #[cfg(feature = "ecdsa")]
    pub fn into_ecdsa(self) -> Option<ecdsa::Keypair> {
        #[allow(deprecated)]
        match self {
            Keypair::Ecdsa(inner) => Some(inner),
            _ => None,
        }
    }

    /// Decode an keypair from a DER-encoded secret key in PKCS#8 PrivateKeyInfo
    /// format (i.e. unencrypted) as defined in [RFC5208].
    ///
    /// [RFC5208]: https://tools.ietf.org/html/rfc5208#section-5
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    pub fn rsa_from_pkcs8(pkcs8_der: &mut [u8]) -> Result<Keypair, DecodingError> {
        #[allow(deprecated)]
        rsa::Keypair::from_pkcs8(pkcs8_der).map(Keypair::Rsa)
    }

    /// Decode a keypair from a DER-encoded Secp256k1 secret key in an ECPrivateKey
    /// structure as defined in [RFC5915].
    ///
    /// [RFC5915]: https://tools.ietf.org/html/rfc5915
    #[cfg(feature = "secp256k1")]
    pub fn secp256k1_from_der(der: &mut [u8]) -> Result<Keypair, DecodingError> {
        #[allow(deprecated)]
        secp256k1::SecretKey::from_der(der)
            .map(|sk| Keypair::Secp256k1(secp256k1::Keypair::from(sk)))
    }

    #[cfg(feature = "ed25519")]
    pub fn ed25519_from_bytes(bytes: impl AsMut<[u8]>) -> Result<Keypair, DecodingError> {
        #[allow(deprecated)]
        Ok(Keypair::Ed25519(ed25519::Keypair::from(
            ed25519::SecretKey::from_bytes(bytes)?,
        )))
    }

    /// Sign a message using the private key of this keypair, producing
    /// a signature that can be verified using the corresponding public key.
    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, SigningError> {
        use Keypair::*;
        #[allow(deprecated)]
        match self {
            #[cfg(feature = "ed25519")]
            Ed25519(ref pair) => Ok(pair.sign(msg)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Rsa(ref pair) => pair.sign(msg),
            #[cfg(feature = "secp256k1")]
            Secp256k1(ref pair) => pair.secret().sign(msg),
            #[cfg(feature = "ecdsa")]
            Ecdsa(ref pair) => Ok(pair.secret().sign(msg)),
        }
    }

    /// Get the public key of this keypair.
    pub fn public(&self) -> PublicKey {
        use Keypair::*;
        #[allow(deprecated)]
        match self {
            #[cfg(feature = "ed25519")]
            Ed25519(pair) => PublicKey::Ed25519(pair.public()),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Rsa(pair) => PublicKey::Rsa(pair.public()),
            #[cfg(feature = "secp256k1")]
            Secp256k1(pair) => PublicKey::Secp256k1(pair.public().clone()),
            #[cfg(feature = "ecdsa")]
            Ecdsa(pair) => PublicKey::Ecdsa(pair.public().clone()),
        }
    }

    /// Encode a private key as protobuf structure.
    #[cfg_attr(
        not(feature = "ed25519"),
        allow(unreachable_code, unused_variables, unused_mut)
    )]
    pub fn to_protobuf_encoding(&self) -> Result<Vec<u8>, DecodingError> {
        use quick_protobuf::MessageWrite;

        #[cfg(not(feature = "ed25519"))]
        return Err(DecodingError::missing_feature("ed25519"));

        #[allow(deprecated)]
        let pk: proto::PrivateKey = match self {
            #[cfg(feature = "ed25519")]
            Self::Ed25519(data) => proto::PrivateKey {
                Type: proto::KeyType::Ed25519,
                Data: data.encode().to_vec(),
            },
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Self::Rsa(_) => return Err(DecodingError::encoding_unsupported("RSA")),
            #[cfg(feature = "secp256k1")]
            Self::Secp256k1(_) => return Err(DecodingError::encoding_unsupported("secp256k1")),
            #[cfg(feature = "ecdsa")]
            Self::Ecdsa(_) => return Err(DecodingError::encoding_unsupported("ECDSA")),
        };

        let mut buf = Vec::with_capacity(pk.get_size());
        let mut writer = Writer::new(&mut buf);
        pk.write_message(&mut writer).expect("Encoding to succeed");

        Ok(buf)
    }

    /// Decode a private key from a protobuf structure and parse it as a [`Keypair`].
    #[cfg_attr(not(feature = "ed25519"), allow(unused_mut))]
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Keypair, DecodingError> {
        use quick_protobuf::MessageRead;

        let mut reader = BytesReader::from_bytes(bytes);
        let mut private_key = proto::PrivateKey::from_reader(&mut reader, bytes)
            .map_err(|e| DecodingError::bad_protobuf("private key bytes", e))
            .map(zeroize::Zeroizing::new)?;

        match private_key.Type {
            #[cfg(feature = "ed25519")]
            proto::KeyType::Ed25519 =>
            {
                #[allow(deprecated)]
                ed25519::Keypair::decode(&mut private_key.Data).map(Keypair::Ed25519)
            }
            #[cfg(not(feature = "ed25519"))]
            proto::KeyType::Ed25519 => Err(DecodingError::missing_feature("ed25519")),
            proto::KeyType::RSA => Err(DecodingError::decoding_unsupported("RSA")),
            proto::KeyType::Secp256k1 => Err(DecodingError::decoding_unsupported("secp256k1")),
            proto::KeyType::ECDSA => Err(DecodingError::decoding_unsupported("ECDSA")),
        }
    }
}

/// The public key of a node's identity keypair.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PublicKey {
    /// A public Ed25519 key.
    #[cfg(feature = "ed25519")]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `PublicKey::into_ed25519` instead."
    )]
    Ed25519(ed25519::PublicKey),
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    /// A public RSA key.

    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `PublicKey::into_rsa` instead."
    )]
    Rsa(rsa::PublicKey),
    #[cfg(feature = "secp256k1")]
    /// A public Secp256k1 key.
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `PublicKey::into_secp256k1` instead."
    )]
    Secp256k1(secp256k1::PublicKey),
    /// A public ECDSA key.
    #[cfg(feature = "ecdsa")]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `PublicKey::into_ecdsa` instead."
    )]
    Ecdsa(ecdsa::PublicKey),
}

impl PublicKey {
    /// Verify a signature for a message using this public key, i.e. check
    /// that the signature has been produced by the corresponding
    /// private key (authenticity), and that the message has not been
    /// tampered with (integrity).
    #[must_use]
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        use PublicKey::*;
        #[allow(deprecated)]
        match self {
            #[cfg(feature = "ed25519")]
            Ed25519(pk) => pk.verify(msg, sig),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Rsa(pk) => pk.verify(msg, sig),
            #[cfg(feature = "secp256k1")]
            Secp256k1(pk) => pk.verify(msg, sig),
            #[cfg(feature = "ecdsa")]
            Ecdsa(pk) => pk.verify(msg, sig),
        }
    }

    #[cfg(feature = "ed25519")]
    pub fn into_ed25519(self) -> Option<ed25519::PublicKey> {
        #[allow(deprecated)]
        match self {
            PublicKey::Ed25519(inner) => Some(inner),
            _ => None,
        }
    }

    #[cfg(feature = "secp256k1")]
    pub fn into_secp256k1(self) -> Option<secp256k1::PublicKey> {
        #[allow(deprecated)]
        match self {
            PublicKey::Secp256k1(inner) => Some(inner),
            _ => None,
        }
    }

    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    pub fn into_rsa(self) -> Option<rsa::PublicKey> {
        #[allow(deprecated)]
        match self {
            PublicKey::Rsa(inner) => Some(inner),
            _ => None,
        }
    }

    #[cfg(feature = "ecdsa")]
    pub fn into_ecdsa(self) -> Option<ecdsa::PublicKey> {
        #[allow(deprecated)]
        match self {
            PublicKey::Ecdsa(inner) => Some(inner),
            _ => None,
        }
    }

    /// Encode the public key into a protobuf structure for storage or
    /// exchange with other nodes.
    pub fn to_protobuf_encoding(&self) -> Vec<u8> {
        use quick_protobuf::MessageWrite;

        let public_key = proto::PublicKey::from(self);

        let mut buf = Vec::with_capacity(public_key.get_size());
        let mut writer = Writer::new(&mut buf);
        public_key
            .write_message(&mut writer)
            .expect("Encoding to succeed");

        buf
    }

    /// Decode a public key from a protobuf structure, e.g. read from storage
    /// or received from another node.
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<PublicKey, DecodingError> {
        use quick_protobuf::MessageRead;

        let mut reader = BytesReader::from_bytes(bytes);

        let pubkey = proto::PublicKey::from_reader(&mut reader, bytes)
            .map_err(|e| DecodingError::bad_protobuf("public key bytes", e))?;

        pubkey.try_into()
    }

    /// Convert the `PublicKey` into the corresponding `PeerId`.
    #[cfg(feature = "peerid")]
    pub fn to_peer_id(&self) -> crate::PeerId {
        self.into()
    }
}

impl TryFrom<proto::PublicKey> for PublicKey {
    type Error = DecodingError;

    fn try_from(pubkey: proto::PublicKey) -> Result<Self, Self::Error> {
        #[allow(deprecated)]
        match pubkey.Type {
            #[cfg(feature = "ed25519")]
            proto::KeyType::Ed25519 => {
                ed25519::PublicKey::decode(&pubkey.Data).map(PublicKey::Ed25519)
            }
            #[cfg(not(feature = "ed25519"))]
            proto::KeyType::Ed25519 => {
                log::debug!("support for ed25519 was disabled at compile-time");
                Err(DecodingError::missing_feature("ed25519"))
            }
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            proto::KeyType::RSA => rsa::PublicKey::decode_x509(&pubkey.Data).map(PublicKey::Rsa),
            #[cfg(any(not(feature = "rsa"), target_arch = "wasm32"))]
            proto::KeyType::RSA => {
                log::debug!("support for RSA was disabled at compile-time");
                Err(DecodingError::missing_feature("rsa"))
            }
            #[cfg(feature = "secp256k1")]
            proto::KeyType::Secp256k1 => {
                secp256k1::PublicKey::decode(&pubkey.Data).map(PublicKey::Secp256k1)
            }
            #[cfg(not(feature = "secp256k1"))]
            proto::KeyType::Secp256k1 => {
                log::debug!("support for secp256k1 was disabled at compile-time");
                Err(DecodingError::missing_feature("secp256k1"))
            }
            #[cfg(feature = "ecdsa")]
            proto::KeyType::ECDSA => {
                ecdsa::PublicKey::decode_der(&pubkey.Data).map(PublicKey::Ecdsa)
            }
            #[cfg(not(feature = "ecdsa"))]
            proto::KeyType::ECDSA => {
                log::debug!("support for ECDSA was disabled at compile-time");
                Err(DecodingError::missing_feature("ecdsa"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PeerId;
    use base64::prelude::*;
    use std::str::FromStr;

    #[test]
    #[cfg(feature = "ed25519")]
    fn keypair_protobuf_roundtrip() {
        let expected_keypair = Keypair::generate_ed25519();
        let expected_peer_id = expected_keypair.public().to_peer_id();

        let encoded = expected_keypair.to_protobuf_encoding().unwrap();

        let keypair = Keypair::from_protobuf_encoding(&encoded).unwrap();
        let peer_id = keypair.public().to_peer_id();

        assert_eq!(expected_peer_id, peer_id);
    }

    #[test]
    fn keypair_from_protobuf_encoding() {
        // E.g. retrieved from an IPFS config file.
        let base_64_encoded = "CAESQL6vdKQuznQosTrW7FWI9At+XX7EBf0BnZLhb6w+N+XSQSdfInl6c7U4NuxXJlhKcRBlBw9d0tj2dfBIVf6mcPA=";
        let expected_peer_id =
            PeerId::from_str("12D3KooWEChVMMMzV8acJ53mJHrw1pQ27UAGkCxWXLJutbeUMvVu").unwrap();

        let encoded = BASE64_STANDARD.decode(base_64_encoded).unwrap();

        let keypair = Keypair::from_protobuf_encoding(&encoded).unwrap();
        let peer_id = keypair.public().to_peer_id();

        assert_eq!(expected_peer_id, peer_id);
    }

    #[test]
    fn public_key_implements_hash() {
        use crate::PublicKey;
        use std::hash::Hash;

        fn assert_implements_hash<T: Hash>() {}

        assert_implements_hash::<PublicKey>();
    }

    #[test]
    fn public_key_implements_ord() {
        use crate::PublicKey;
        use std::cmp::Ord;

        fn assert_implements_ord<T: Ord>() {}

        assert_implements_ord::<PublicKey>();
    }
}
