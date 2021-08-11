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

//! Components of a Noise protocol.

pub mod x25519;
pub mod x25519_spec;

use crate::NoiseError;
use libp2p_core::identity;
use rand::SeedableRng;
use zeroize::Zeroize;

/// The parameters of a Noise protocol, consisting of a choice
/// for a handshake pattern as well as DH, cipher and hash functions.
#[derive(Clone)]
pub struct ProtocolParams(snow::params::NoiseParams);

impl ProtocolParams {
    /// Turn the protocol parameters into a session builder.
    pub(crate) fn into_builder(self) -> snow::Builder<'static> {
        snow::Builder::with_resolver(self.0, Box::new(Resolver))
    }
}

/// Type tag for the IK handshake pattern.
#[derive(Debug, Clone)]
pub enum IK {}

/// Type tag for the IX handshake pattern.
#[derive(Debug, Clone)]
pub enum IX {}

/// Type tag for the XX handshake pattern.
#[derive(Debug, Clone)]
pub enum XX {}

/// A Noise protocol over DH keys of type `C`. The choice of `C` determines the
/// protocol parameters for each handshake pattern.
pub trait Protocol<C> {
    /// The protocol parameters for the IK handshake pattern.
    fn params_ik() -> ProtocolParams;
    /// The protocol parameters for the IX handshake pattern.
    fn params_ix() -> ProtocolParams;
    /// The protocol parameters for the XX handshake pattern.
    fn params_xx() -> ProtocolParams;

    /// Construct a DH public key from a byte slice.
    fn public_from_bytes(s: &[u8]) -> Result<PublicKey<C>, NoiseError>;

    /// Determines whether the authenticity of the given DH static public key
    /// and public identity key is linked, i.e. that proof of ownership of a
    /// secret key for the static DH public key implies that the key is
    /// authentic w.r.t. the given public identity key.
    ///
    /// The trivial case is when the keys are byte for byte identical.
    #[allow(unused_variables)]
    #[deprecated]
    fn linked(id_pk: &identity::PublicKey, dh_pk: &PublicKey<C>) -> bool {
        false
    }

    /// Verifies that a given static DH public key is authentic w.r.t. a
    /// given public identity key in the context of an optional signature.
    ///
    /// The given static DH public key is assumed to already be authentic
    /// in the sense that possession of a corresponding secret key has been
    /// established, as is the case at the end of a Noise handshake involving
    /// static DH keys.
    ///
    /// If the public keys are [`linked`](Protocol::linked), verification succeeds
    /// without a signature, otherwise a signature over the static DH public key
    /// must be given and is verified with the public identity key, establishing
    /// the authenticity of the static DH public key w.r.t. the public identity key.
    #[allow(deprecated)]
    fn verify(id_pk: &identity::PublicKey, dh_pk: &PublicKey<C>, sig: &Option<Vec<u8>>) -> bool
    where
        C: AsRef<[u8]>,
    {
        Self::linked(id_pk, dh_pk)
            || sig
                .as_ref()
                .map_or(false, |s| id_pk.verify(dh_pk.as_ref(), s))
    }

    fn sign(id_keys: &identity::Keypair, dh_pk: &PublicKey<C>) -> Result<Vec<u8>, NoiseError>
    where
        C: AsRef<[u8]>,
    {
        Ok(id_keys.sign(dh_pk.as_ref())?)
    }
}

/// DH keypair.
#[derive(Clone)]
pub struct Keypair<T: Zeroize> {
    secret: SecretKey<T>,
    public: PublicKey<T>,
}

/// A DH keypair that is authentic w.r.t. a [`identity::PublicKey`].
#[derive(Clone)]
pub struct AuthenticKeypair<T: Zeroize> {
    keypair: Keypair<T>,
    identity: KeypairIdentity,
}

impl<T: Zeroize> AuthenticKeypair<T> {
    /// Extract the public [`KeypairIdentity`] from this `AuthenticKeypair`,
    /// dropping the DH `Keypair`.
    pub fn into_identity(self) -> KeypairIdentity {
        self.identity
    }
}

impl<T: Zeroize> std::ops::Deref for AuthenticKeypair<T> {
    type Target = Keypair<T>;

    fn deref(&self) -> &Self::Target {
        &self.keypair
    }
}

/// The associated public identity of a DH keypair.
#[derive(Clone)]
pub struct KeypairIdentity {
    /// The public identity key.
    pub public: identity::PublicKey,
    /// The signature over the public DH key.
    pub signature: Option<Vec<u8>>,
}

impl<T: Zeroize> Keypair<T> {
    /// The public key of the DH keypair.
    pub fn public(&self) -> &PublicKey<T> {
        &self.public
    }

    /// The secret key of the DH keypair.
    pub fn secret(&self) -> &SecretKey<T> {
        &self.secret
    }

    /// Turn this DH keypair into a [`AuthenticKeypair`], i.e. a DH keypair that
    /// is authentic w.r.t. the given identity keypair, by signing the DH public key.
    pub fn into_authentic(
        self,
        id_keys: &identity::Keypair,
    ) -> Result<AuthenticKeypair<T>, NoiseError>
    where
        T: AsRef<[u8]>,
        T: Protocol<T>,
    {
        let sig = T::sign(id_keys, &self.public)?;

        let identity = KeypairIdentity {
            public: id_keys.public(),
            signature: Some(sig),
        };

        Ok(AuthenticKeypair {
            keypair: self,
            identity,
        })
    }
}

/// DH secret key.
#[derive(Clone)]
pub struct SecretKey<T: Zeroize>(T);

impl<T: Zeroize> Drop for SecretKey<T> {
    fn drop(&mut self) {
        self.0.zeroize()
    }
}

impl<T: AsRef<[u8]> + Zeroize> AsRef<[u8]> for SecretKey<T> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// DH public key.
#[derive(Clone)]
pub struct PublicKey<T>(T);

impl<T: AsRef<[u8]>> PartialEq for PublicKey<T> {
    fn eq(&self, other: &PublicKey<T>) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: AsRef<[u8]>> Eq for PublicKey<T> {}

impl<T: AsRef<[u8]>> AsRef<[u8]> for PublicKey<T> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// Custom `snow::CryptoResolver` which delegates to either the
/// `RingResolver` on native or the `DefaultResolver` on wasm
/// for hash functions and symmetric ciphers, while using x25519-dalek
/// for Curve25519 DH.
struct Resolver;

impl snow::resolvers::CryptoResolver for Resolver {
    fn resolve_rng(&self) -> Option<Box<dyn snow::types::Random>> {
        Some(Box::new(Rng(rand::rngs::StdRng::from_entropy())))
    }

    fn resolve_dh(&self, choice: &snow::params::DHChoice) -> Option<Box<dyn snow::types::Dh>> {
        if let snow::params::DHChoice::Curve25519 = choice {
            Some(Box::new(Keypair::<x25519::X25519>::default()))
        } else {
            None
        }
    }

    fn resolve_hash(
        &self,
        choice: &snow::params::HashChoice,
    ) -> Option<Box<dyn snow::types::Hash>> {
        #[cfg(target_arch = "wasm32")]
        {
            snow::resolvers::DefaultResolver.resolve_hash(choice)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            snow::resolvers::RingResolver.resolve_hash(choice)
        }
    }

    fn resolve_cipher(
        &self,
        choice: &snow::params::CipherChoice,
    ) -> Option<Box<dyn snow::types::Cipher>> {
        #[cfg(target_arch = "wasm32")]
        {
            snow::resolvers::DefaultResolver.resolve_cipher(choice)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            snow::resolvers::RingResolver.resolve_cipher(choice)
        }
    }
}

/// Wrapper around a CSPRNG to implement `snow::Random` trait for.
struct Rng(rand::rngs::StdRng);

impl rand::RngCore for Rng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.0.try_fill_bytes(dest)
    }
}

impl rand::CryptoRng for Rng {}

impl snow::types::Random for Rng {}
