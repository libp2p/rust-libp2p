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

//! Sr25519 keys.

use super::error::DecodingError;
use core::fmt;
use std::fmt::{Display, Formatter};
use tari_crypto::keys::{PublicKey as _, SecretKey as _};
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey};
use tari_crypto::tari_utilities::ByteArray;
use zeroize::Zeroize;

/// An Sr25519 keypair.
#[derive(Clone)]
pub struct Keypair {
    public: PublicKey,
    secret: SecretKey,
}

impl Keypair {
    /// Generate a new random Sr25519 keypair.
    #[cfg(feature = "rand")]
    pub fn generate() -> Keypair {
        Keypair::from(SecretKey::generate())
    }

    /// Convert the keypair into a byte array. This is encoded as the secret key only since the public key can be derived from it.
    /// TODO: this leaks the secret key in stack memory
    pub fn to_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }

    /// Try to parse a keypair from the [binary format](https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.5)
    /// produced by [`Keypair::to_bytes`], zeroing the input on success.
    ///
    /// Note that this binary format is the same as `curve25519_dalek`'s compressed bytes
    pub fn try_from_bytes(kp: &mut [u8]) -> Result<Keypair, DecodingError> {
        let secret = SecretKey::try_from_bytes(kp)
            .map_err(|e| DecodingError::failed_to_parse("Sr25519 keypair", e))?;

        Ok(Self::from(secret))
    }

    /// Sign a message using the private key of this keypair.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let sig = RistrettoSchnorr::sign(&self.secret.0, msg, &mut rand::rngs::OsRng).expect(
            "SchnorrSignature::sign shouldn't return a Result (Blake2b<u64> is hard coded as the hasher)",
        );
        let mut buf = vec![0u8; 64];
        buf[..32].copy_from_slice(sig.get_public_nonce().as_bytes());
        buf[32..].copy_from_slice(sig.get_signature().as_bytes());
        buf
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

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair")
            .field("public", self.public())
            .finish()
    }
}

/// Demote an Sr25519 keypair to a secret key.
impl From<Keypair> for SecretKey {
    fn from(kp: Keypair) -> SecretKey {
        kp.secret
    }
}

/// Promote an Sr25519 secret key into a keypair.
impl From<SecretKey> for Keypair {
    fn from(secret: SecretKey) -> Keypair {
        Keypair {
            public: PublicKey(RistrettoPublicKey::from_secret_key(&secret.0)),
            secret,
        }
    }
}

/// An Sr25519 public key.
#[derive(Eq, Clone, PartialEq, Hash, PartialOrd, Ord)]
pub struct PublicKey(RistrettoPublicKey);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublicKey(compressed): ")?;
        for byte in self.0.as_bytes() {
            write!(f, "{byte:x}")?;
        }
        Ok(())
    }
}

impl PublicKey {
    /// Verify the Sr25519 signature on a message using the public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        if sig.len() != 64 {
            return false;
        }
        let nonce = match RistrettoPublicKey::from_canonical_bytes(&sig[..32]) {
            Ok(n) => n,
            Err(_) => return false,
        };
        let sig = match RistrettoSecretKey::from_canonical_bytes(&sig[32..]) {
            Ok(s) => s,
            Err(_) => return false,
        };
        RistrettoSchnorr::new(nonce, sig).verify(&self.0, msg)
    }

    /// Convert the public key to a byte array in compressed form, i.e.
    /// where one coordinate is represented by a single bit.
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(self.0.as_bytes());
        buf
    }

    /// Try to parse a public key from a byte array containing the actual key as produced by `to_bytes`.
    pub fn try_from_bytes(k: &[u8]) -> Result<PublicKey, DecodingError> {
        let pk = RistrettoPublicKey::from_canonical_bytes(k)
            // TODO: cant pass ByteArrayError because it doesnt implement std Error
            .map_err(|e| DecodingError::failed_to_parse("Sr25519 public key", ByteArrayError(e)))?;
        Ok(PublicKey(pk))
    }
}

impl From<RistrettoPublicKey> for PublicKey {
    fn from(pk: RistrettoPublicKey) -> Self {
        PublicKey(pk)
    }
}

/// An Sr25519 secret key.
#[derive(Clone)]
pub struct SecretKey(RistrettoSecretKey);

/// View the bytes of the secret key.
impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey")
    }
}

impl SecretKey {
    /// Generate a new Sr25519 secret key.
    #[cfg(feature = "rand")]
    pub fn generate() -> SecretKey {
        SecretKey(RistrettoSecretKey::random(&mut rand::rngs::OsRng))
    }

    /// Try to parse an Sr25519 secret key from a byte slice
    /// containing the actual key, zeroing the input on success.
    /// If the bytes do not constitute a valid Sr25519 secret key, an error is
    /// returned.
    pub fn try_from_bytes(mut sk_bytes: impl AsMut<[u8]>) -> Result<SecretKey, DecodingError> {
        let sk_bytes = sk_bytes.as_mut();
        let secret = RistrettoSecretKey::from_canonical_bytes(&*sk_bytes)
            .map_err(|e| DecodingError::failed_to_parse("Sr25519 secret key", ByteArrayError(e)))?;
        sk_bytes.zeroize();
        Ok(SecretKey(secret))
    }

    // Not great, leaves the secret key in stack memory (all key types not just Sr25519)
    pub(crate) fn to_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(self.0.as_bytes());
        buf
    }
}

#[derive(Debug)]
struct ByteArrayError(tari_crypto::tari_utilities::ByteArrayError);

impl Display for ByteArrayError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ByteArrayError")
    }
}

impl std::error::Error for ByteArrayError {}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;

    fn eq_keypairs(kp1: &Keypair, kp2: &Keypair) -> bool {
        kp1.public() == kp2.public() && kp1.secret.to_bytes() == kp2.secret.to_bytes()
    }

    #[test]
    #[cfg(feature = "rand")]
    fn sr25519_keypair_encode_decode() {
        fn prop() -> bool {
            let kp1 = Keypair::generate();
            let mut kp1_enc = kp1.to_bytes();
            let kp2 = Keypair::try_from_bytes(&mut kp1_enc).unwrap();
            eq_keypairs(&kp1, &kp2) && kp1_enc.iter().all(|b| *b == 0)
        }
        QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
    }

    #[test]
    #[cfg(feature = "rand")]
    fn sr25519_keypair_from_secret() {
        fn prop() -> bool {
            let kp1 = Keypair::generate();
            let mut sk = kp1.secret.to_bytes();
            let kp2 = Keypair::from(SecretKey::try_from_bytes(&mut sk).unwrap());
            eq_keypairs(&kp1, &kp2) && sk == [0u8; 32]
        }
        QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
    }

    #[test]
    #[cfg(feature = "rand")]
    fn sr25519_signature() {
        let kp = Keypair::generate();
        let pk = kp.public();

        let msg = "hello world".as_bytes();
        let sig = kp.sign(msg);
        assert!(pk.verify(msg, &sig));

        let mut invalid_sig = sig.clone();
        invalid_sig[3..6].copy_from_slice(&[10, 23, 42]);
        assert!(!pk.verify(msg, &invalid_sig));

        let invalid_msg = "h3ll0 w0rld".as_bytes();
        assert!(!pk.verify(invalid_msg, &sig));
    }
}
