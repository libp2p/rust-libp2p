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

use crate::NoiseError;
use curve25519_dalek::{
    constants::{X25519_BASEPOINT, ED25519_BASEPOINT_POINT},
    edwards::CompressedEdwardsY,
    montgomery::MontgomeryPoint,
    scalar::Scalar
};

#[derive(Debug, Clone)]
pub enum Curve25519 {}

#[derive(Debug, Clone)]
pub enum Ed25519 {}

/// ECC public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey<T> {
    bytes: [u8; 32],
    _marker: std::marker::PhantomData<T>
}

impl<T> PublicKey<T> {
    pub(crate) fn new(bytes: [u8; 32]) -> Self {
        PublicKey { bytes, _marker: std::marker::PhantomData }
    }
}

impl PublicKey<Ed25519> {
    /// Attempt to construct an Ed25519 public key from a `libp2p_core::PublicKey`.
    pub fn from_core(key: &libp2p_core::PublicKey) -> Result<PublicKey<Ed25519>, NoiseError> {
        if let libp2p_core::PublicKey::Ed25519(k) = key {
            if k.len() == 32 {
                let cp = CompressedEdwardsY::from_slice(k);
                cp.decompress().ok_or(NoiseError::InvalidKey)?;
                return Ok(PublicKey::new(cp.0))
            }
        }
        Err(NoiseError::InvalidKey)
    }

    /// Convert this Edwards 25519 public key into a Curve 25519 public key.
    pub fn into_curve_25519(self) -> PublicKey<Curve25519> {
        let m = CompressedEdwardsY(self.bytes)
            .decompress()
            .expect("Constructing a PublicKey<Ed25519> ensures this is a valid y-coordinate.")
            .to_montgomery();
        PublicKey::new(m.0)
    }
}

impl<T> AsRef<[u8]> for PublicKey<T> {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// ECC secret key.
#[derive(Clone)]
pub struct SecretKey {
    scalar: Scalar
}

impl SecretKey {
    pub(crate) fn new(bytes: [u8; 32]) -> Self {
        SecretKey {
            scalar: Scalar::from_bytes_mod_order(bytes)
        }
    }
}

impl SecretKey {
    /// Compute the public key from this secret key.
    ///
    /// Performs scalar multiplication with Curve25519's base point.
    pub fn public(&self) -> PublicKey<Curve25519> {
        PublicKey::new(self.x25519(&X25519_BASEPOINT).0)
    }

    /// Elliptic-Curve Diffie-Hellman key agreement.
    ///
    /// Performs scalar multiplication with the given public key.
    pub fn ecdh(&self, pk: &PublicKey<Curve25519>) -> [u8; 32] {
        self.x25519(&MontgomeryPoint(pk.bytes)).0
    }

    /// The actual scalar multiplication with a u-coordinate of Curve25519.
    fn x25519(&self, p: &MontgomeryPoint) -> MontgomeryPoint {
        let mut s = self.scalar.to_bytes();
        // Cf. RFC 7748 section 5 (page 7)
        s[0]  &= 248;
        s[31] &= 127;
        s[31] |= 64;
        Scalar::from_bits(s) * p
    }
}

impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        self.scalar.as_bytes()
    }
}

/// ECC secret and public key.
#[derive(Clone)]
pub struct Keypair<T> {
    secret: SecretKey,
    public: PublicKey<T>
}

impl<T> Keypair<T> {
    /// Create a new keypair.
    pub fn new(s: SecretKey, p: PublicKey<T>) -> Self {
        Keypair { secret: s, public: p }
    }

    /// Access this keypair's secret key.
    pub fn secret(&self) -> &SecretKey {
        &self.secret
    }

    /// Access this keypair's public key.
    pub fn public(&self) -> &PublicKey<T> {
        &self.public
    }
}

impl<T> Into<(SecretKey, PublicKey<T>)> for Keypair<T> {
    fn into(self) -> (SecretKey, PublicKey<T>) {
        (self.secret, self.public)
    }
}

impl Keypair<Curve25519> {
    /// Create a fresh Curve25519 keypair.
    pub fn gen_curve25519() -> Self {
        let secret = SecretKey {
            scalar: Scalar::random(&mut rand::thread_rng())
        };
        let public = secret.public();
        Keypair { secret, public }
    }
}

impl Keypair<Ed25519> {
    /// Create a fresh Ed25519 keypair.
    pub fn gen_ed25519() -> Self {
        let scalar = Scalar::random(&mut rand::thread_rng());
        let public = PublicKey::new((scalar * ED25519_BASEPOINT_POINT).compress().0);
        let secret = SecretKey { scalar };
        Keypair { secret, public }
    }
}

