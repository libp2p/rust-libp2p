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
    #[cfg(feature = "ed25519")]
    pub fn generate_ed25519() -> Keypair {
        Keypair {
            keypair: KeyPairInner::Ed25519(ed25519::Keypair::generate()),
        }
    }

    /// Generate a new Secp256k1 keypair.
    #[cfg(feature = "secp256k1")]
    pub fn generate_secp256k1() -> Keypair {
        Keypair {
            keypair: KeyPairInner::Secp256k1(secp256k1::Keypair::generate()),
        }
    }

    /// Generate a new ECDSA keypair.
    #[cfg(feature = "ecdsa")]
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
    pub fn secp256k1_from_der(der: &mut [u8]) -> Result<Keypair, DecodingError> {
        secp256k1::SecretKey::from_der(der).map(|sk| Keypair {
            keypair: KeyPairInner::Secp256k1(secp256k1::Keypair::from(sk)),
        })
    }

    #[cfg(feature = "ed25519")]
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
    pub fn to_protobuf_encoding(&self) -> Result<Vec<u8>, DecodingError> {
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
    #[allow(unused_variables)]
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Keypair, DecodingError> {
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
                log::debug!("support for ed25519 was disabled at compile-time");
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
                log::debug!("support for RSA was disabled at compile-time");
                Err(DecodingError::missing_feature("rsa"))
            }
            #[cfg(feature = "secp256k1")]
            proto::KeyType::Secp256k1 => Ok(secp256k1::PublicKey::try_from_bytes(&pubkey.Data)
                .map(|kp| PublicKey {
                    publickey: PublicKeyInner::Secp256k1(kp),
                })?),
            #[cfg(not(feature = "secp256k1"))]
            proto::KeyType::Secp256k1 => {
                log::debug!("support for secp256k1 was disabled at compile-time");
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
    #[cfg(feature = "peerid")]
    use crate::PeerId;
    use base64::prelude::*;
    use std::str::FromStr;

    #[test]
    #[cfg(feature = "ed25519")]
    #[cfg(feature = "peerid")]
    fn keypair_protobuf_roundtrip() {
        let expected_keypair = Keypair::generate_ed25519();
        let expected_peer_id = expected_keypair.public().to_peer_id();

        let encoded = expected_keypair.to_protobuf_encoding().unwrap();

        let keypair = Keypair::from_protobuf_encoding(&encoded).unwrap();
        let peer_id = keypair.public().to_peer_id();

        assert_eq!(expected_peer_id, peer_id);
    }

    #[test]
    #[cfg(all(feature = "ecdsa", feature = "peerid"))]
    fn keypair_protobuf_roundtrip_ecdsa() {
        let priv_key = Keypair::from_protobuf_encoding(&hex_literal::hex!(
            "08031279307702010104203E5B1FE9712E6C314942A750BD67485DE3C1EFE85B1BFB520AE8F9AE3DFA4A4CA00A06082A8648CE3D030107A14403420004DE3D300FA36AE0E8F5D530899D83ABAB44ABF3161F162A4BC901D8E6ECDA020E8B6D5F8DA30525E71D6851510C098E5C47C646A597FB4DCEC034E9F77C409E62"
        ))
        .unwrap();
        let pub_key = PublicKey::try_decode_protobuf(&hex_literal::hex!("0803125b3059301306072a8648ce3d020106082a8648ce3d03010703420004de3d300fa36ae0e8f5d530899d83abab44abf3161f162a4bc901d8e6ecda020e8b6d5f8da30525e71d6851510c098e5c47c646a597fb4dcec034e9f77c409e62")).unwrap();

        roundtrip_protobuf_encoding(&priv_key, &pub_key, KeyType::Ecdsa);
    }

    #[test]
    #[cfg(all(feature = "secp256k1", feature = "peerid"))]
    fn keypair_protobuf_roundtrip_secp256k1() {
        let priv_key = Keypair::from_protobuf_encoding(&hex_literal::hex!(
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

        let encoded_priv = private_key.to_protobuf_encoding().unwrap();
        let decoded_priv = Keypair::from_protobuf_encoding(&encoded_priv).unwrap();

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
        for (base_64_encoded_keypair, expected_peer_id) in [
            #[cfg(feature = "ed25519")]
            ("CAESQL6vdKQuznQosTrW7FWI9At+XX7EBf0BnZLhb6w+N+XSQSdfInl6c7U4NuxXJlhKcRBlBw9d0tj2dfBIVf6mcPA=", "12D3KooWEChVMMMzV8acJ53mJHrw1pQ27UAGkCxWXLJutbeUMvVu"),
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            ("CAASrhIwggkqAgEAAoICAQDhvqsHHQggC94k7vANBJRJsHdw/5kQJXstfV3aJCzo8OLxLhr0sy2e/SwJD2aw8pmG27ZF2umIAIlwSpTlBm1ZQWKubuiJLm7HBwHbCmxEXAR3jrPeEpOqGiPDgluFxmIKK8P4L5sMMJvAqzrrGHMoK+vT2gPDPnbCHpvrFy/UTJ5DvjLiyZgnAzz40PDGBvRXkybJMOtOhUOVrZQSVlQseTkCGFFTxHS+0QnW/1FB6/nNJWz1iJOjf4Nyn5fny0NexnnS4zkB0nuzWqDX4gVh2giIXvCrv44vtI1qVIcEep7LGtQfp+2E9uPo7NXZizmC0qkBtEVJkXZtopWreIIq3VYSot+DvO6BTPUJc+gNfvOBEbG9h9oq6SQ4osjLzHCzHuMZk5o7nHYdvBO1wIbWtkv3rn2swUYiN12SqP+a9+uWIWK73ev5CssyrbXk5AKfHJYBmUns+/7/16weP7zGthaMNL49Wi5Zmfy7ObunrbynjqsJubw59/pLk0EfTMF15wwKCD6Wv677BKlYC0dTwXOKanYK4a/YUaGkva0jHPVukoTYMkg98hWkbBwhvfDGz+lRwY8e5AeMecE9Y+224U/q7/q8kK0xfkh1/mSBAbCGQJfpmPDKMCXvljjNKwyuzTdwq1Sh2cbKlZsPXcvJDK7vxBNbrKb9R1IkJpu+GwIDAQABAoICAQCkcv+oWO/YWIzlnuJkuVdFLzZzrN9WMde/1boO9Zd5wjGwvIOKixTK42e22e9XLAPHiDsKPGUvXCTDFrHM/ZefE9DNfaIMfTTZ7DLf3IHucpIWfnBtcF795bjz7fy6QUCeZC+IlzV99dMg0hxDszYAp65OUF25V8GvvBidc/C12XLZqq7rIyyiDuvV3m/n8p0BRwNUQTzJoK8RVLevfBAprc1nx0tHmK/raeCfLLOHMF5zobX0UCAtVPDvCW/hveNAIZoRlNGskCbpCzZszgxZsjnRDkiI9SyheAgk05rgGmufTdYFkZGn8SsqPY2zwoaM1OWlhiuLYlpBl9Usasd3EBFuvTztgcTZGtX9++1oMS685+6kXBgzyjrPfaIFKCDqz1xrB9CG2r64kzkccUF/2KSxgpriz2DRdJ0OJdoZUw2IlGHCHaNJKo3GzKx96DrBwhhSYsdHPIzEL1R8yYZLAqgHO2qlSgN9jA3jkUeE5iBeg9l5GLlE8RuHexIITA3R02WS+KT4uNpbtATD0sB5sits6r+8tjfA2+AgHwkJ1TP4vzCK2keu5kGgEqSU0xtUyXTli4fxQCWCWLuC8xaSZZ23qgfhelsqCDLCThItOourzJ7nTLsH0wWLuFsV9vayZ0q6n9NDZ76XgtREM1++0x48QIbGUll8JxBJOLR/oQKCAQEA6f34Q8FVAHDKcRy4/yhBFGYZjw4hJRHDGGYjiQwAcb9lYSGWgv59vf2BF266fE+rohYUogch4PzWN2jm2SVojsyQmSBZrIklbgUk3pC/PYoFLOap9q2vpxLzEHoBbiDIAlXJ432CBtG8Mn4G5m6yQojahmtVkE/YtZ5rKrMbxeq0fllwk8Y/q3hyEC1XtMWJxmB39TSmH19lEnRZozyR9tth/EMbGukL6StBSaMlUpG6+UME4++3exEHtaO9qRE1nEClPDR/+RALr4823FzZkQZrW9wos57WRPQEr+khP00xydTkDzpfXjw5vr6yROhBN1ROGhg5wciq6/DHin+tWQKCAQEA9vofHmuAN0LVSQt0QRUvUAlw9G/rC3Om5Lq6Kq88DiRe2FL8MdhqjkbrSOkPrECZid/uRSOPl+jx+Og6E2SIwbBLin+2lfN7hhYwf/io1j6M+gtPubkWf/rrq/ERqlpDRK+6vQAq6JYcOMAtp2qRSavd6T6ziesyWVwpujDYKDp4hSGKWp0z9/AdvfhfOq0BbAcTlUkTOOwxjTkiDhx71p09a1IKE6MNdFwQK4J62ZhLDdau1zkW/6gqBsHBEecEfc0maPmIoFcKcUdJku7PQW4GjwKewyPV1jX9JGlPyb+WlzwlXSbHcqlb+Lf4dlR6W+q/hvBs0htnmU+UTnpUkwKCAQEAlbAv0wBp5UdCaovqWOiigW8zaI2sbG9pdEFa+EAiRKIhM7rt80zkmdcDbz8Zs46wCJfBiUmwxaJZU8ca7sz8j2WUFzFXzIVL2Y8W3/6PKMoTt360OicwWFxJ/D9gjNgRu1SwO4S92qjvkQmIVn94MBImYZlmelRqGP2IJx+/Y6Ra5P1IhHBtqL77kRfApNc95RcvhkCxCR7YpK6j7UZBRj9f9qXjQBrX0MkoEfh5VtH9X5odFcfzg5oIaY2fNfnZZuUAD3yyZV17bErc2KnZUOpfYbt8mjPBdQj5uqMT7s/uSuSTJJ6+BaXXdwu9NVGy7rdS42SeBjbeCOPWcuZsuQKCAQEArZPkwxBysGP8WrX+Iq+s7Od1x5XQ7998cEz8AnveDWJqdkb8kFu1qAEX48pJBZrxTgFgCJ+RkAZb6b/s8Sw7IUWyEcjonkLdkcOOmqI8pzaXBjVk9vaqZZAIinOHIt8FYATRjXvMrGKzuv72Fy/CpLBx6jfzHv96B2vKt90UTlGp2odUIZNSrvLHNHiXFTn6Qd5HWShepib6PHLnCFvkfVVNkVu7UUnLbvg1NR8jEEMEnNlBUGoDS/L4dn8+HkLq2S+Ryz11VJtX731WrDnC2A1n9qK0yhkpdL/FBg4t0XEheXEAIZPboS5+QTOrIB8HUAqQSVo4YQJ5sTpI1U8MmQKCAQA+OhrAwrZ9VO1cS74Ep9uZEDZZ0zpPnTWAnh9gwoLlmI3dyWRSfzsF5syJDqs9y1cdZt6/OlUncEyHJks5VNcmX06NLGN92JtJG5zyPyZIAfgEuQRU1lrwxMgw0a73b1l+9hsmyoV+zOnLeNT2wiGMANKXXUbCsBP79Zt1DDuS2NPtnm0f0O8ewJGlwoaj/i3q0pL0DzgAZXMeIHnrufKn7yxBXsu0iNqY86EmCcobbsjHNAMsi9UTKS/4QsN11KzRsC37IGskzYFfji+dSvjn3qA3CxnBsjzFMdeLQOBuERnuLgj28xxuLoRExWjRPF1FGika4Mnx1PJ9I7OgDWCt","QmaeANgBs1DTSxWSrPPtobgQuxW8XTfsS4ydbK4rCHzqxG" ),
        ] {
            let expected_peer_id =
                PeerId::from_str(expected_peer_id).unwrap();

            let protobuf_encoded_keypair = BASE64_STANDARD.decode(base_64_encoded_keypair).unwrap();

            let keypair = Keypair::from_protobuf_encoding(&protobuf_encoded_keypair).unwrap();
            let peer_id = keypair.public().to_peer_id();

            assert_eq!(expected_peer_id, peer_id);
        }
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
    #[cfg(feature = "ed25519")]
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
    #[cfg(feature = "secp256k1")]
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
    #[cfg(feature = "ecdsa")]
    fn test_publickey_from_ecdsa_public_key() {
        let pubkey = Keypair::generate_ecdsa().public();
        let ecdsa_pubkey = pubkey.clone().try_into_ecdsa().expect("A ecdsa keypair");
        let converted_pubkey = PublicKey::from(ecdsa_pubkey);

        assert_eq!(converted_pubkey, pubkey);
        assert_eq!(converted_pubkey.key_type(), KeyType::Ecdsa)
    }
}
