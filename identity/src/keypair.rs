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

#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
use crate::error::OtherVariantError;
use crate::error::{DecodingError, SigningError};
#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
use crate::proto;
#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
use quick_protobuf::{BytesReader, Writer};
#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
use std::convert::TryFrom;

#[cfg(feature = "ed25519")]
use crate::ed25519;

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
use crate::rsa;

#[cfg(feature = "secp256k1")]
use crate::secp256k1;

#[cfg(feature = "ecdsa")]
use crate::ecdsa;
use crate::KeyType;

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
pub struct Keypair {
    keypair: KeyPairInner,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum KeyPairInner {
    /// An Ed25519 keypair.
    #[cfg(feature = "ed25519")]
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
    #[cfg(all(feature = "ed25519", feature = "rand"))]
    pub fn generate_ed25519() -> Keypair {
        Keypair {
            keypair: KeyPairInner::Ed25519(ed25519::Keypair::generate()),
        }
    }

    /// Generate a new Secp256k1 keypair.
    #[cfg(all(feature = "secp256k1", feature = "rand"))]
    pub fn generate_secp256k1() -> Keypair {
        Keypair {
            keypair: KeyPairInner::Secp256k1(secp256k1::Keypair::generate()),
        }
    }

    /// Generate a new ECDSA keypair.
    #[cfg(all(feature = "ecdsa", feature = "rand"))]
    pub fn generate_ecdsa() -> Keypair {
        Keypair {
            keypair: KeyPairInner::Ecdsa(ecdsa::Keypair::generate()),
        }
    }

    #[cfg(feature = "ed25519")]
    pub fn try_into_ed25519(self) -> Result<ed25519::Keypair, OtherVariantError> {
        self.try_into()
    }

    #[cfg(feature = "secp256k1")]
    pub fn try_into_secp256k1(self) -> Result<secp256k1::Keypair, OtherVariantError> {
        self.try_into()
    }

    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
    pub fn try_into_rsa(self) -> Result<rsa::Keypair, OtherVariantError> {
        self.try_into()
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
        note = "Use `rsa::Keypair::try_decode_pkcs8` and promote it into `Keypair` instead."
    )]
    pub fn rsa_from_pkcs8(pkcs8_der: &mut [u8]) -> Result<Keypair, DecodingError> {
        rsa::Keypair::try_decode_pkcs8(pkcs8_der).map(|kp| Keypair {
            keypair: KeyPairInner::Rsa(kp),
        })
    }

    /// Decode a keypair from a DER-encoded Secp256k1 secret key in an ECPrivateKey
    /// structure as defined in [RFC5915].
    ///
    /// [RFC5915]: https://tools.ietf.org/html/rfc5915
    #[cfg(feature = "secp256k1")]
    #[deprecated(
        note = "Use `secp256k1::Keypair::try_from_bytes` and promote it into `Keypair` instead."
    )]
    pub fn secp256k1_from_der(der: &mut [u8]) -> Result<Keypair, DecodingError> {
        secp256k1::SecretKey::try_decode_der(der).map(|sk| Keypair {
            keypair: KeyPairInner::Secp256k1(secp256k1::Keypair::from(sk)),
        })
    }

    /// Decode a keypair from the [binary format](https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.5)
    /// produced by `ed25519::Keypair::encode`, zeroing the input on success.
    ///
    /// Note that this binary format is the same as `ed25519_dalek`'s and `ed25519_zebra`'s.
    #[cfg(feature = "ed25519")]
    #[deprecated(
        note = "Use `ed25519::Keypair::try_decode` and promote it into `Keypair` instead."
    )]
    pub fn ed25519_from_bytes(bytes: impl AsMut<[u8]>) -> Result<Keypair, DecodingError> {
        Ok(Keypair {
            keypair: KeyPairInner::Ed25519(ed25519::Keypair::from(
                ed25519::SecretKey::try_from_bytes(bytes)?,
            )),
        })
    }

    /// Sign a message using the private key of this keypair, producing
    /// a signature that can be verified using the corresponding public key.
    #[allow(unused_variables)]
    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, SigningError> {
        match self.keypair {
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(ref pair) => Ok(pair.sign(msg)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(ref pair) => pair.sign(msg),
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(ref pair) => Ok(pair.secret().sign(msg)),
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(ref pair) => Ok(pair.secret().sign(msg)),
        }
    }

    /// Get the public key of this keypair.
    pub fn public(&self) -> PublicKey {
        match self.keypair {
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(ref pair) => PublicKey {
                publickey: PublicKeyInner::Ed25519(pair.public()),
            },
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(ref pair) => PublicKey {
                publickey: PublicKeyInner::Rsa(pair.public()),
            },
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(ref pair) => PublicKey {
                publickey: PublicKeyInner::Secp256k1(pair.public().clone()),
            },
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(ref pair) => PublicKey {
                publickey: PublicKeyInner::Ecdsa(pair.public().clone()),
            },
        }
    }

    /// Encode a private key as protobuf structure.
    #[deprecated(
        note = "This method name does not follow Rust naming conventions, use `Keypair::try_into_ed25519` instead."
    )]
    pub fn to_protobuf_encoding(&self) -> Result<Vec<u8>, DecodingError> {

        self.encode_protobuf()
    }
  
    /// Encode a private key as protobuf structure.
    ///
    /// See <https://github.com/libp2p/specs/blob/master/peer-ids/peer-ids.md#key-types> for details on the encoding.
    pub fn encode_protobuf(&self) -> Result<Vec<u8>, DecodingError> {
        #[cfg(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        ))]
        {
            use quick_protobuf::MessageWrite;
            let pk: proto::PrivateKey = match self.keypair {
                #[cfg(feature = "ed25519")]
                KeyPairInner::Ed25519(ref data) => proto::PrivateKey {
                    Type: proto::KeyType::Ed25519,
                    Data: data.to_bytes().to_vec(),
                },
                #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
                KeyPairInner::Rsa(_) => return Err(DecodingError::encoding_unsupported("RSA")),
                #[cfg(feature = "secp256k1")]
                KeyPairInner::Secp256k1(ref data) => proto::PrivateKey {
                    Type: proto::KeyType::Secp256k1,
                    Data: data.secret().to_bytes().to_vec(),
                },
                #[cfg(feature = "ecdsa")]
                KeyPairInner::Ecdsa(ref data) => proto::PrivateKey {
                    Type: proto::KeyType::ECDSA,
                    Data: data.secret().encode_der(),
                },
            };

            let mut buf = Vec::with_capacity(pk.get_size());
            let mut writer = Writer::new(&mut buf);
            pk.write_message(&mut writer).expect("Encoding to succeed");

            Ok(buf)
        }

        #[cfg(not(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        )))]
        unreachable!()
    }

    /// Decode a private key from a protobuf structure and parse it as a [`Keypair`].
    #[deprecated(
        note = "This method name does not follow Rust naming conventions, use `Keypair::try_decode_protobuf` instead."
    )]
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Keypair, DecodingError> {
        Self::try_decode_protobuf(bytes)
    }

    /// Try to decode a private key from a protobuf structure and parse it as a [`Keypair`].
    #[cfg_attr(not(feature = "ed25519"), allow(unused_mut))]
    pub fn try_decode_protobuf(bytes: &[u8]) -> Result<Keypair, DecodingError> {
        #[cfg(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        ))]
        {
            use quick_protobuf::MessageRead;

            let mut reader = BytesReader::from_bytes(bytes);
            let mut private_key = proto::PrivateKey::from_reader(&mut reader, bytes)
                .map_err(|e| DecodingError::bad_protobuf("private key bytes", e))
                .map(zeroize::Zeroizing::new)?;

            #[allow(unreachable_code)]
            match private_key.Type {
                proto::KeyType::Ed25519 => {
                    #[cfg(feature = "ed25519")]
                    return ed25519::Keypair::try_from_bytes(&mut private_key.Data).map(|sk| {
                        Keypair {
                            keypair: KeyPairInner::Ed25519(sk),
                        }
                    });
                    Err(DecodingError::missing_feature("ed25519"))
                }
                proto::KeyType::RSA => {
                    #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
                    return rsa::Keypair::try_decode_pkcs1(&mut private_key.Data).map(|sk| {
                        Keypair {
                            keypair: KeyPairInner::Rsa(sk),
                        }
                    });
                    Err(DecodingError::missing_feature("rsa"))
                }
                proto::KeyType::Secp256k1 => {
                    #[cfg(feature = "secp256k1")]
                    return secp256k1::SecretKey::try_from_bytes(&mut private_key.Data).map(
                        |key| Keypair {
                            keypair: KeyPairInner::Secp256k1(key.into()),
                        },
                    );

                    Err(DecodingError::missing_feature("secp256k1"))
                }
                proto::KeyType::ECDSA => {
                    #[cfg(feature = "ecdsa")]
                    return ecdsa::SecretKey::try_decode_der(&mut private_key.Data).map(|key| {
                        Keypair {
                            keypair: KeyPairInner::Ecdsa(key.into()),
                        }
                    });

                    Err(DecodingError::missing_feature("ecdsa"))
                }
            }

        #[cfg(not(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        )))]
        unreachable!()
    }

    /// Return a [`KeyType`] of the [`Keypair`].
    pub fn key_type(&self) -> KeyType {
        match self.keypair {
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(_) => KeyType::Ed25519,
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(_) => KeyType::RSA,
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(_) => KeyType::Secp256k1,
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(_) => KeyType::Ecdsa,
        }
    }

    /// Deterministically derive a new secret from this [`Keypair`], taking into account the provided domain.
    ///
    /// This works for all key types except RSA where it returns `None`.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() {
    /// # use libp2p_identity as identity;
    /// let key = identity::Keypair::generate_ed25519();
    ///
    /// let new_key = key.derive_secret(b"my encryption key").expect("can derive secret for ed25519");
    /// # }
    /// ```
    ///
    #[cfg(any(
        feature = "ecdsa",
        feature = "secp256k1",
        feature = "ed25519",
        feature = "rsa"
    ))]
    pub fn derive_secret(&self, domain: &[u8]) -> Option<[u8; 32]> {
        let mut okm = [0u8; 32];
        hkdf::Hkdf::<sha2::Sha256>::new(None, &self.secret()?)
            .expand(domain, &mut okm)
            .expect("okm.len() == 32");

        Some(okm)
    }

    // We build docs with all features so this doesn't need to have any docs.
    #[cfg(not(any(
        feature = "ecdsa",
        feature = "secp256k1",
        feature = "ed25519",
        feature = "rsa"
    )))]
    pub fn derive_secret(&self, _: &[u8]) -> Option<[u8; 32]> {
        None
    }

    /// Return the secret key of the [`Keypair`].
    #[allow(dead_code)]
    pub(crate) fn secret(&self) -> Option<[u8; 32]> {
        match self.keypair {
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(ref inner) => Some(inner.secret().to_bytes()),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(_) => None,
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(ref inner) => Some(inner.secret().to_bytes()),
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(ref inner) => Some(
                inner
                    .secret()
                    .to_bytes()
                    .try_into()
                    .expect("Ecdsa's private key should be 32 bytes"),
            ),
        }

        #[cfg(not(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        )))]
        unreachable!()
    }

    /// Return a [`KeyType`] of the [`Keypair`].
    pub fn key_type(&self) -> KeyType {
        match self.keypair {
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(_) => KeyType::Ed25519,
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(_) => KeyType::RSA,
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(_) => KeyType::Secp256k1,
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(_) => KeyType::Ecdsa,
        }
    }

    /// Deterministically derive a new secret from this [`Keypair`], taking into account the provided domain.
    ///
    /// This works for all key types except RSA where it returns `None`.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() {
    /// # use libp2p_identity as identity;
    /// let key = identity::Keypair::generate_ed25519();
    ///
    /// let new_key = key.derive_secret(b"my encryption key").expect("can derive secret for ed25519");
    /// # }
    /// ```
    ///
    #[cfg(any(
        feature = "ecdsa",
        feature = "secp256k1",
        feature = "ed25519",
        feature = "rsa"
    ))]
    pub fn derive_secret(&self, domain: &[u8]) -> Option<[u8; 32]> {
        let mut okm = [0u8; 32];
        hkdf::Hkdf::<sha2::Sha256>::new(None, &self.secret()?)
            .expand(domain, &mut okm)
            .expect("okm.len() == 32");

        Some(okm)
    }

    // We build docs with all features so this doesn't need to have any docs.
    #[cfg(not(any(
        feature = "ecdsa",
        feature = "secp256k1",
        feature = "ed25519",
        feature = "rsa"
    )))]
    pub fn derive_secret(&self, _: &[u8]) -> Option<[u8; 32]> {
        None
    }

    /// Return the secret key of the [`Keypair`].
    #[allow(dead_code)]
    pub(crate) fn secret(&self) -> Option<[u8; 32]> {
        match self.keypair {
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(ref inner) => Some(inner.secret().to_bytes()),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(_) => None,
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(ref inner) => Some(inner.secret().to_bytes()),
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(ref inner) => Some(
                inner
                    .secret()
                    .to_bytes()
                    .try_into()
                    .expect("Ecdsa's private key should be 32 bytes"),
            ),
        }
    }
}
#[cfg(feature = "ecdsa")]
impl From<ecdsa::Keypair> for Keypair {
    fn from(kp: ecdsa::Keypair) -> Self {
        Keypair {
            keypair: KeyPairInner::Ecdsa(kp),
        }
    }
}

#[cfg(feature = "ed25519")]
impl From<ed25519::Keypair> for Keypair {
    fn from(kp: ed25519::Keypair) -> Self {
        Keypair {
            keypair: KeyPairInner::Ed25519(kp),
        }
    }
}

#[cfg(feature = "secp256k1")]
impl From<secp256k1::Keypair> for Keypair {
    fn from(kp: secp256k1::Keypair) -> Self {
        Keypair {
            keypair: KeyPairInner::Secp256k1(kp),
        }
    }
}

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
impl From<rsa::Keypair> for Keypair {
    fn from(kp: rsa::Keypair) -> Self {
        Keypair {
            keypair: KeyPairInner::Rsa(kp),
        }
    }
}

#[cfg(feature = "ed25519")]
impl TryInto<ed25519::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ed25519::Keypair, Self::Error> {
        match self.keypair {
            KeyPairInner::Ed25519(inner) => Ok(inner),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(_) => Err(OtherVariantError::new(crate::KeyType::RSA)),
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(_) => Err(OtherVariantError::new(crate::KeyType::Secp256k1)),
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(_) => Err(OtherVariantError::new(crate::KeyType::Ecdsa)),
        }
    }
}

#[cfg(feature = "ecdsa")]
impl TryInto<ecdsa::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ecdsa::Keypair, Self::Error> {
        match self.keypair {
            KeyPairInner::Ecdsa(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(_) => Err(OtherVariantError::new(crate::KeyType::Ed25519)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(_) => Err(OtherVariantError::new(crate::KeyType::RSA)),
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(_) => Err(OtherVariantError::new(crate::KeyType::Secp256k1)),
        }
    }
}

#[cfg(feature = "secp256k1")]
impl TryInto<secp256k1::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<secp256k1::Keypair, Self::Error> {
        match self.keypair {
            KeyPairInner::Secp256k1(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(_) => Err(OtherVariantError::new(crate::KeyType::Ed25519)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            KeyPairInner::Rsa(_) => Err(OtherVariantError::new(crate::KeyType::RSA)),
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(_) => Err(OtherVariantError::new(crate::KeyType::Ecdsa)),
        }
    }
}

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
impl TryInto<rsa::Keypair> for Keypair {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<rsa::Keypair, Self::Error> {
        match self.keypair {
            KeyPairInner::Rsa(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            KeyPairInner::Ed25519(_) => Err(OtherVariantError::new(crate::KeyType::Ed25519)),
            #[cfg(feature = "secp256k1")]
            KeyPairInner::Secp256k1(_) => Err(OtherVariantError::new(crate::KeyType::Secp256k1)),
            #[cfg(feature = "ecdsa")]
            KeyPairInner::Ecdsa(_) => Err(OtherVariantError::new(crate::KeyType::Ecdsa)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum PublicKeyInner {
    /// A public Ed25519 key.
    #[cfg(feature = "ed25519")]
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

/// The public key of a node's identity keypair.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PublicKey {
    pub(crate) publickey: PublicKeyInner,
}

impl PublicKey {
    /// Verify a signature for a message using this public key, i.e. check
    /// that the signature has been produced by the corresponding
    /// private key (authenticity), and that the message has not been
    /// tampered with (integrity).
    #[must_use]
    #[allow(unused_variables)]
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        match self.publickey {
            #[cfg(feature = "ed25519")]
            PublicKeyInner::Ed25519(ref pk) => pk.verify(msg, sig),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            PublicKeyInner::Rsa(ref pk) => pk.verify(msg, sig),
            #[cfg(feature = "secp256k1")]
            PublicKeyInner::Secp256k1(ref pk) => pk.verify(msg, sig),
            #[cfg(feature = "ecdsa")]
            PublicKeyInner::Ecdsa(ref pk) => pk.verify(msg, sig),
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
    pub fn encode_protobuf(&self) -> Vec<u8> {
        #[cfg(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        ))]
        {
            use quick_protobuf::MessageWrite;
            let public_key = proto::PublicKey::from(self);

            let mut buf = Vec::with_capacity(public_key.get_size());
            let mut writer = Writer::new(&mut buf);
            public_key
                .write_message(&mut writer)
                .expect("Encoding to succeed");

            buf
        }

        #[cfg(not(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        )))]
        unreachable!()
    }

    /// Decode a public key from a protobuf structure, e.g. read from storage
    /// or received from another node.
    #[allow(unused_variables)]
    pub fn try_decode_protobuf(bytes: &[u8]) -> Result<PublicKey, DecodingError> {
        #[cfg(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        ))]
        {
            use quick_protobuf::MessageRead;
            let mut reader = BytesReader::from_bytes(bytes);

            let pubkey = proto::PublicKey::from_reader(&mut reader, bytes)
                .map_err(|e| DecodingError::bad_protobuf("public key bytes", e))?;

            pubkey.try_into()
        }

        #[cfg(not(any(
            feature = "ecdsa",
            feature = "secp256k1",
            feature = "ed25519",
            feature = "rsa"
        )))]
        unreachable!()
    }

    /// Convert the `PublicKey` into the corresponding `PeerId`.
    #[cfg(feature = "peerid")]
    pub fn to_peer_id(&self) -> crate::PeerId {
        self.into()
    }

    /// Return a [`KeyType`] of the [`PublicKey`].
    pub fn key_type(&self) -> KeyType {
        match self.publickey {
            #[cfg(feature = "ed25519")]
            PublicKeyInner::Ed25519(_) => KeyType::Ed25519,
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            PublicKeyInner::Rsa(_) => KeyType::RSA,
            #[cfg(feature = "secp256k1")]
            PublicKeyInner::Secp256k1(_) => KeyType::Secp256k1,
            #[cfg(feature = "ecdsa")]
            PublicKeyInner::Ecdsa(_) => KeyType::Ecdsa,
        }
    }
}

#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
impl TryFrom<proto::PublicKey> for PublicKey {
    type Error = DecodingError;

    fn try_from(pubkey: proto::PublicKey) -> Result<Self, Self::Error> {
        match pubkey.Type {
            #[cfg(feature = "ed25519")]
            proto::KeyType::Ed25519 => Ok(ed25519::PublicKey::try_from_bytes(&pubkey.Data).map(
                |kp| PublicKey {
                    publickey: PublicKeyInner::Ed25519(kp),
                },
            )?),
            #[cfg(not(feature = "ed25519"))]
            proto::KeyType::Ed25519 => {
                tracing::debug!("support for ed25519 was disabled at compile-time");
                Err(DecodingError::missing_feature("ed25519"))
            }
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            proto::KeyType::RSA => {
                Ok(
                    rsa::PublicKey::try_decode_x509(&pubkey.Data).map(|kp| PublicKey {
                        publickey: PublicKeyInner::Rsa(kp),
                    })?,
                )
            }
            #[cfg(any(not(feature = "rsa"), target_arch = "wasm32"))]
            proto::KeyType::RSA => {
                tracing::debug!("support for RSA was disabled at compile-time");
                Err(DecodingError::missing_feature("rsa"))
            }
            #[cfg(feature = "secp256k1")]
            proto::KeyType::Secp256k1 => Ok(secp256k1::PublicKey::try_from_bytes(&pubkey.Data)
                .map(|kp| PublicKey {
                    publickey: PublicKeyInner::Secp256k1(kp),
                })?),
            #[cfg(not(feature = "secp256k1"))]
            proto::KeyType::Secp256k1 => {
                tracing::debug!("support for secp256k1 was disabled at compile-time");
                Err(DecodingError::missing_feature("secp256k1"))
            }
            #[cfg(feature = "ecdsa")]
            proto::KeyType::ECDSA => Ok(ecdsa::PublicKey::try_decode_der(&pubkey.Data).map(
                |kp| PublicKey {
                    publickey: PublicKeyInner::Ecdsa(kp),
                },
            )?),
            #[cfg(not(feature = "ecdsa"))]
            proto::KeyType::ECDSA => {
                tracing::debug!("support for ECDSA was disabled at compile-time");
                Err(DecodingError::missing_feature("ecdsa"))
            }
        }
    }
}

#[cfg(feature = "ed25519")]
impl TryInto<ed25519::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ed25519::PublicKey, Self::Error> {
        match self.publickey {
            PublicKeyInner::Ed25519(inner) => Ok(inner),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            PublicKeyInner::Rsa(_) => Err(OtherVariantError::new(crate::KeyType::RSA)),
            #[cfg(feature = "secp256k1")]
            PublicKeyInner::Secp256k1(_) => Err(OtherVariantError::new(crate::KeyType::Secp256k1)),
            #[cfg(feature = "ecdsa")]
            PublicKeyInner::Ecdsa(_) => Err(OtherVariantError::new(crate::KeyType::Ecdsa)),
        }
    }
}

#[cfg(feature = "ecdsa")]
impl TryInto<ecdsa::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<ecdsa::PublicKey, Self::Error> {
        match self.publickey {
            PublicKeyInner::Ecdsa(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            PublicKeyInner::Ed25519(_) => Err(OtherVariantError::new(crate::KeyType::Ed25519)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            PublicKeyInner::Rsa(_) => Err(OtherVariantError::new(crate::KeyType::RSA)),
            #[cfg(feature = "secp256k1")]
            PublicKeyInner::Secp256k1(_) => Err(OtherVariantError::new(crate::KeyType::Secp256k1)),
        }
    }
}

#[cfg(feature = "secp256k1")]
impl TryInto<secp256k1::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<secp256k1::PublicKey, Self::Error> {
        match self.publickey {
            PublicKeyInner::Secp256k1(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            PublicKeyInner::Ed25519(_) => Err(OtherVariantError::new(crate::KeyType::Ed25519)),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            PublicKeyInner::Rsa(_) => Err(OtherVariantError::new(crate::KeyType::RSA)),
            #[cfg(feature = "ecdsa")]
            PublicKeyInner::Ecdsa(_) => Err(OtherVariantError::new(crate::KeyType::Ecdsa)),
        }
    }
}

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
impl TryInto<rsa::PublicKey> for PublicKey {
    type Error = OtherVariantError;

    fn try_into(self) -> Result<rsa::PublicKey, Self::Error> {
        match self.publickey {
            PublicKeyInner::Rsa(inner) => Ok(inner),
            #[cfg(feature = "ed25519")]
            PublicKeyInner::Ed25519(_) => Err(OtherVariantError::new(crate::KeyType::Ed25519)),
            #[cfg(feature = "secp256k1")]
            PublicKeyInner::Secp256k1(_) => Err(OtherVariantError::new(crate::KeyType::Secp256k1)),
            #[cfg(feature = "ecdsa")]
            PublicKeyInner::Ecdsa(_) => Err(OtherVariantError::new(crate::KeyType::Ecdsa)),
        }
    }
}

#[cfg(feature = "ed25519")]
impl From<ed25519::PublicKey> for PublicKey {
    fn from(key: ed25519::PublicKey) -> Self {
        PublicKey {
            publickey: PublicKeyInner::Ed25519(key),
        }
    }
}

#[cfg(feature = "secp256k1")]
impl From<secp256k1::PublicKey> for PublicKey {
    fn from(key: secp256k1::PublicKey) -> Self {
        PublicKey {
            publickey: PublicKeyInner::Secp256k1(key),
        }
    }
}

#[cfg(feature = "ecdsa")]
impl From<ecdsa::PublicKey> for PublicKey {
    fn from(key: ecdsa::PublicKey) -> Self {
        PublicKey {
            publickey: PublicKeyInner::Ecdsa(key),
        }
    }
}

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
impl From<rsa::PublicKey> for PublicKey {
    fn from(key: rsa::PublicKey) -> Self {
        PublicKey {
            publickey: PublicKeyInner::Rsa(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "ed25519")]
    #[cfg(feature = "peerid")]
    fn keypair_protobuf_roundtrip_ed25519() {

        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!(
            "080112407e0830617c4a7de83925dfb2694556b12936c477a0e1feb2e148ec9da60fee7d1ed1e8fae2c4a144b8be8fd4b47bf3d3b34b871c3cacf6010f0e42d474fce27e"
        ))
            .unwrap();

        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!(
            "080112201ed1e8fae2c4a144b8be8fd4b47bf3d3b34b871c3cacf6010f0e42d474fce27e"
        ))
        .unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key, KeyType::Ed25519);
    }

    #[test]
    #[cfg(all(feature = "ecdsa", feature = "peerid"))]
    fn keypair_protobuf_roundtrip_ecdsa() {
        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!(
            "08031279307702010104203E5B1FE9712E6C314942A750BD67485DE3C1EFE85B1BFB520AE8F9AE3DFA4A4CA00A06082A8648CE3D030107A14403420004DE3D300FA36AE0E8F5D530899D83ABAB44ABF3161F162A4BC901D8E6ECDA020E8B6D5F8DA30525E71D6851510C098E5C47C646A597FB4DCEC034E9F77C409E62"
        ))
        .unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!("0803125b3059301306072a8648ce3d020106082a8648ce3d03010703420004de3d300fa36ae0e8f5d530899d83abab44abf3161f162a4bc901d8e6ecda020e8b6d5f8da30525e71d6851510c098e5c47c646a597fb4dcec034e9f77c409e62")).unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key, KeyType::Ecdsa);
    }

    // #[test]
    // #[cfg(feature = "rsa")]
    // fn keypair_protobuf_roundtrip_rsa() {
    //     let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!("080012c81230820944020100300d06092a864886f70d01010105000482092e3082092a02010002820201009c897f33e0d0b3297f2fe404ea5b7a98096b329693292aefc2d05ef1e82fd0e121ce74ec77d75ef4b532fa34dee2a19626f3389c6d2bb9b8de614e138302bc4254727a7ee35f7827f1094403bc2fe8e1f64d0e8a2a77e8f3a879f69f94a71f3589de184f5910d6b5270f58e684f71ddd3a3f486a4cb2c390194ee6e9b65f9f1dff7b8f6c0bf4e0c4ac683bd4ba2d2fd022fdaaa3db75e90e16662fc4b3aca4c9aa65514d51690cd372c2b96c61a1ed4f9298ec213d5398aa9120379477118391104deb77ab157a59b70714e95caa9b55d15fa386b0c80f36e50d738bdd10e0baa3c3eafb4703dec3d6a757601f18541eb87ae9111f60eae17d843cf1047dbf5a8982ad9ef0aa88f59b17689f1210a305f7da8a012c1a58e4e82b48811618e98cef13c9eb28ce6fcc589ea5d902149ee4f49f8b39758b349ca90be5a8bddf4a46bacaaa48aec1c0c6e996ab13f2cb351c351d40b0a7b8e0c12b366a8555c392b0aadf71fe746eb4f8ea0b829da6ddcc39081abdd40ea2f3d8778b9a3f06a480ef34234975e919c0d64d818f2e904a9f251c8669dbb1666cb2c28e955446fc7efd460d4677ed922ccff1e24bb5a8699e050075c7897a64daa1bc2f05e4132e76c4f72baea5d073042254236c116ea3e40540bb7986468b4468aadfadad068331ef9dbe13e4012196e8eb9f8cdba096c35f09e80893ea68f3253dc41053983855e50203010001028202002699dd6d4c960a68443dea0bb04308b32f37690d2a92ef4c9a8cc9acfba5b6eb9d6b8cf7b701bc1fba032d2216886a725d7e82ca483d8d19e274ba4d23746c3a2b1ae3cc2083ad5ca41ab5d3f9f712858e38284ab7f843d0ba0e015c0ecb3b6df766763632ef6d12d4e3faf73578bebb8c1e88dbf5b7eb73c059eda55a5cb01f349e229af143dc9d832a5cfeb33e6b58f717f8995987f5058d4e7b9f14f390db4e1297feea016eb141ce74ed1e125133db21acb0f1af88a91f0a83ca2fa678fc2fba1743b643a09d38fe1d1102d1eb6639304d61ec7c190c5f6576c5d9a8ccd2198a398ae75333feb51324ffc60b38cb2e90d8a2694b7c0048f47016bb15cb36c482e038e455254e35fc4f0e0babc84e046bd441b0291412c784e4e9639664cad07cb09a01626049cdbfd1d9ad75b314448df811f4988c6e64d93ebefc602b574d0763e31e9d567c891349cfe75f0ca37429b743d6452d1fffc1f9f4901e5f68772b4f24542d654fd29b893e44c85e6037bba304d48873721131f18248b16bd71384abd00f9336c73f071a4ca2456878070f9704ed7df0cd64e5c3e5949a78968525865b96e71d5015dc68bff857f2bba05a3976d83d8866d4dfe8caac144741ae97879a765dc0d4c7c34aa79ef6ebc86b5bf32b50ad995780f5f1a6c052eec5671164f407061a9c6bd49251b1bb7803bb222f5d859c321601236dd893dc9d810282010100cf13fe9908fe59e947122d5606cf9f70c123b7cb43a1916463e729f01dc31c3b70cb6a37bde542ecdc6029cea39b28c99c6395d0aaa29c1c4cf14b3fed9e0fcd793e31b7a09930352261c03b3dc0b66a62f8ae3771b705382cfeb6130d4a7e5b4854117a05767b99915099e2d542fc3fa505a0dbe217b169b46714384774380408bd8b3dbf0c9a177bbd3e64af115988159f485d70c885171007646765b50eb9bbebfabe60e71c69b2b822a124e235ad05f2b55cda9ddc78d671436981a3064a80c29bb37e6b5581a9372a6366c79af695a39ea0f3839ed77ec3985252f2e126955774727955b63ccbeff64208fd7280e8ba52e4297cb6bf72b44b07618923610282010100c184cd27d3a643df768764a7c66de40c222bdb4b7e02c35aa1e4a8377676247c629df58ecb5bb541fb4aac1bde35057b0b266bddd818876909b8fff1aca4859515069258d84b0c5178e4bff6842c68d39cad9a3a03aa6533fa76b92c995f381eb9c83f5e6118fd962807c931b7ca50dc20b261f2a71928f3e882af4da979cef843970cb2af68b86477b92ca90c8c0f1d640d39e943704366314c446f7a54851419e60f4e92e1e69bd52ee7294f9eddc6dc873144b0d0d9f13eb8d6aa955cf11edbd5a0673d8b70ef937e54fdaade185facc8437496d43a53169342280718a3679170ef4a0e582af4db598210fb64616f0d8daa08519d875e37c4d02e1af1c5050282010100c14865648c3b74cac3b698b06a4d130218947131fd9f69e8ed42d0273a706a02a546888f1ce547f173c52260a8dee354436fc45f6f55b626c83e94c147d637e3cede1963cf380d021b64681c2388a3fb6b03b9013157e63c47eb3b214f4f8fdf3e04920775dfe080375da7354d5f67b9341babc87121324c7ac197e2ebf6f36df8868ad8086207d6117e5325812ecd85b2c0e8b7a6d4d33cf28e23ce4ae593a8135ab0c1500b87beb4bd203d8f02c19d0d273cd73d8b094594cb4563ce47cf506d1cb85df28ad6d5de8f0a369bb185d7d1565672deb8a4e37983b1c26d801c5d7a19962c5f4a7c7e04d0a6e77e22aae4ddd54417890dca39aa23d4c03feed4210282010100915975de1c121d9892264f6bd496655ad7afa91ea29ee0ac0a3cfc3bec3600618c90a80780a67915fdf0b0249e59a4ac2e4bc568f30e3966a36ed88e64e58d8fd4230378c7bc569c3af955558b20effb410b0373df9cf4367e40fe04898e0350d0a99f2efc2f1108df3839dda5f5c7960ed8ecc89cc9410131fa364156b1aecab9b992480387dc3759d533be25366d83ddca315d0ad21f4d7a69965d44bc86d7fa3bd9f3624f5a2e6188c1073e4e4cb5389e325b2d93309f0a453ab71548a1b253dbb886d2ab114060bfda864cf853c648b88231e7b7afb70895c272de219b5a06db945f4336e5ccd393ff25522cab220644091a06731361a8f1a28b7ea169210282010100bd80196d3d11a8257b5f439776388f4d53e4da3690f710e9aff3e3e970e545ec92d285e7049da000d5364dd7f550c17cf662d516282fe89813cab322ce5aad5cc744c52a024dd1a94aa9484037281637d1c8e3503b6ed6231225c93f7865d29269c899bbf5d248cf9d41f9aee9b9cb2afac172ba17c2df0699c6604b4ce7ab95c91c5f7fc7804f2bde268a7e15c512920f7325cfba47463da1c201549fc44c2bc4fbe5d8619cde9733470c5e38b996f5c3633c6311af88663ce4d2d0dc415ac5c8258e1aa7659f9f35d4b90b7b9a5a888867d75636e6443cce5391c57d48d56409029edef53e1a5130eb1fa708758bc821e15f7c53edf6d4c6f868a6b5b0c1e6")).unwrap();
    //     let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!("080012a60430820222300d06092a864886f70d01010105000382020f003082020a02820201009c897f33e0d0b3297f2fe404ea5b7a98096b329693292aefc2d05ef1e82fd0e121ce74ec77d75ef4b532fa34dee2a19626f3389c6d2bb9b8de614e138302bc4254727a7ee35f7827f1094403bc2fe8e1f64d0e8a2a77e8f3a879f69f94a71f3589de184f5910d6b5270f58e684f71ddd3a3f486a4cb2c390194ee6e9b65f9f1dff7b8f6c0bf4e0c4ac683bd4ba2d2fd022fdaaa3db75e90e16662fc4b3aca4c9aa65514d51690cd372c2b96c61a1ed4f9298ec213d5398aa9120379477118391104deb77ab157a59b70714e95caa9b55d15fa386b0c80f36e50d738bdd10e0baa3c3eafb4703dec3d6a757601f18541eb87ae9111f60eae17d843cf1047dbf5a8982ad9ef0aa88f59b17689f1210a305f7da8a012c1a58e4e82b48811618e98cef13c9eb28ce6fcc589ea5d902149ee4f49f8b39758b349ca90be5a8bddf4a46bacaaa48aec1c0c6e996ab13f2cb351c351d40b0a7b8e0c12b366a8555c392b0aadf71fe746eb4f8ea0b829da6ddcc39081abdd40ea2f3d8778b9a3f06a480ef34234975e919c0d64d818f2e904a9f251c8669dbb1666cb2c28e955446fc7efd460d4677ed922ccff1e24bb5a8699e050075c7897a64daa1bc2f05e4132e76c4f72baea5d073042254236c116ea3e40540bb7986468b4468aadfadad068331ef9dbe13e4012196e8eb9f8cdba096c35f09e80893ea68f3253dc41053983855e50203010001")).unwrap();

    //     roundtrip_protobuf_encoding(&priv_key, &pub_key);
    // }

    #[test]
    #[cfg(all(feature = "secp256k1", feature = "peerid"))]
    fn keypair_protobuf_roundtrip_secp256k1() {
        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!(
            "0802122053DADF1D5A164D6B4ACDB15E24AA4C5B1D3461BDBD42ABEDB0A4404D56CED8FB"
        ))
        .unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!(
            "08021221037777e994e452c21604f91de093ce415f5432f701dd8cd1a7a6fea0e630bfca99"
        ))
        .unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key, KeyType::Secp256k1);
    }

    #[cfg(feature = "peerid")]
    fn roundtrip_protobuf_encoding(private_key: &Keypair, public_key: &PublicKey, tpe: KeyType) {
        assert_eq!(&private_key.public(), public_key);

        let encoded_priv = private_key
            .encode_protobuf()
            .expect("key to support protobuf encoding");
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
        assert_eq!(private_key.key_type(), tpe)
    }

    #[test]
    #[cfg(feature = "peerid")]
    fn keypair_from_protobuf_encoding() {
      
        let priv_key = Keypair::try_decode_protobuf(&hex_literal::hex!(
            "080012ae123082092a0201000282020100e1beab071d08200bde24eef00d049449b07770ff9910257b2d7d5dda242ce8f0e2f12e1af4b32d9efd2c090f66b0f29986dbb645dae9880089704a94e5066d594162ae6ee8892e6ec70701db0a6c445c04778eb3de1293aa1a23c3825b85c6620a2bc3f82f9b0c309bc0ab3aeb1873282bebd3da03c33e76c21e9beb172fd44c9e43be32e2c99827033cf8d0f0c606f4579326c930eb4e854395ad941256542c793902185153c474bed109d6ff5141ebf9cd256cf58893a37f83729f97e7cb435ec679d2e33901d27bb35aa0d7e20561da08885ef0abbf8e2fb48d6a5487047a9ecb1ad41fa7ed84f6e3e8ecd5d98b3982d2a901b4454991766da295ab78822add5612a2df83bcee814cf50973e80d7ef38111b1bd87da2ae92438a2c8cbcc70b31ee319939a3b9c761dbc13b5c086d6b64bf7ae7dacc14622375d92a8ff9af7eb962162bbddebf90acb32adb5e4e4029f1c96019949ecfbfeffd7ac1e3fbcc6b6168c34be3d5a2e5999fcbb39bba7adbca78eab09b9bc39f7fa4b93411f4cc175e70c0a083e96bfaefb04a9580b4753c1738a6a760ae1afd851a1a4bdad231cf56e9284d832483df215a46c1c21bdf0c6cfe951c18f1ee4078c79c13d63edb6e14feaeffabc90ad317e4875fe648101b0864097e998f0ca3025ef9638cd2b0caecd3770ab54a1d9c6ca959b0f5dcbc90caeefc4135baca6fd475224269bbe1b02030100010282020100a472ffa858efd8588ce59ee264b957452f3673acdf5631d7bfd5ba0ef59779c231b0bc838a8b14cae367b6d9ef572c03c7883b0a3c652f5c24c316b1ccfd979f13d0cd7da20c7d34d9ec32dfdc81ee7292167e706d705efde5b8f3edfcba41409e642f8897357df5d320d21c43b33600a7ae4e505db957c1afbc189d73f0b5d972d9aaaeeb232ca20eebd5de6fe7f29d01470354413cc9a0af1154b7af7c1029adcd67c74b4798afeb69e09f2cb387305e73a1b5f450202d54f0ef096fe1bde340219a1194d1ac9026e90b366cce0c59b239d10e4888f52ca1780824d39ae01a6b9f4dd6059191a7f12b2a3d8db3c2868cd4e5a5862b8b625a4197d52c6ac77710116ebd3ced81c4d91ad5fdfbed68312ebce7eea45c1833ca3acf7da2052820eacf5c6b07d086dabeb893391c71417fd8a4b1829ae2cf60d1749d0e25da19530d889461c21da3492a8dc6ccac7de83ac1c2185262c7473c8cc42f547cc9864b02a8073b6aa54a037d8c0de3914784e6205e83d97918b944f11b877b12084c0dd1d36592f8a4f8b8da5bb404c3d2c079b22b6ceabfbcb637c0dbe0201f0909d533f8bf308ada47aee641a012a494d31b54c974e58b87f140258258bb82f31692659db7aa07e17a5b2a0832c24e122d3a8babcc9ee74cbb07d3058bb85b15f6f6b2674aba9fd34367be9782d444335fbed31e3c4086c652597c27104938b47fa10282010100e9fdf843c1550070ca711cb8ff28411466198f0e212511c3186623890c0071bf6561219682fe7dbdfd81176eba7c4faba21614a20721e0fcd63768e6d925688ecc90992059ac89256e0524de90bf3d8a052ce6a9f6adafa712f3107a016e20c80255c9e37d8206d1bc327e06e66eb24288da866b55904fd8b59e6b2ab31bc5eab47e597093c63fab7872102d57b4c589c66077f534a61f5f65127459a33c91f6db61fc431b1ae90be92b4149a3255291baf94304e3efb77b1107b5a3bda911359c40a53c347ff9100baf8f36dc5cd991066b5bdc28b39ed644f404afe9213f4d31c9d4e40f3a5f5e3c39bebeb244e84137544e1a1839c1c8aaebf0c78a7fad590282010100f6fa1f1e6b803742d5490b7441152f500970f46feb0b73a6e4baba2aaf3c0e245ed852fc31d86a8e46eb48e90fac409989dfee45238f97e8f1f8e83a136488c1b04b8a7fb695f37b8616307ff8a8d63e8cfa0b4fb9b9167ffaebabf111aa5a4344afbabd002ae8961c38c02da76a9149abdde93eb389eb32595c29ba30d8283a7885218a5a9d33f7f01dbdf85f3aad016c071395491338ec318d39220e1c7bd69d3d6b520a13a30d745c102b827ad9984b0dd6aed73916ffa82a06c1c111e7047dcd2668f988a0570a71474992eecf416e068f029ec323d5d635fd24694fc9bf96973c255d26c772a95bf8b7f876547a5beabf86f06cd21b67994f944e7a5493028201010095b02fd30069e547426a8bea58e8a2816f33688dac6c6f6974415af8402244a22133baedf34ce499d7036f3f19b38eb00897c18949b0c5a25953c71aeeccfc8f6594173157cc854bd98f16dffe8f28ca13b77eb43a2730585c49fc3f608cd811bb54b03b84bddaa8ef910988567f783012266199667a546a18fd88271fbf63a45ae4fd4884706da8befb9117c0a4d73de5172f8640b1091ed8a4aea3ed4641463f5ff6a5e3401ad7d0c92811f87956d1fd5f9a1d15c7f3839a08698d9f35f9d966e5000f7cb2655d7b6c4adcd8a9d950ea5f61bb7c9a33c17508f9baa313eecfee4ae493249ebe05a5d7770bbd3551b2eeb752e3649e0636de08e3d672e66cb90282010100ad93e4c31072b063fc5ab5fe22afacece775c795d0efdf7c704cfc027bde0d626a7646fc905bb5a80117e3ca49059af14e0160089f9190065be9bfecf12c3b2145b211c8e89e42dd91c38e9aa23ca73697063564f6f6aa6590088a738722df056004d18d7bccac62b3bafef6172fc2a4b071ea37f31eff7a076bcab7dd144e51a9da8754219352aef2c73478971539fa41de4759285ea626fa3c72e7085be47d554d915bbb5149cb6ef835351f231043049cd941506a034bf2f8767f3e1e42ead92f91cb3d75549b57ef7d56ac39c2d80d67f6a2b4ca192974bfc5060e2dd171217971002193dba12e7e4133ab201f07500a90495a38610279b13a48d54f0c99028201003e3a1ac0c2b67d54ed5c4bbe04a7db99103659d33a4f9d35809e1f60c282e5988dddc964527f3b05e6cc890eab3dcb571d66debf3a5527704c87264b3954d7265f4e8d2c637dd89b491b9cf23f264801f804b90454d65af0c4c830d1aef76f597ef61b26ca857ecce9cb78d4f6c2218c00d2975d46c2b013fbf59b750c3b92d8d3ed9e6d1fd0ef1ec091a5c286a3fe2dead292f40f380065731e2079ebb9f2a7ef2c415ecbb488da98f3a12609ca1b6ec8c734032c8bd513292ff842c375d4acd1b02dfb206b24cd815f8e2f9d4af8e7dea0370b19c1b23cc531d78b40e06e1119ee2e08f6f31c6e2e8444c568d13c5d451a291ae0c9f1d4f27d23b3a00d60ad"
        ))
            .unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!(
            "080012a60430820222300d06092a864886f70d01010105000382020f003082020a0282020100e1beab071d08200bde24eef00d049449b07770ff9910257b2d7d5dda242ce8f0e2f12e1af4b32d9efd2c090f66b0f29986dbb645dae9880089704a94e5066d594162ae6ee8892e6ec70701db0a6c445c04778eb3de1293aa1a23c3825b85c6620a2bc3f82f9b0c309bc0ab3aeb1873282bebd3da03c33e76c21e9beb172fd44c9e43be32e2c99827033cf8d0f0c606f4579326c930eb4e854395ad941256542c793902185153c474bed109d6ff5141ebf9cd256cf58893a37f83729f97e7cb435ec679d2e33901d27bb35aa0d7e20561da08885ef0abbf8e2fb48d6a5487047a9ecb1ad41fa7ed84f6e3e8ecd5d98b3982d2a901b4454991766da295ab78822add5612a2df83bcee814cf50973e80d7ef38111b1bd87da2ae92438a2c8cbcc70b31ee319939a3b9c761dbc13b5c086d6b64bf7ae7dacc14622375d92a8ff9af7eb962162bbddebf90acb32adb5e4e4029f1c96019949ecfbfeffd7ac1e3fbcc6b6168c34be3d5a2e5999fcbb39bba7adbca78eab09b9bc39f7fa4b93411f4cc175e70c0a083e96bfaefb04a9580b4753c1738a6a760ae1afd851a1a4bdad231cf56e9284d832483df215a46c1c21bdf0c6cfe951c18f1ee4078c79c13d63edb6e14feaeffabc90ad317e4875fe648101b0864097e998f0ca3025ef9638cd2b0caecd3770ab54a1d9c6ca959b0f5dcbc90caeefc4135baca6fd475224269bbe1b0203010001"
        ))
            .unwrap();

        assert_eq!(priv_key.public(), pub_key);
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

    #[test]
    #[cfg(all(feature = "ed25519", feature = "rand"))]
    fn test_publickey_from_ed25519_public_key() {
        let pubkey = Keypair::generate_ed25519().public();
        let ed25519_pubkey = pubkey
            .clone()
            .try_into_ed25519()
            .expect("A ed25519 keypair");

        let converted_pubkey = PublicKey::from(ed25519_pubkey);

        assert_eq!(converted_pubkey, pubkey);
        assert_eq!(converted_pubkey.key_type(), KeyType::Ed25519)
    }

    #[test]
    #[cfg(all(feature = "secp256k1", feature = "rand"))]
    fn test_publickey_from_secp256k1_public_key() {
        let pubkey = Keypair::generate_secp256k1().public();
        let secp256k1_pubkey = pubkey
            .clone()
            .try_into_secp256k1()
            .expect("A secp256k1 keypair");
        let converted_pubkey = PublicKey::from(secp256k1_pubkey);

        assert_eq!(converted_pubkey, pubkey);
        assert_eq!(converted_pubkey.key_type(), KeyType::Secp256k1)
    }

    #[test]
    #[cfg(all(feature = "ecdsa", feature = "rand"))]
    fn test_publickey_from_ecdsa_public_key() {
        let pubkey = Keypair::generate_ecdsa().public();
        let ecdsa_pubkey = pubkey.clone().try_into_ecdsa().expect("A ecdsa keypair");
        let converted_pubkey = PublicKey::from(ecdsa_pubkey);

        assert_eq!(converted_pubkey, pubkey);
        assert_eq!(converted_pubkey.key_type(), KeyType::Ecdsa)
    }

    #[test]
    #[cfg(feature = "ecdsa")]
    fn test_secret_from_ecdsa_private_key() {
        let keypair = Keypair::generate_ecdsa();
        assert!(keypair.derive_secret(b"domain separator!").is_some())
    }
}
