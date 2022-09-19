// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! A node's network identity keys.
//!
//! Such identity keys can be randomly generated on every startup,
//! but using already existing, fixed keys is usually required.
//! Though libp2p uses other crates (e.g. `ed25519_dalek`) internally,
//! such details are not exposed as part of libp2p's public interface
//! to keep them easily upgradable or replaceable (e.g. to `ed25519_zebra`).
//! Consequently, keys of external ed25519 or secp256k1 crates cannot be
//! directly converted into libp2p network identities.
//! Instead, loading fixed keys must use the standard, thus more portable
//! binary representation of the specific key type
//! (e.g. [ed25519 binary format](https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.5)).
//! All key types have functions to enable conversion to/from their binary representations.

#[cfg(feature = "ecdsa")]
pub mod ecdsa;
pub mod ed25519;
#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
pub mod rsa;
#[cfg(feature = "secp256k1")]
pub mod secp256k1;

pub mod error;

use self::error::*;
use crate::{keys_proto, PeerId};
use std::convert::{TryFrom, TryInto};

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
    Ed25519(ed25519::Keypair),
    /// An RSA keypair.
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    Rsa(rsa::Keypair),
    /// A Secp256k1 keypair.
    #[cfg(feature = "secp256k1")]
    Secp256k1(secp256k1::Keypair),
    /// An ECDSA keypair.
    #[cfg(feature = "ecdsa")]
    Ecdsa(ecdsa::Keypair),
}

impl Keypair {
    /// Generate a new Ed25519 keypair.
    pub fn generate_ed25519() -> Keypair {
        Keypair::Ed25519(ed25519::Keypair::generate())
    }

    /// Generate a new Secp256k1 keypair.
    #[cfg(feature = "secp256k1")]
    pub fn generate_secp256k1() -> Keypair {
        Keypair::Secp256k1(secp256k1::Keypair::generate())
    }

    /// Generate a new ECDSA keypair.
    #[cfg(feature = "ecdsa")]
    pub fn generate_ecdsa() -> Keypair {
        Keypair::Ecdsa(ecdsa::Keypair::generate())
    }

    /// Decode an keypair from a DER-encoded secret key in PKCS#8 PrivateKeyInfo
    /// format (i.e. unencrypted) as defined in [RFC5208].
    ///
    /// [RFC5208]: https://tools.ietf.org/html/rfc5208#section-5
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    pub fn rsa_from_pkcs8(pkcs8_der: &mut [u8]) -> Result<Keypair, DecodingError> {
        rsa::Keypair::from_pkcs8(pkcs8_der).map(Keypair::Rsa)
    }

    /// Decode a keypair from a DER-encoded Secp256k1 secret key in an ECPrivateKey
    /// structure as defined in [RFC5915].
    ///
    /// [RFC5915]: https://tools.ietf.org/html/rfc5915
    #[cfg(feature = "secp256k1")]
    pub fn secp256k1_from_der(der: &mut [u8]) -> Result<Keypair, DecodingError> {
        secp256k1::SecretKey::from_der(der)
            .map(|sk| Keypair::Secp256k1(secp256k1::Keypair::from(sk)))
    }

    /// Sign a message using the private key of this keypair, producing
    /// a signature that can be verified using the corresponding public key.
    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, SigningError> {
        use Keypair::*;
        match self {
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
        match self {
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
    pub fn to_protobuf_encoding(&self) -> Result<Vec<u8>, DecodingError> {
        use prost::Message;

        let pk = match self {
            Self::Ed25519(data) => keys_proto::PrivateKey {
                r#type: keys_proto::KeyType::Ed25519.into(),
                data: data.encode().into(),
            },
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Self::Rsa(_) => {
                return Err(DecodingError::new(
                    "Encoding RSA key into Protobuf is unsupported",
                ))
            }
            #[cfg(feature = "secp256k1")]
            Self::Secp256k1(_) => {
                return Err(DecodingError::new(
                    "Encoding Secp256k1 key into Protobuf is unsupported",
                ))
            }
            #[cfg(feature = "ecdsa")]
            Self::Ecdsa(_) => {
                return Err(DecodingError::new(
                    "Encoding ECDSA key into Protobuf is unsupported",
                ))
            }
        };

        Ok(pk.encode_to_vec())
    }

    /// Decode a private key from a protobuf structure and parse it as a [`Keypair`].
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Keypair, DecodingError> {
        use prost::Message;

        let mut private_key = keys_proto::PrivateKey::decode(bytes)
            .map_err(|e| DecodingError::new("Protobuf").source(e))
            .map(zeroize::Zeroizing::new)?;

        let key_type = keys_proto::KeyType::from_i32(private_key.r#type).ok_or_else(|| {
            DecodingError::new(format!("unknown key type: {}", private_key.r#type))
        })?;

        match key_type {
            keys_proto::KeyType::Ed25519 => {
                ed25519::Keypair::decode(&mut private_key.data).map(Keypair::Ed25519)
            }
            keys_proto::KeyType::Rsa => Err(DecodingError::new(
                "Decoding RSA key from Protobuf is unsupported.",
            )),
            keys_proto::KeyType::Secp256k1 => Err(DecodingError::new(
                "Decoding Secp256k1 key from Protobuf is unsupported.",
            )),
            keys_proto::KeyType::Ecdsa => Err(DecodingError::new(
                "Decoding ECDSA key from Protobuf is unsupported.",
            )),
        }
    }
}

impl zeroize::Zeroize for keys_proto::PrivateKey {
    fn zeroize(&mut self) {
        self.r#type.zeroize();
        self.data.zeroize();
    }
}

/// The public key of a node's identity keypair.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PublicKey {
    /// A public Ed25519 key.
    Ed25519(ed25519::PublicKey),
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    /// A public RSA key.
    Rsa(rsa::PublicKey),
    #[cfg(feature = "secp256k1")]
    /// A public Secp256k1 key.
    Secp256k1(secp256k1::PublicKey),
    /// A public ECDSA key.
    #[cfg(feature = "ecdsa")]
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
        match self {
            Ed25519(pk) => pk.verify(msg, sig),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Rsa(pk) => pk.verify(msg, sig),
            #[cfg(feature = "secp256k1")]
            Secp256k1(pk) => pk.verify(msg, sig),
            #[cfg(feature = "ecdsa")]
            Ecdsa(pk) => pk.verify(msg, sig),
        }
    }

    /// Encode the public key into a protobuf structure for storage or
    /// exchange with other nodes.
    pub fn to_protobuf_encoding(&self) -> Vec<u8> {
        use prost::Message;

        let public_key = keys_proto::PublicKey::from(self);

        let mut buf = Vec::with_capacity(public_key.encoded_len());
        public_key
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");
        buf
    }

    /// Decode a public key from a protobuf structure, e.g. read from storage
    /// or received from another node.
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<PublicKey, DecodingError> {
        use prost::Message;

        let pubkey = keys_proto::PublicKey::decode(bytes)
            .map_err(|e| DecodingError::new("Protobuf").source(e))?;

        pubkey.try_into()
    }

    /// Convert the `PublicKey` into the corresponding `PeerId`.
    pub fn to_peer_id(&self) -> PeerId {
        self.into()
    }
}

impl From<&PublicKey> for keys_proto::PublicKey {
    fn from(key: &PublicKey) -> Self {
        match key {
            PublicKey::Ed25519(key) => keys_proto::PublicKey {
                r#type: keys_proto::KeyType::Ed25519 as i32,
                data: key.encode().to_vec(),
            },
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            PublicKey::Rsa(key) => keys_proto::PublicKey {
                r#type: keys_proto::KeyType::Rsa as i32,
                data: key.encode_x509(),
            },
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(key) => keys_proto::PublicKey {
                r#type: keys_proto::KeyType::Secp256k1 as i32,
                data: key.encode().to_vec(),
            },
            #[cfg(feature = "ecdsa")]
            PublicKey::Ecdsa(key) => keys_proto::PublicKey {
                r#type: keys_proto::KeyType::Ecdsa as i32,
                data: key.encode_der(),
            },
        }
    }
}

impl TryFrom<keys_proto::PublicKey> for PublicKey {
    type Error = DecodingError;

    fn try_from(pubkey: keys_proto::PublicKey) -> Result<Self, Self::Error> {
        let key_type = keys_proto::KeyType::from_i32(pubkey.r#type)
            .ok_or_else(|| DecodingError::new(format!("unknown key type: {}", pubkey.r#type)))?;

        match key_type {
            keys_proto::KeyType::Ed25519 => {
                ed25519::PublicKey::decode(&pubkey.data).map(PublicKey::Ed25519)
            }
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            keys_proto::KeyType::Rsa => {
                rsa::PublicKey::decode_x509(&pubkey.data).map(PublicKey::Rsa)
            }
            #[cfg(any(not(feature = "rsa"), target_arch = "wasm32"))]
            keys_proto::KeyType::Rsa => {
                log::debug!("support for RSA was disabled at compile-time");
                Err(DecodingError::new("Unsupported"))
            }
            #[cfg(feature = "secp256k1")]
            keys_proto::KeyType::Secp256k1 => {
                secp256k1::PublicKey::decode(&pubkey.data).map(PublicKey::Secp256k1)
            }
            #[cfg(not(feature = "secp256k1"))]
            keys_proto::KeyType::Secp256k1 => {
                log::debug!("support for secp256k1 was disabled at compile-time");
                Err(DecodingError::new("Unsupported"))
            }
            #[cfg(feature = "ecdsa")]
            keys_proto::KeyType::Ecdsa => {
                ecdsa::PublicKey::decode_der(&pubkey.data).map(PublicKey::Ecdsa)
            }
            #[cfg(not(feature = "ecdsa"))]
            keys_proto::KeyType::Ecdsa => {
                log::debug!("support for ECDSA was disabled at compile-time");
                Err(DecodingError::new("Unsupported"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
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

        let encoded = base64::decode(base_64_encoded).unwrap();

        let keypair = Keypair::from_protobuf_encoding(&encoded).unwrap();
        let peer_id = keypair.public().to_peer_id();

        assert_eq!(expected_peer_id, peer_id);
    }

    #[test]
    fn public_key_implements_hash() {
        use std::hash::Hash;

        fn assert_implements_hash<T: Hash>() {}

        assert_implements_hash::<PublicKey>();
    }

    #[test]
    fn public_key_implements_ord() {
        use std::cmp::Ord;

        fn assert_implements_ord<T: Ord>() {}

        assert_implements_ord::<PublicKey>();
    }
}
