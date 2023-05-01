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

//! [libp2p-noise-spec] compliant Noise protocols based on X25519.
//!
//! [libp2p-noise-spec]: https://github.com/libp2p/specs/tree/master/noise

use crate::ProtocolParams;
use once_cell::sync::Lazy;
use rand::Rng;
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};
use zeroize::Zeroize;

use super::*;

/// Prefix of static key signatures for domain separation.
pub(crate) const STATIC_KEY_DOMAIN: &str = "noise-libp2p-static-key:";

pub(crate) static PARAMS_XX: Lazy<ProtocolParams> = Lazy::new(|| {
    "Noise_XX_25519_ChaChaPoly_SHA256"
        .parse()
        .map(ProtocolParams)
        .expect("Invalid protocol name")
});

/// A X25519 key.
#[derive(Clone)]
pub struct X25519Spec(pub(crate) [u8; 32]);

impl AsRef<[u8]> for X25519Spec {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Zeroize for X25519Spec {
    fn zeroize(&mut self) {
        self.0.zeroize()
    }
}

impl Keypair {
    /// An "empty" keypair as a starting state for DH computations in `snow`,
    /// which get manipulated through the `snow::types::Dh` interface.
    pub(super) fn default() -> Self {
        Keypair {
            secret: SecretKey(X25519Spec([0u8; 32])),
            public: PublicKey(X25519Spec([0u8; 32])),
        }
    }

    /// Create a new X25519 keypair.
    pub fn new() -> Keypair {
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill(&mut sk_bytes);
        let sk = SecretKey(X25519Spec(sk_bytes)); // Copy
        sk_bytes.zeroize();
        Self::from(sk)
    }
}

impl Default for Keypair {
    fn default() -> Self {
        Self::new()
    }
}

/// Promote a X25519 secret key into a keypair.
impl From<SecretKey> for Keypair {
    fn from(secret: SecretKey) -> Keypair {
        let public = PublicKey(X25519Spec(x25519((secret.0).0, X25519_BASEPOINT_BYTES)));
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
        self.secret = SecretKey(X25519Spec(secret)); // Copy
        self.public = PublicKey(X25519Spec(x25519(secret, X25519_BASEPOINT_BYTES)));
        secret.zeroize();
    }

    fn generate(&mut self, rng: &mut dyn snow::types::Random) {
        let mut secret = [0u8; 32];
        rng.fill_bytes(&mut secret);
        self.secret = SecretKey(X25519Spec(secret)); // Copy
        self.public = PublicKey(X25519Spec(x25519(secret, X25519_BASEPOINT_BYTES)));
        secret.zeroize();
    }

    fn dh(&self, pk: &[u8], shared_secret: &mut [u8]) -> Result<(), snow::Error> {
        let mut p = [0; 32];
        p.copy_from_slice(&pk[..32]);
        let ss = x25519((self.secret.0).0, p);
        shared_secret[..32].copy_from_slice(&ss[..]);
        Ok(())
    }
}
