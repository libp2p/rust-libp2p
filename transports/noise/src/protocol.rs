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

use libp2p_identity as identity;
use once_cell::sync::Lazy;
use rand::{Rng as _, SeedableRng};
use snow::params::NoiseParams;
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};
use zeroize::Zeroize;

use crate::Error;

/// Prefix of static key signatures for domain separation.
pub(crate) const STATIC_KEY_DOMAIN: &str = "noise-libp2p-static-key:";

pub(crate) static PARAMS_XX: Lazy<NoiseParams> = Lazy::new(|| {
    "Noise_XX_25519_ChaChaPoly_SHA256"
        .parse()
        .expect("Invalid protocol name")
});

pub(crate) fn noise_params_into_builder<'b>(
    params: NoiseParams,
    prologue: &'b [u8],
    private_key: &'b SecretKey,
    remote_public_key: Option<&'b PublicKey>,
) -> snow::Builder<'b> {
    let mut builder = snow::Builder::with_resolver(params, Box::new(Resolver))
        .prologue(prologue.as_ref())
        .local_private_key(private_key.as_ref());

    if let Some(remote_public_key) = remote_public_key {
        builder = builder.remote_public_key(remote_public_key.as_ref());
    }

    builder
}

/// DH keypair.
#[derive(Clone)]
pub(crate) struct Keypair {
    secret: SecretKey,
    public: PublicKey,
}

/// A DH keypair that is authentic w.r.t. a [`identity::PublicKey`].
#[derive(Clone)]
pub(crate) struct AuthenticKeypair {
    pub(crate) keypair: Keypair,
    pub(crate) identity: KeypairIdentity,
}

/// The associated public identity of a DH keypair.
#[derive(Clone)]
pub(crate) struct KeypairIdentity {
    /// The public identity key.
    pub(crate) public: identity::PublicKey,
    /// The signature over the public DH key.
    pub(crate) signature: Vec<u8>,
}

impl Keypair {
    /// The secret key of the DH keypair.
    pub(crate) fn secret(&self) -> &SecretKey {
        &self.secret
    }

    /// Turn this DH keypair into a [`AuthenticKeypair`], i.e. a DH keypair that
    /// is authentic w.r.t. the given identity keypair, by signing the DH public key.
    pub(crate) fn into_authentic(
        self,
        id_keys: &identity::Keypair,
    ) -> Result<AuthenticKeypair, Error> {
        let sig = id_keys.sign(&[STATIC_KEY_DOMAIN.as_bytes(), self.public.as_ref()].concat())?;

        let identity = KeypairIdentity {
            public: id_keys.public(),
            signature: sig,
        };

        Ok(AuthenticKeypair {
            keypair: self,
            identity,
        })
    }

    /// An "empty" keypair as a starting state for DH computations in `snow`,
    /// which get manipulated through the `snow::types::Dh` interface.
    pub(crate) fn empty() -> Self {
        Keypair {
            secret: SecretKey([0u8; 32]),
            public: PublicKey([0u8; 32]),
        }
    }

    /// Create a new X25519 keypair.
    pub(crate) fn new() -> Keypair {
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill(&mut sk_bytes);
        let sk = SecretKey(sk_bytes); // Copy
        sk_bytes.zeroize();
        Self::from(sk)
    }
}

/// DH secret key.
#[derive(Clone, Default)]
pub(crate) struct SecretKey([u8; 32]);

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
pub(crate) struct PublicKey([u8; 32]);

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

impl Default for Keypair {
    fn default() -> Self {
        Self::new()
    }
}

/// Promote a X25519 secret key into a keypair.
impl From<SecretKey> for Keypair {
    fn from(secret: SecretKey) -> Keypair {
        let public = PublicKey(x25519(secret.0, X25519_BASEPOINT_BYTES));
        Keypair { secret, public }
    }
}

#[doc(hidden)]
impl snow::types::Dh for Keypair {
    fn name(&self) -> &'static str {
        "25519"
    }
    fn pub_len(&self) -> usize {
        32
    }
    fn priv_len(&self) -> usize {
        32
    }
    fn pubkey(&self) -> &[u8] {
        self.public.as_ref()
    }
    fn privkey(&self) -> &[u8] {
        self.secret.as_ref()
    }

    fn set(&mut self, sk: &[u8]) {
        let mut secret = [0u8; 32];
        secret.copy_from_slice(sk);
        self.secret = SecretKey(secret); // Copy
        self.public = PublicKey(x25519(secret, X25519_BASEPOINT_BYTES));
        secret.zeroize();
    }

    fn generate(&mut self, rng: &mut dyn snow::types::Random) {
        let mut secret = [0u8; 32];
        rng.fill_bytes(&mut secret);
        self.secret = SecretKey(secret); // Copy
        self.public = PublicKey(x25519(secret, X25519_BASEPOINT_BYTES));
        secret.zeroize();
    }

    fn dh(&self, pk: &[u8], shared_secret: &mut [u8]) -> Result<(), snow::Error> {
        let mut p = [0; 32];
        p.copy_from_slice(&pk[..32]);
        let ss = x25519(self.secret.0, p);
        shared_secret[..32].copy_from_slice(&ss[..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        noise_params_into_builder(PARAMS_XX.clone(), prologue, TEST_KEY.secret(), None)
    }

    // Hack to work around borrow-checker.
    static TEST_KEY: Lazy<Keypair> = Lazy::new(Keypair::new);
}
