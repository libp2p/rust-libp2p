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

use crate::error::{DecodingError, OtherVariantError, SigningError};
use crate::{proto, KeyType};
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
        note = "This enum will be made opaque in the future, use `Keypair::try_into_ed25519` instead."
    )]
    Ed25519(ed25519::Keypair),
    /// An RSA keypair.
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `Keypair::try_into_rsa` instead."
    )]
    Rsa(rsa::Keypair),
    /// A Secp256k1 keypair.
    #[cfg(feature = "secp256k1")]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `Keypair::try_into_secp256k1` instead."
    )]
    Secp256k1(secp256k1::Keypair),
    /// An ECDSA keypair.
    #[cfg(feature = "ecdsa")]
    #[deprecated(
        since = "0.1.0",
        note = "This enum will be made opaque in the future, use `Keypair::try_into_ecdsa` instead."
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
    #[deprecated(
        since = "0.2.0",
        note = "This method name does not follow Rust naming conventions, use `Keypair::try_into_ed25519` instead."
    )]
    pub fn into_ed25519(self) -> Option<ed25519::Keypair> {
        self.try_into().ok()
    }

    #[cfg(feature = "ed25519")]
    pub fn try_into_ed25519(self) -> Result<ed25519::Keypair, OtherVariantError> {
        self.try_into()
    }

    #[cfg(feature = "secp256k1")]
    #[deprecated(
        since = "0.2.0",
        note = "This method name does not follow Rust naming conventions, use `Keypair::try_into_secp256k1` instead."
    )]
    pub fn into_secp256k1(self) -> Option<secp256k1::Keypair> {
        self.try_into().ok()
    }

    #[cfg(feature = "secp256k1")]
    pub fn try_into_secp256k1(self) -> Result<secp256k1::Keypair, OtherVariantError> {
        self.try_into()
    }

    #[cfg(feature = "rsa")]
    #[deprecated(
        since = "0.2.0",
        note = "This method name does not follow Rust naming conventions, use `Keypair::try_into_rsa` instead."
    )]
    pub fn into_rsa(self) -> Option<rsa::Keypair> {
        self.try_into().ok()
    }

    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    pub fn try_into_rsa(self) -> Result<rsa::Keypair, OtherVariantError> {
        self.try_into()
    }

    #[cfg(feature = "ecdsa")]
    #[deprecated(
        since = "0.2.0",
        note = "This method name does not follow Rust naming conventions, use `Keypair::try_into_ecdsa` instead."
    )]
    pub fn into_ecdsa(self) -> Option<ecdsa::Keypair> {
        self.try_into().ok()
    }

    #[cfg(feature = "ecdsa")]
    pub fn try_into_ecdsa(self) -> Result<ecdsa::Keypair, OtherVariantError> {
        self.try_into()
    }

    /// Decode an keypair from a DER-encoded secret key in PKCS#8 PrivateKeyInfo
    /// format (i.e. unencrypted) as defined in [RFC5208].
    ///
    /// [RFC5208]: https://tools.ietf.org/html/rfc5208#section-5
    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    #[deprecated(
        since = "0.2.0",
        note = "Deprecated, use `rsa::Keypair::try_decode_pkcs8` and promote it into `Keypair` instead."
    )]
    pub fn rsa_from_pkcs8(pkcs8_der: &mut [u8]) -> Result<Keypair, DecodingError> {
        #[allow(deprecated)]
        rsa::Keypair::try_decode_pkcs8(pkcs8_der).map(Keypair::Rsa)
    }

    /// Decode a keypair from a DER-encoded Secp256k1 secret key in an ECPrivateKey
    /// structure as defined in [RFC5915].
    ///
    /// [RFC5915]: https://tools.ietf.org/html/rfc5915
    #[cfg(feature = "secp256k1")]
    #[deprecated(
        since = "0.2.0",
        note = "Deprecated, use `secp256k1::Keypair::try_from_bytes` and promote it into `Keypair` instead."
    )]
    pub fn secp256k1_from_der(der: &mut [u8]) -> Result<Keypair, DecodingError> {
        #[allow(deprecated)]
        secp256k1::SecretKey::try_decode_der(der)
            .map(|sk| Keypair::Secp256k1(secp256k1::Keypair::from(sk)))
    }

    /// Decode a keypair from the [binary format](https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.5)
    /// produced by `ed25519::Keypair::encode`, zeroing the input on success.
    ///
    /// Note that this binary format is the same as `ed25519_dalek`'s and `ed25519_zebra`'s.
    #[cfg(feature = "ed25519")]
    #[deprecated(
        since = "0.2.0",
        note = "Deprecated, use `ed25519::Keypair::try_decode` and promote it into `Keypair` instead."
    )]
    pub fn ed25519_from_bytes(bytes: impl AsMut<[u8]>) -> Result<Keypair, DecodingError> {
        #[allow(deprecated)]
        Ok(Keypair::Ed25519(ed25519::Keypair::from(
            ed25519::SecretKey::try_from_bytes(bytes)?,
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
    #[deprecated(since = "0.2.0", note = "Renamed to `encode_protobuf`")]
    pub fn to_protobuf_encoding(&self) -> Vec<u8> {
        self.encode_protobuf()
    }

    /// Encode a private key as protobuf structure.
    /// 
    /// See <https://github.com/libp2p/specs/blob/master/peer-ids/peer-ids.md#key-types> for details on the encoding.
    pub fn encode_protobuf(&self) -> Vec<u8> {
        use quick_protobuf::MessageWrite;

        #[allow(deprecated)]
        let pk: proto::PrivateKey = match self {
            #[cfg(feature = "ed25519")]
            Self::Ed25519(data) => proto::PrivateKey {
                Type: proto::KeyType::Ed25519,
                Data: data.encode().to_vec(),
            },
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Self::Rsa(data) => proto::PrivateKey {
                Type: proto::KeyType::RSA,
                Data: data.to_raw_bytes(),
            },
            #[cfg(feature = "secp256k1")]
            Self::Secp256k1(data) => proto::PrivateKey {
                Type: proto::KeyType::Secp256k1,
                Data: data.secret().to_bytes().into(),
            },
            #[cfg(feature = "ecdsa")]
            Self::Ecdsa(data) => proto::PrivateKey {
                Type: proto::KeyType::ECDSA,
                Data: data.secret().to_bytes(),
            },
        };

        let mut buf = Vec::with_capacity(pk.get_size());
        let mut writer = Writer::new(&mut buf);
        pk.write_message(&mut writer).expect("Encoding to succeed");

        buf
    }

    /// Decode a private key from a protobuf structure and parse it as a [`Keypair`].
    #[deprecated(
        since = "0.2.0",
        note = "This method name does not follow Rust naming conventions, use `Keypair::try_decode_protobuf` instead."
    )]
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Keypair, DecodingError> {
        Self::try_decode_protobuf(bytes)
    }

    /// Try to decode a private key from a protobuf structure and parse it as a [`Keypair`].
    #[cfg_attr(not(feature = "ed25519"), allow(unused_mut))]
    pub fn try_decode_protobuf(bytes: &[u8]) -> Result<Keypair, DecodingError> {
        use quick_protobuf::MessageRead;

        let mut reader = BytesReader::from_bytes(bytes);
        let mut private_key = proto::PrivateKey::from_reader(&mut reader, bytes)
            .map_err(|e| DecodingError::bad_protobuf("private key bytes", e))
            .map(zeroize::Zeroizing::new)?;

        #[allow(deprecated, unreachable_code)]
        match private_key.Type {
            proto::KeyType::Ed25519 => {
                #[cfg(feature = "ed25519")]
                return ed25519::Keypair::try_decode(&mut private_key.Data).map(Keypair::Ed25519);
                Err(DecodingError::missing_feature("ed25519"))
            }
            proto::KeyType::RSA => {
                #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
                return rsa::Keypair::try_decode_pkcs8(&mut private_key.Data).map(Keypair::Rsa);
                Err(DecodingError::missing_feature("rsa"))
            }
            proto::KeyType::Secp256k1 => {
                #[cfg(feature = "secp256k1")]
                return secp256k1::Keypair::try_from_bytes(&mut private_key.Data)
                    .map(Keypair::Secp256k1);
                Err(DecodingError::missing_feature("secp256k1"))
            }
            proto::KeyType::ECDSA => {
                #[cfg(feature = "ecdsa")]
                return ecdsa::Keypair::try_from_bytes(&private_key.Data).map(Keypair::Ecdsa);
                Err(DecodingError::missing_feature("ECDSA"))
            }
        }
    }
}

#[cfg(feature = "ecdsa")]
impl From<ecdsa::Keypair> for Keypair {
    fn from(kp: ecdsa::Keypair) -> Self {
        #[allow(deprecated)]
        Keypair::Ecdsa(kp)
    }
}

#[cfg(feature = "ed25519")]
impl From<ed25519::Keypair> for Keypair {
    fn from(kp: ed25519::Keypair) -> Self {
        #[allow(deprecated)]
        Keypair::Ed25519(kp)
    }
}

#[cfg(feature = "secp256k1")]
impl From<secp256k1::Keypair> for Keypair {
    fn from(kp: secp256k1::Keypair) -> Self {
        #[allow(deprecated)]
        Keypair::Secp256k1(kp)
    }
}

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
impl From<rsa::Keypair> for Keypair {
    fn from(kp: rsa::Keypair) -> Self {
        #[allow(deprecated)]
        Keypair::Rsa(kp)
    }
}

#[cfg(feature = "ed25519")]
impl TryInto<ed25519::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ed25519::Keypair, Self::Error> {
        #[allow(deprecated)]
        match self {
            Keypair::Ed25519(inner) => Ok(inner),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Keypair::Rsa(_) => Err(OtherVariantError::new(KeyType::RSA)),
            #[cfg(feature = "secp256k1")]
            Keypair::Secp256k1(_) => Err(OtherVariantError::new(KeyType::Secp256k1)),
            #[cfg(feature = "ecdsa")]
            Keypair::Ecdsa(_) => Err(OtherVariantError::new(KeyType::Ecdsa)),
        }
    }
}

#[cfg(feature = "ecdsa")]
impl TryInto<ecdsa::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ecdsa::Keypair, Self::Error> {
        #[allow(deprecated)]
        match self {
            Keypair::Ecdsa(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            Keypair::Ed25519(_) => Err(OtherVariantError::new(KeyType::Ed25519)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Keypair::Rsa(_) => Err(OtherVariantError::new(KeyType::RSA)),
            #[cfg(feature = "secp256k1")]
            Keypair::Secp256k1(_) => Err(OtherVariantError::new(KeyType::Secp256k1)),
        }
    }
}

#[cfg(feature = "secp256k1")]
impl TryInto<secp256k1::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<secp256k1::Keypair, Self::Error> {
        #[allow(deprecated)]
        match self {
            Keypair::Secp256k1(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            Keypair::Ed25519(_) => Err(OtherVariantError::new(KeyType::Ed25519)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            Keypair::Rsa(_) => Err(OtherVariantError::new(KeyType::RSA)),
            #[cfg(feature = "ecdsa")]
            Keypair::Ecdsa(_) => Err(OtherVariantError::new(KeyType::Ecdsa)),
        }
    }
}

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
impl TryInto<rsa::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<rsa::Keypair, Self::Error> {
        #[allow(deprecated)]
        match self {
            Keypair::Rsa(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            Keypair::Ed25519(_) => Err(OtherVariantError::new(KeyType::Ed25519)),
            #[cfg(feature = "secp256k1")]
            Keypair::Secp256k1(_) => Err(OtherVariantError::new(KeyType::Secp256k1)),
            #[cfg(feature = "ecdsa")]
            Keypair::Ecdsa(_) => Err(OtherVariantError::new(KeyType::Ecdsa)),
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
    pub fn try_into_ed25519(self) -> Result<ed25519::PublicKey, OtherVariantError> {
        self.try_into()
    }

    #[cfg(feature = "secp256k1")]
    pub fn try_into_secp256k1(self) -> Result<secp256k1::PublicKey, OtherVariantError> {
        self.try_into()
    }

    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    pub fn try_into_rsa(self) -> Result<rsa::PublicKey, OtherVariantError> {
        self.try_into()
    }

    #[cfg(feature = "ecdsa")]
    pub fn try_into_ecdsa(self) -> Result<ecdsa::PublicKey, OtherVariantError> {
        self.try_into()
    }

    /// Encode the public key into a protobuf structure for storage or
    /// exchange with other nodes.
    #[deprecated(since = "0.2.0", note = "Renamed to `encode_protobuf`")]
    pub fn to_protobuf_encoding(&self) -> Vec<u8> {
        self.encode_protobuf()
    }

    /// Encode the public key into a protobuf structure for storage or
    /// exchange with other nodes.
    pub fn encode_protobuf(&self) -> Vec<u8> {
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
    #[deprecated(
        since = "0.2.0",
        note = "This method name does not follow Rust naming conventions, use `PublicKey::try_decode_protobuf` instead."
    )]
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<PublicKey, DecodingError> {
        Self::try_decode_protobuf(bytes)
    }

    /// Decode a public key from a protobuf structure, e.g. read from storage
    /// or received from another node.
    pub fn try_decode_protobuf(bytes: &[u8]) -> Result<PublicKey, DecodingError> {
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
        #[allow(unreachable_code)]
        match pubkey.Type {
            proto::KeyType::Ed25519 => {
                #[cfg(feature = "ed25519")]
                return ed25519::PublicKey::try_decode(&pubkey.Data).map(PublicKey::Ed25519);
                log::debug!("support for ed25519 was disabled at compile-time");
                Err(DecodingError::missing_feature("ed25519"))
            }
            proto::KeyType::RSA => {
                #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
                return rsa::PublicKey::try_decode_x509(&pubkey.Data).map(PublicKey::Rsa);
                log::debug!("support for RSA was disabled at compile-time");
                Err(DecodingError::missing_feature("rsa"))
            }
            proto::KeyType::Secp256k1 => {
                #[cfg(feature = "secp256k1")]
                return secp256k1::PublicKey::try_decode(&pubkey.Data).map(PublicKey::Secp256k1);
                log::debug!("support for secp256k1 was disabled at compile-time");
                Err(DecodingError::missing_feature("secp256k1"))
            }
            proto::KeyType::ECDSA => {
                #[cfg(feature = "ecdsa")]
                return ecdsa::PublicKey::try_decode_der(&pubkey.Data).map(PublicKey::Ecdsa);
                log::debug!("support for ECDSA was disabled at compile-time");
                Err(DecodingError::missing_feature("ecdsa"))
            }
        }
    }
}

#[cfg(feature = "ed25519")]
impl TryInto<ed25519::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ed25519::PublicKey, Self::Error> {
        #[allow(deprecated)]
        match self {
            PublicKey::Ed25519(inner) => Ok(inner),
            PublicKey::Rsa(_) => Err(OtherVariantError::new(KeyType::RSA)),
            PublicKey::Secp256k1(_) => Err(OtherVariantError::new(KeyType::Secp256k1)),
            PublicKey::Ecdsa(_) => Err(OtherVariantError::new(KeyType::Ecdsa)),
        }
    }
}

#[cfg(feature = "ecdsa")]
impl TryInto<ecdsa::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ecdsa::PublicKey, Self::Error> {
        #[allow(deprecated)]
        match self {
            PublicKey::Ecdsa(inner) => Ok(inner),
            PublicKey::Ed25519(_) => Err(OtherVariantError::new(KeyType::Ed25519)),
            PublicKey::Rsa(_) => Err(OtherVariantError::new(KeyType::RSA)),
            PublicKey::Secp256k1(_) => Err(OtherVariantError::new(KeyType::Secp256k1)),
        }
    }
}

#[cfg(feature = "secp256k1")]
impl TryInto<secp256k1::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<secp256k1::PublicKey, Self::Error> {
        #[allow(deprecated)]
        match self {
            PublicKey::Secp256k1(inner) => Ok(inner),
            PublicKey::Ed25519(_) => Err(OtherVariantError::new(KeyType::Ed25519)),
            PublicKey::Rsa(_) => Err(OtherVariantError::new(KeyType::RSA)),
            PublicKey::Ecdsa(_) => Err(OtherVariantError::new(KeyType::Ecdsa)),
        }
    }
}

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
impl TryInto<rsa::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<rsa::PublicKey, Self::Error> {
        #[allow(deprecated)]
        match self {
            PublicKey::Rsa(inner) => Ok(inner),
            PublicKey::Ed25519(_) => Err(OtherVariantError::new(KeyType::Ed25519)),
            PublicKey::Secp256k1(_) => Err(OtherVariantError::new(KeyType::Secp256k1)),
            PublicKey::Ecdsa(_) => Err(OtherVariantError::new(KeyType::Ecdsa)),
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
    fn keypair_protobuf_roundtrip_ed25519() {
        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!("080112407e0830617c4a7de83925dfb2694556b12936c477a0e1feb2e148ec9da60fee7d1ed1e8fae2c4a144b8be8fd4b47bf3d3b34b871c3cacf6010f0e42d474fce27e")).unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!(
            "080112201ed1e8fae2c4a144b8be8fd4b47bf3d3b34b871c3cacf6010f0e42d474fce27e"
        ))
        .unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key);
    }

    #[test]
    #[cfg(feature = "secp256k1")]
    fn keypair_protobuf_roundtrip_secp256k1() {
        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!(
            "080212201e4f6a12b43bec6871976295bcb13aace62a7e7b821334125d3ed3b720af419f"
        ))
        .unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!(
            "0802122102f0a81ddde0a3180610155ff3b2d98d683a6831fad0c84ba36cd49b81eaa7cf8f"
        ))
        .unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key);
    }

    #[test]
    #[cfg(feature = "ecdsa")]
    fn keypair_protobuf_roundtrip_ecdsa() {
        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!(
            "08031220f0d87659b402f0d47589e7670ca0954036f87b2fbf11fafbc66f4de7c3eb10a2"
        ))
        .unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!("0803125b3059301306072a8648ce3d020106082a8648ce3d03010703420004de6af15d8bc9b7f7c6eb8b32888d0da721d33f16af062306bafc64cdad741240cd61d6d9884c4899308ea25513a5cc03495ff88200dc7ae8e603ceb6698d2fee")).unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key);
    }

    #[test]
    #[cfg(feature = "rsa")]
    fn keypair_protobuf_roundtrip_rsa() {
        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!("080012c81230820944020100300d06092a864886f70d01010105000482092e3082092a02010002820201009c897f33e0d0b3297f2fe404ea5b7a98096b329693292aefc2d05ef1e82fd0e121ce74ec77d75ef4b532fa34dee2a19626f3389c6d2bb9b8de614e138302bc4254727a7ee35f7827f1094403bc2fe8e1f64d0e8a2a77e8f3a879f69f94a71f3589de184f5910d6b5270f58e684f71ddd3a3f486a4cb2c390194ee6e9b65f9f1dff7b8f6c0bf4e0c4ac683bd4ba2d2fd022fdaaa3db75e90e16662fc4b3aca4c9aa65514d51690cd372c2b96c61a1ed4f9298ec213d5398aa9120379477118391104deb77ab157a59b70714e95caa9b55d15fa386b0c80f36e50d738bdd10e0baa3c3eafb4703dec3d6a757601f18541eb87ae9111f60eae17d843cf1047dbf5a8982ad9ef0aa88f59b17689f1210a305f7da8a012c1a58e4e82b48811618e98cef13c9eb28ce6fcc589ea5d902149ee4f49f8b39758b349ca90be5a8bddf4a46bacaaa48aec1c0c6e996ab13f2cb351c351d40b0a7b8e0c12b366a8555c392b0aadf71fe746eb4f8ea0b829da6ddcc39081abdd40ea2f3d8778b9a3f06a480ef34234975e919c0d64d818f2e904a9f251c8669dbb1666cb2c28e955446fc7efd460d4677ed922ccff1e24bb5a8699e050075c7897a64daa1bc2f05e4132e76c4f72baea5d073042254236c116ea3e40540bb7986468b4468aadfadad068331ef9dbe13e4012196e8eb9f8cdba096c35f09e80893ea68f3253dc41053983855e50203010001028202002699dd6d4c960a68443dea0bb04308b32f37690d2a92ef4c9a8cc9acfba5b6eb9d6b8cf7b701bc1fba032d2216886a725d7e82ca483d8d19e274ba4d23746c3a2b1ae3cc2083ad5ca41ab5d3f9f712858e38284ab7f843d0ba0e015c0ecb3b6df766763632ef6d12d4e3faf73578bebb8c1e88dbf5b7eb73c059eda55a5cb01f349e229af143dc9d832a5cfeb33e6b58f717f8995987f5058d4e7b9f14f390db4e1297feea016eb141ce74ed1e125133db21acb0f1af88a91f0a83ca2fa678fc2fba1743b643a09d38fe1d1102d1eb6639304d61ec7c190c5f6576c5d9a8ccd2198a398ae75333feb51324ffc60b38cb2e90d8a2694b7c0048f47016bb15cb36c482e038e455254e35fc4f0e0babc84e046bd441b0291412c784e4e9639664cad07cb09a01626049cdbfd1d9ad75b314448df811f4988c6e64d93ebefc602b574d0763e31e9d567c891349cfe75f0ca37429b743d6452d1fffc1f9f4901e5f68772b4f24542d654fd29b893e44c85e6037bba304d48873721131f18248b16bd71384abd00f9336c73f071a4ca2456878070f9704ed7df0cd64e5c3e5949a78968525865b96e71d5015dc68bff857f2bba05a3976d83d8866d4dfe8caac144741ae97879a765dc0d4c7c34aa79ef6ebc86b5bf32b50ad995780f5f1a6c052eec5671164f407061a9c6bd49251b1bb7803bb222f5d859c321601236dd893dc9d810282010100cf13fe9908fe59e947122d5606cf9f70c123b7cb43a1916463e729f01dc31c3b70cb6a37bde542ecdc6029cea39b28c99c6395d0aaa29c1c4cf14b3fed9e0fcd793e31b7a09930352261c03b3dc0b66a62f8ae3771b705382cfeb6130d4a7e5b4854117a05767b99915099e2d542fc3fa505a0dbe217b169b46714384774380408bd8b3dbf0c9a177bbd3e64af115988159f485d70c885171007646765b50eb9bbebfabe60e71c69b2b822a124e235ad05f2b55cda9ddc78d671436981a3064a80c29bb37e6b5581a9372a6366c79af695a39ea0f3839ed77ec3985252f2e126955774727955b63ccbeff64208fd7280e8ba52e4297cb6bf72b44b07618923610282010100c184cd27d3a643df768764a7c66de40c222bdb4b7e02c35aa1e4a8377676247c629df58ecb5bb541fb4aac1bde35057b0b266bddd818876909b8fff1aca4859515069258d84b0c5178e4bff6842c68d39cad9a3a03aa6533fa76b92c995f381eb9c83f5e6118fd962807c931b7ca50dc20b261f2a71928f3e882af4da979cef843970cb2af68b86477b92ca90c8c0f1d640d39e943704366314c446f7a54851419e60f4e92e1e69bd52ee7294f9eddc6dc873144b0d0d9f13eb8d6aa955cf11edbd5a0673d8b70ef937e54fdaade185facc8437496d43a53169342280718a3679170ef4a0e582af4db598210fb64616f0d8daa08519d875e37c4d02e1af1c5050282010100c14865648c3b74cac3b698b06a4d130218947131fd9f69e8ed42d0273a706a02a546888f1ce547f173c52260a8dee354436fc45f6f55b626c83e94c147d637e3cede1963cf380d021b64681c2388a3fb6b03b9013157e63c47eb3b214f4f8fdf3e04920775dfe080375da7354d5f67b9341babc87121324c7ac197e2ebf6f36df8868ad8086207d6117e5325812ecd85b2c0e8b7a6d4d33cf28e23ce4ae593a8135ab0c1500b87beb4bd203d8f02c19d0d273cd73d8b094594cb4563ce47cf506d1cb85df28ad6d5de8f0a369bb185d7d1565672deb8a4e37983b1c26d801c5d7a19962c5f4a7c7e04d0a6e77e22aae4ddd54417890dca39aa23d4c03feed4210282010100915975de1c121d9892264f6bd496655ad7afa91ea29ee0ac0a3cfc3bec3600618c90a80780a67915fdf0b0249e59a4ac2e4bc568f30e3966a36ed88e64e58d8fd4230378c7bc569c3af955558b20effb410b0373df9cf4367e40fe04898e0350d0a99f2efc2f1108df3839dda5f5c7960ed8ecc89cc9410131fa364156b1aecab9b992480387dc3759d533be25366d83ddca315d0ad21f4d7a69965d44bc86d7fa3bd9f3624f5a2e6188c1073e4e4cb5389e325b2d93309f0a453ab71548a1b253dbb886d2ab114060bfda864cf853c648b88231e7b7afb70895c272de219b5a06db945f4336e5ccd393ff25522cab220644091a06731361a8f1a28b7ea169210282010100bd80196d3d11a8257b5f439776388f4d53e4da3690f710e9aff3e3e970e545ec92d285e7049da000d5364dd7f550c17cf662d516282fe89813cab322ce5aad5cc744c52a024dd1a94aa9484037281637d1c8e3503b6ed6231225c93f7865d29269c899bbf5d248cf9d41f9aee9b9cb2afac172ba17c2df0699c6604b4ce7ab95c91c5f7fc7804f2bde268a7e15c512920f7325cfba47463da1c201549fc44c2bc4fbe5d8619cde9733470c5e38b996f5c3633c6311af88663ce4d2d0dc415ac5c8258e1aa7659f9f35d4b90b7b9a5a888867d75636e6443cce5391c57d48d56409029edef53e1a5130eb1fa708758bc821e15f7c53edf6d4c6f868a6b5b0c1e6")).unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!("080012a60430820222300d06092a864886f70d01010105000382020f003082020a02820201009c897f33e0d0b3297f2fe404ea5b7a98096b329693292aefc2d05ef1e82fd0e121ce74ec77d75ef4b532fa34dee2a19626f3389c6d2bb9b8de614e138302bc4254727a7ee35f7827f1094403bc2fe8e1f64d0e8a2a77e8f3a879f69f94a71f3589de184f5910d6b5270f58e684f71ddd3a3f486a4cb2c390194ee6e9b65f9f1dff7b8f6c0bf4e0c4ac683bd4ba2d2fd022fdaaa3db75e90e16662fc4b3aca4c9aa65514d51690cd372c2b96c61a1ed4f9298ec213d5398aa9120379477118391104deb77ab157a59b70714e95caa9b55d15fa386b0c80f36e50d738bdd10e0baa3c3eafb4703dec3d6a757601f18541eb87ae9111f60eae17d843cf1047dbf5a8982ad9ef0aa88f59b17689f1210a305f7da8a012c1a58e4e82b48811618e98cef13c9eb28ce6fcc589ea5d902149ee4f49f8b39758b349ca90be5a8bddf4a46bacaaa48aec1c0c6e996ab13f2cb351c351d40b0a7b8e0c12b366a8555c392b0aadf71fe746eb4f8ea0b829da6ddcc39081abdd40ea2f3d8778b9a3f06a480ef34234975e919c0d64d818f2e904a9f251c8669dbb1666cb2c28e955446fc7efd460d4677ed922ccff1e24bb5a8699e050075c7897a64daa1bc2f05e4132e76c4f72baea5d073042254236c116ea3e40540bb7986468b4468aadfadad068331ef9dbe13e4012196e8eb9f8cdba096c35f09e80893ea68f3253dc41053983855e50203010001")).unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key);
    }

    fn roundtrip_protobuf_encoding(private_key: &Keypair, public_key: &PublicKey) {
        assert_eq!(&private_key.public(), public_key);

        let encoded_priv = private_key.encode_protobuf();
        let decoded_priv = Keypair::try_decode_protobuf(&encoded_priv).unwrap();

        assert_eq!(
            private_key.public().to_peer_id(),
            decoded_priv.public().to_peer_id(),
            "PeerId from roundtripped private key should be the same"
        );

        let encoded_public = private_key.public().encode_protobuf();
        let decoded_public = PublicKey::try_decode_protobuf(&encoded_public).unwrap();

        assert_eq!(
            private_key.public().to_peer_id(),
            decoded_public.to_peer_id(),
            "PeerId from roundtripped public key should be the same"
        );
    }

    #[test]
    fn keypair_from_protobuf_encoding() {
        // E.g. retrieved from an IPFS config file.
        let base_64_encoded = "CAESQL6vdKQuznQosTrW7FWI9At+XX7EBf0BnZLhb6w+N+XSQSdfInl6c7U4NuxXJlhKcRBlBw9d0tj2dfBIVf6mcPA=";
        let expected_peer_id =
            PeerId::from_str("12D3KooWEChVMMMzV8acJ53mJHrw1pQ27UAGkCxWXLJutbeUMvVu").unwrap();

        let encoded = BASE64_STANDARD.decode(base_64_encoded).unwrap();

        let keypair = Keypair::try_decode_protobuf(&encoded).unwrap();
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
