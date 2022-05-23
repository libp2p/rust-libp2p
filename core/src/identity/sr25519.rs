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
use zeroize::Zeroize;

const SIGNING_CTX: &[u8] = b"rust-libp2p";

// TODO: We need this because `schnorrkel` errors doesn't implement `std::error::Error`
#[derive(Clone, Debug)]
struct StringError(String);

impl StringError {
    pub fn from_error(error: impl ToString) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for StringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for StringError {}

/// An Sr25519 keypair.
#[derive(Clone)]
pub struct Keypair(schnorrkel::Keypair);

impl Keypair {
    /// Generate a new random Sr25519 keypair.
    pub fn generate() -> Keypair {
        Self(schnorrkel::Keypair::generate())
    }

    /// Encode the keypair into a byte array by concatenating the bytes
    /// of the secret scalar and the compressed public point,
    /// an informal standard for encoding Sr25519 keypairs.
    pub fn encode(&self) -> [u8; 96] {
        self.0.to_bytes()
    }

    /// Decode a keypair from the [binary format](https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.5)
    /// produced by [`Keypair::encode`], zeroing the input on success.
    ///
    /// Note that this binary format is the same as `ed25519_dalek`'s and `ed25519_zebra`'s.
    pub fn decode(kp: &mut [u8]) -> Result<Keypair, DecodingError> {
        schnorrkel::Keypair::from_bytes(kp)
            .map(|k| {
                kp.zeroize();
                Keypair(k)
            })
            .map_err(StringError::from_error)
            .map_err(|e| DecodingError::new("Sr25519 keypair").source(e))
    }

    /// Sign a message using the private key of this keypair.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let context = schnorrkel::signing_context(SIGNING_CTX);
        self.0.sign(context.bytes(msg)).to_bytes().to_vec()
    }

    /// Get the public key of this keypair.
    pub fn public(&self) -> PublicKey {
        PublicKey(self.0.public)
    }

    /// Get the secret key of this keypair.
    pub fn secret(&self) -> SecretKey {
        SecretKey::from_bytes(&mut self.0.secret.to_bytes())
            .expect("ed25519::SecretKey::from_bytes(to_bytes(k)) != k")
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair")
            .field("public", &self.0.public)
            .finish()
    }
}

/// Demote an Sr25519 keypair to a secret key.
impl From<Keypair> for SecretKey {
    fn from(Keypair(kp): Keypair) -> SecretKey {
        SecretKey(kp.secret.clone())
    }
}

/// Promote an Sr25519 secret key into a keypair.
impl From<SecretKey> for Keypair {
    fn from(sk: SecretKey) -> Keypair {
        Self(sk.0.into())
    }
}

/// An Sr25519 public key.
#[derive(PartialEq, Eq, Clone)]
pub struct PublicKey(schnorrkel::PublicKey);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublicKey(compressed): ")?;
        for byte in self.0.to_bytes() {
            write!(f, "{:x}", byte)?;
        }
        Ok(())
    }
}

impl PublicKey {
    /// Verify the Sr25519 signature on a message using the public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        schnorrkel::Signature::from_bytes(sig)
            .and_then(|sig| self.0.verify_simple(SIGNING_CTX, msg, &sig))
            .is_ok()
    }

    /// Encode the public key into a byte array in compressed form, i.e.
    /// where one coordinate is represented by a single bit.
    pub fn encode(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Decode a public key from a byte array as produced by `encode`.
    pub fn decode(k: &[u8]) -> Result<PublicKey, DecodingError> {
        schnorrkel::PublicKey::from_bytes(k)
            .map_err(StringError::from_error)
            .map_err(|e| DecodingError::new("Sr25519 public key").source(e))
            .map(PublicKey)
    }
}

/// An Sr25519 secret key.
pub struct SecretKey(schnorrkel::SecretKey);

impl Clone for SecretKey {
    fn clone(&self) -> SecretKey {
        let mut sk_bytes = self.0.to_bytes();
        Self::from_bytes(&mut sk_bytes)
            .expect("schnorrkel::SecretKey::from_bytes(to_bytes(k)) != k")
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey")
    }
}

impl SecretKey {
    /// Generate a new Sr25519 secret key.
    pub fn generate() -> SecretKey {
        Self(schnorrkel::Keypair::generate().secret.clone())
    }

    /// Create an Sr25519 secret key from a byte slice, zeroing the input on success.
    /// If the bytes do not constitute a valid Sr25519 secret key, an error is
    /// returned.
    pub fn from_bytes(mut sk_bytes: impl AsMut<[u8]>) -> Result<SecretKey, DecodingError> {
        let sk_bytes = sk_bytes.as_mut();
        let secret = schnorrkel::SecretKey::from_bytes(&*sk_bytes)
            .map_err(StringError::from_error)
            .map_err(|e| DecodingError::new("Sr25519 secret key").source(e))?;
        sk_bytes.zeroize();
        Ok(SecretKey(secret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;

    fn eq_keypairs(kp1: &Keypair, kp2: &Keypair) -> bool {
        kp1.public() == kp2.public() && kp1.0.secret == kp2.0.secret
    }

    #[test]
    fn ed25519_keypair_encode_decode() {
        fn prop() -> bool {
            let kp1 = Keypair::generate();
            let mut kp1_enc = kp1.encode();
            let kp2 = Keypair::decode(&mut kp1_enc).unwrap();
            eq_keypairs(&kp1, &kp2) && kp1_enc.iter().all(|b| *b == 0)
        }
        QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
    }

    #[test]
    fn ed25519_keypair_from_secret() {
        fn prop() -> bool {
            let kp1 = Keypair::generate();
            let mut sk = kp1.0.secret.to_bytes();
            let kp2 = Keypair::from(SecretKey::from_bytes(&mut sk).unwrap());
            eq_keypairs(&kp1, &kp2) && sk == [0u8; 64]
        }
        QuickCheck::new().tests(10).quickcheck(prop as fn() -> _);
    }

    #[test]
    fn ed25519_signature() {
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
