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

use crate::{keys::{Curve25519, Keypair, SecretKey, PublicKey}, NoiseError};
use rand::FromEntropy;

pub(crate) fn to_array(bytes: &[u8]) -> Result<[u8; 32], NoiseError> {
    if bytes.len() != 32 {
        return Err(NoiseError::InvalidKey)
    }
    let mut m = [0; 32];
    m.copy_from_slice(bytes);
    Ok(m)
}

/// Custom `snow::CryptoResolver` which uses `ring` as much as possible.
pub(crate) struct Resolver;

impl snow::resolvers::CryptoResolver for Resolver {
    fn resolve_rng(&self) -> Option<Box<dyn snow::types::Random>> {
        Some(Box::new(Rng(rand::rngs::StdRng::from_entropy())))
    }

    fn resolve_dh(&self, choice: &snow::params::DHChoice) -> Option<Box<dyn snow::types::Dh>> {
        if let snow::params::DHChoice::Curve25519 = choice {
            Some(Box::new(X25519 { keypair: Keypair::gen_curve25519() }))
        } else {
            None
        }
    }

    fn resolve_hash(&self, choice: &snow::params::HashChoice) -> Option<Box<dyn snow::types::Hash>> {
        snow::resolvers::RingResolver.resolve_hash(choice)
    }

    fn resolve_cipher(&self, choice: &snow::params::CipherChoice) -> Option<Box<dyn snow::types::Cipher>> {
        snow::resolvers::RingResolver.resolve_cipher(choice)
    }
}

/// Wrapper around a CPRNG to implement `snow::Random` trait for.
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

/// X25519 Diffie-Hellman key agreement.
struct X25519 {
    keypair: Keypair<Curve25519>
}

impl snow::types::Dh for X25519 {
    fn name(&self) -> &'static str { "25519" }

    fn pub_len(&self) -> usize { 32 }

    fn priv_len(&self) -> usize { 32 }

    fn pubkey(&self) -> &[u8] {
        &self.keypair.public().as_ref()
    }

    fn privkey(&self) -> &[u8] {
        &self.keypair.secret().as_ref()
    }

    fn set(&mut self, privkey: &[u8]) {
        let mut s = [0; 32];
        s.copy_from_slice(privkey);
        let secret = SecretKey::new(s);
        let public = secret.public();
        self.keypair = Keypair::new(secret, public)
    }

    fn generate(&mut self, rng: &mut snow::types::Random) {
        let mut s = [0; 32];
        rng.fill_bytes(&mut s);
        let secret = SecretKey::new(s);
        let public = secret.public();
        self.keypair = Keypair::new(secret, public)
    }

    fn dh(&self, pubkey: &[u8], out: &mut [u8]) -> Result<(), ()> {
        let mut p = [0; 32];
        p.copy_from_slice(&pubkey[.. 32]);
        let public = PublicKey::new(p);
        let result = self.keypair.secret().ecdh(&public);
        out[.. 32].copy_from_slice(&result[..]);
        Ok(())
    }
}

