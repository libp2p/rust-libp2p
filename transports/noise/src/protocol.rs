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

pub(crate) mod x25519_spec;

use crate::Error;
use libp2p_identity as identity;
use rand::SeedableRng;
use zeroize::Zeroize;

/// The parameters of a Noise protocol, consisting of a choice
/// for a handshake pattern as well as DH, cipher and hash functions.
#[derive(Clone)]
pub struct ProtocolParams(snow::params::NoiseParams);

impl ProtocolParams {
    pub(crate) fn into_builder<'b>(
        self,
        prologue: &'b [u8],
        private_key: &'b SecretKey,
        remote_public_key: Option<&'b PublicKey>,
    ) -> snow::Builder<'b> {
        let mut builder = snow::Builder::with_resolver(self.0, Box::new(Resolver))
            .prologue(prologue.as_ref())
            .local_private_key(private_key.as_ref());

        if let Some(remote_public_key) = remote_public_key {
            builder = builder.remote_public_key(remote_public_key.as_ref());
        }

        builder
    }
}

/// Type tag for the XX handshake pattern.
#[derive(Debug, Clone)]
pub enum XX {}

/// DH keypair.
#[derive(Clone)]
pub struct Keypair {
    secret: SecretKey,
    public: PublicKey,
}

/// A DH keypair that is authentic w.r.t. a [`identity::PublicKey`].
#[derive(Clone)]
pub struct AuthenticKeypair {
    pub(crate) keypair: Keypair,
    pub(crate) identity: KeypairIdentity,
}

impl AuthenticKeypair {
    /// Returns the public DH key of this keypair.
    pub fn public_dh_key(&self) -> &PublicKey {
        &self.keypair.public
    }

    /// Extract the public [`KeypairIdentity`] from this `AuthenticKeypair`,
    /// dropping the DH `Keypair`.
    #[deprecated(
        since = "0.40.0",
        note = "This function was only used internally and will be removed in the future unless more usecases come up."
    )]
    pub fn into_identity(self) -> KeypairIdentity {
        self.identity
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

impl Keypair {
    /// The public key of the DH keypair.
    pub fn public(&self) -> &PublicKey {
        &self.public
    }

    /// The secret key of the DH keypair.
    pub fn secret(&self) -> &SecretKey {
        &self.secret
    }

    /// Turn this DH keypair into a [`AuthenticKeypair`], i.e. a DH keypair that
    /// is authentic w.r.t. the given identity keypair, by signing the DH public key.
    pub fn into_authentic(self, id_keys: &identity::Keypair) -> Result<AuthenticKeypair, Error> {
        let sig = id_keys.sign(self.public.as_ref())?;

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
#[derive(Clone, Default)]
pub struct SecretKey([u8; 32]);

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.0.zeroize()
    }
}

impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// DH public key.
#[derive(Clone, PartialEq, Default)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub(crate) fn from_slice(slice: &[u8]) -> Result<Self, Error> {
        if slice.len() != 32 {
            return Err(Error::InvalidLength);
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(slice);
        Ok(PublicKey(key))
    }
}

impl AsRef<[u8]> for PublicKey {
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
            Some(Box::new(Keypair::empty()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::x25519_spec::PARAMS_XX;
    use once_cell::sync::Lazy;

    #[test]
    fn handshake_hashes_disagree_if_prologue_differs() {
        let alice = xx_builder(b"alice prologue").build_initiator().unwrap();
        let bob = xx_builder(b"bob prologue").build_responder().unwrap();

        let alice_handshake_hash = alice.get_handshake_hash();
        let bob_handshake_hash = bob.get_handshake_hash();

        assert_ne!(alice_handshake_hash, bob_handshake_hash)
    }

    #[test]
    fn handshake_hashes_agree_if_prologue_is_the_same() {
        let alice = xx_builder(b"shared knowledge").build_initiator().unwrap();
        let bob = xx_builder(b"shared knowledge").build_responder().unwrap();

        let alice_handshake_hash = alice.get_handshake_hash();
        let bob_handshake_hash = bob.get_handshake_hash();

        assert_eq!(alice_handshake_hash, bob_handshake_hash)
    }

    fn xx_builder(prologue: &'static [u8]) -> snow::Builder<'static> {
        PARAMS_XX
            .clone()
            .into_builder(prologue, TEST_KEY.secret(), None)
    }

    // Hack to work around borrow-checker.
    static TEST_KEY: Lazy<Keypair> = Lazy::new(Keypair::new);
}
