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

use asn1_der::{FromDerObject, DerObject};
use lazy_static::lazy_static;
use sha2::{Digest as ShaDigestTrait, Sha256};
use secp256k1 as secp;
use secp::{Message, Signature};
use super::error::DecodingError;
use zeroize::Zeroize;

// Cached `Secp256k1` context, to avoid recreating it every time.
lazy_static! {
    static ref SECP: secp::Secp256k1<secp::All> = secp::Secp256k1::new();
}

/// A Secp256k1 keypair.
#[derive(Clone)]
pub struct Keypair {
    secret: SecretKey,
    public: PublicKey
}

impl Keypair {
    /// Generate a new sec256k1 `Keypair`.
    pub fn generate() -> Keypair {
        Keypair::from(SecretKey::generate())
    }

    /// Get the public key of this keypair.
    pub fn public(&self) -> &PublicKey {
        &self.public
    }

    /// Get the secret key of this keypair.
    pub fn secret(&self) -> &SecretKey {
        &self.secret
    }
}

/// Promote a Secp256k1 secret key into a keypair.
impl From<SecretKey> for Keypair {
    fn from(secret: SecretKey) -> Keypair {
        let public = PublicKey(secp::key::PublicKey::from_secret_key(&SECP, &secret.0));
        Keypair { secret, public }
    }
}

/// Demote a Secp256k1 keypair into a secret key.
impl From<Keypair> for SecretKey {
    fn from(kp: Keypair) -> SecretKey {
        kp.secret
    }
}

/// A Secp256k1 secret key.
#[derive(Clone)]
pub struct SecretKey(secp::key::SecretKey);

/// View the bytes of the secret key.
impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl SecretKey {
    /// Generate a new Secp256k1 secret key.
    pub fn generate() -> SecretKey {
        SecretKey(secp::key::SecretKey::new(&mut secp::rand::thread_rng()))
    }

    /// Create a secret key from a byte slice, zeroing the slice on success.
    /// If the bytes do not constitute a valid Secp256k1 secret key, an
    /// error is returned.
    pub fn from_bytes(mut sk: impl AsMut<[u8]>) -> Result<SecretKey, DecodingError> {
        let sk_bytes = sk.as_mut();
        let secret = secp::key::SecretKey::from_slice(&*sk_bytes)
            .map_err(|e| DecodingError::new("Secp256k1 secret key", e))?;
        Ok(SecretKey(secret))
    }

    /// Decode a DER-encoded Secp256k1 secret key in an ECPrivateKey
    /// structure as defined in [RFC5915].
    ///
    /// [RFC5915]: https://tools.ietf.org/html/rfc5915
    pub fn from_der(mut der: impl AsMut<[u8]>) -> Result<SecretKey, DecodingError> {
        // TODO: Stricter parsing.
        let der_obj = der.as_mut();
        let obj: Vec<DerObject> = FromDerObject::deserialize((&*der_obj).iter())
            .map_err(|e| DecodingError::new("Secp256k1 DER ECPrivateKey", e))?;
        der_obj.zeroize();
        let sk_obj = obj.into_iter().nth(1)
            .ok_or_else(|| "Not enough elements in DER".to_string())?;
        let mut sk_bytes: Vec<u8> = FromDerObject::from_der_object(sk_obj)
            .map_err(|e| e.to_string())?;
        let sk = SecretKey::from_bytes(&mut sk_bytes)?;
        sk_bytes.zeroize();
        Ok(sk)
    }

    /// Sign a message with this secret key, producing a DER-encoded
    /// ECDSA signature, as defined in [RFC3278].
    ///
    /// [RFC3278]: https://tools.ietf.org/html/rfc3278#section-8.2
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let m = Message::from_slice(Sha256::digest(&msg).as_ref())
            .expect("digest output length doesn't match secp256k1 input length");
        SECP.sign(&m, &self.0).serialize_der()
    }
}

/// A Secp256k1 public key.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct PublicKey(secp::key::PublicKey);

impl PublicKey {
    /// Verify the Secp256k1 signature on a message using the public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        Message::from_slice(&Sha256::digest(msg))
            .and_then(|m| Signature::from_der(sig)
                .and_then(|s| SECP.verify(&m, &s, &self.0))).is_ok()
    }

    /// Encode the public key in compressed form, i.e. with one coordinate
    /// represented by a single bit.
    pub fn encode(&self) -> [u8; 33] {
        self.0.serialize()
    }

    /// Decode a public key from a byte slice in the the format produced
    /// by `encode`.
    pub fn decode(k: &[u8]) -> Result<PublicKey, DecodingError> {
        secp256k1::PublicKey::from_slice(k)
            .map_err(|e| DecodingError::new("Secp256k1 public key", e))
            .map(PublicKey)
    }
}

