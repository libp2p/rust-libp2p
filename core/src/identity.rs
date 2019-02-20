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

pub mod ed25519;
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
pub mod rsa;
#[cfg(feature = "secp256k1")]
pub mod secp256k1;

pub mod error;

use self::error::*;
use crate::{PeerId, keys_proto};

/// Identity keypair of the local node.
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
/// let mut bytes = std::fs::read("private.pem").unwrap();
/// let keypair = Keypair::rsa_from_pkcs8(&mut bytes);
/// ```
///
#[derive(Clone)]
pub enum Keypair {
    Ed25519(ed25519::Keypair),
    #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
    Rsa(rsa::Keypair),
    #[cfg(feature = "secp256k1")]
    Secp256k1(secp256k1::Keypair)
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

    /// Decode an keypair from a DER-encoded secret key in PKCS#8 PrivateKeyInfo
    /// format (i.e. unencrypted) as defined in [RFC5208].
    ///
    /// [RFC5208]: https://tools.ietf.org/html/rfc5208#section-5
    #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
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
            #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
            Rsa(ref pair) => pair.sign(msg),
            #[cfg(feature = "secp256k1")]
            Secp256k1(ref pair) => Ok(pair.secret().sign(msg)),
        }
    }

    /// Get the public key of this keypair.
    pub fn public(&self) -> PublicKey {
        use Keypair::*;
        match self {
            Ed25519(pair) => PublicKey::Ed25519(pair.public()),
            #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
            Rsa(pair) => PublicKey::Rsa(pair.public()),
            #[cfg(feature = "secp256k1")]
            Secp256k1(pair) => PublicKey::Secp256k1(pair.public().clone()),
        }
    }
}

/// The public key of a node's identity keypair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicKey {
    Ed25519(ed25519::PublicKey),
    #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
    Rsa(rsa::PublicKey),
    #[cfg(feature = "secp256k1")]
    Secp256k1(secp256k1::PublicKey)
}

impl PublicKey {
    /// Verify a signature for a message using this public key, i.e. check
    /// that the signature has been produced by the corresponding
    /// private key (authenticity), and that the message has not been
    /// tampered with (integrity).
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        use PublicKey::*;
        match self {
            Ed25519(pk) => pk.verify(msg, sig),
            #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
            Rsa(pk) => pk.verify(msg, sig),
            #[cfg(feature = "secp256k1")]
            Secp256k1(pk) => pk.verify(msg, sig)
        }
    }

    /// Encode the public key into a protobuf structure for storage or
    /// exchange with other nodes.
    pub fn into_protobuf_encoding(self) -> Result<Vec<u8>, EncodingError> {
        use protobuf::Message;
        let mut public_key = keys_proto::PublicKey::new();
        match self {
            PublicKey::Ed25519(key) => {
                public_key.set_Type(keys_proto::KeyType::Ed25519);
                public_key.set_Data(key.encode().to_vec());
            },
            #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
            PublicKey::Rsa(key) => {
                let der = key.encode_x509()?;
                public_key.set_Type(keys_proto::KeyType::RSA);
                public_key.set_Data(der);
            },
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(key) => {
                public_key.set_Type(keys_proto::KeyType::Secp256k1);
                public_key.set_Data(key.encode().to_vec());
            },
        };

        public_key
            .write_to_bytes()
            .map_err(|e| EncodingError::new("Protobuf", e))
    }

    /// Decode a public key from a protobuf structure, e.g. read from storage
    /// or received from another node.
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<PublicKey, DecodingError> {
        #[allow(unused_mut)] // Due to conditional compilation.
        let mut pubkey = protobuf::parse_from_bytes::<keys_proto::PublicKey>(bytes)
            .map_err(|e| DecodingError::new("Protobuf", e))?;

        match pubkey.get_Type() {
            keys_proto::KeyType::Ed25519 => {
                ed25519::PublicKey::decode(pubkey.get_Data())
                    .map(PublicKey::Ed25519)
            },
            #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
            keys_proto::KeyType::RSA => {
                rsa::PublicKey::decode_x509(&pubkey.take_Data())
                    .map(PublicKey::Rsa)
            }
            #[cfg(any(target_os = "emscripten", target_os = "unknown"))]
            keys_proto::KeyType::RSA => {
                log::debug!("support for RSA was disabled at compile-time");
                Err("Unsupported".to_string().into())
            },
            #[cfg(feature = "secp256k1")]
            keys_proto::KeyType::Secp256k1 => {
                secp256k1::PublicKey::decode(pubkey.get_Data())
                    .map(PublicKey::Secp256k1)
            }
            #[cfg(not(feature = "secp256k1"))]
            keys_proto::KeyType::Secp256k1 => {
                log::debug!("support for secp256k1 was disabled at compile-time");
                Err("Unsupported".to_string().into())
            },
        }
    }

    pub fn into_peer_id(self) -> PeerId {
        self.into()
    }
}

