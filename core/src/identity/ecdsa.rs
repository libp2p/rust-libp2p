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

//! ECDSA keys with secp256r1 curve support.

use super::error::DecodingError;
use core::fmt;
use p256::ecdsa::{
    signature::{Signer, Verifier},
    Signature, SigningKey, VerifyingKey,
};

/// An ECDSA keypair.
#[derive(Clone)]
pub struct Keypair {
    secret: SecretKey,
    public: PublicKey,
}

impl Keypair {
    /// Generate a new random ECDSA keypair.
    pub fn generate() -> Keypair {
        Keypair::from(SecretKey::generate())
    }

    /// Sign a message using the private key of this keypair.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.secret.sign(msg)
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
            .field("public", &self.public())
            .finish()
    }
}

/// Promote an ECDSA secret key into a keypair.
impl From<SecretKey> for Keypair {
    fn from(secret: SecretKey) -> Keypair {
        let public = PublicKey(VerifyingKey::from(&secret.0));
        Keypair { secret, public }
    }
}

/// Demote an ECDSA keypair to a secret key.
impl From<Keypair> for SecretKey {
    fn from(kp: Keypair) -> SecretKey {
        kp.secret
    }
}

/// An ECDSA secret key.
#[derive(Clone)]
pub struct SecretKey(SigningKey);

impl SecretKey {
    /// Generate a new random ECDSA secret key.
    pub fn generate() -> SecretKey {
        SecretKey(SigningKey::random(rand::thread_rng()))
    }

    /// Sign a message with this secret key, producing a DER-encoded ECDSA signature.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.0.sign(msg).to_der().as_bytes().to_owned()
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey")
    }
}

/// An ECDSA public key.
#[derive(Clone, PartialEq, Eq)]
pub struct PublicKey(VerifyingKey);

impl PublicKey {
    /// Verify an ECDSA signature on a message using the public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        let sig = match Signature::from_der(sig) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        self.0.verify(msg, &sig).is_ok()
    }

    /// Encode a public key into a DER encoded byte buffer as defined by SEC1 standard.
    pub fn encode_der(&self) -> Vec<u8> {
        self.0.to_encoded_point(false).as_bytes().to_owned()
    }

    // Decode a public key from a DER encoded byte buffer as defined for SEC1 standard.
    pub fn decode_der(k: &[u8]) -> Result<PublicKey, DecodingError> {
        VerifyingKey::from_sec1_bytes(k)
            .map_err(|_| DecodingError::new("failed to parse ecdsa p256 public key"))
            .map(PublicKey)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublicKey(asn.1 uncompressed): ")?;
        for byte in &self.encode_der() {
            write!(f, "{:x}", byte)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecdsa_signature() {
        let pair = Keypair::generate();
        let pk = pair.public();

        let msg = "hello world".as_bytes();
        let sig = pair.sign(msg);
        assert!(pk.verify(msg, &sig));

        let mut invalid_sig = sig.clone();
        invalid_sig[3..6].copy_from_slice(&[10, 23, 42]);
        assert!(!pk.verify(msg, &invalid_sig));

        let invalid_msg = "h3ll0 w0rld".as_bytes();
        assert!(!pk.verify(invalid_msg, &sig));
    }
}
