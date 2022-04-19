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

use crate::{NoiseConfig, NoiseError, Protocol, ProtocolParams};
use libp2p_core::identity;
use libp2p_core::UpgradeInfo;
use rand::Rng;
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};
use zeroize::Zeroize;

use super::{x25519::X25519, *};

/// Prefix of static key signatures for domain separation.
const STATIC_KEY_DOMAIN: &str = "noise-libp2p-static-key:";

/// A X25519 key.
#[derive(Clone)]
pub struct X25519Spec([u8; 32]);

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

impl Keypair<X25519Spec> {
    /// Create a new X25519 keypair.
    pub fn new() -> Keypair<X25519Spec> {
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill(&mut sk_bytes);
        let sk = SecretKey(X25519Spec(sk_bytes)); // Copy
        sk_bytes.zeroize();
        Self::from(sk)
    }
}

impl Default for Keypair<X25519Spec> {
    fn default() -> Self {
        Self::new()
    }
}

/// Promote a X25519 secret key into a keypair.
impl From<SecretKey<X25519Spec>> for Keypair<X25519Spec> {
    fn from(secret: SecretKey<X25519Spec>) -> Keypair<X25519Spec> {
        let public = PublicKey(X25519Spec(x25519((secret.0).0, X25519_BASEPOINT_BYTES)));
        Keypair { secret, public }
    }
}

impl UpgradeInfo for NoiseConfig<XX, X25519Spec> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise")
    }
}

/// **Note**: This is not currentlyy a standardised upgrade.
impl UpgradeInfo for NoiseConfig<IX, X25519Spec> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ix/25519/chachapoly/sha256/0.1.0")
    }
}

/// **Note**: This is not currently a standardised upgrade.
impl<R> UpgradeInfo for NoiseConfig<IK, X25519Spec, R> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ik/25519/chachapoly/sha256/0.1.0")
    }
}

/// Noise protocols for X25519 with libp2p-spec compliant signatures.
///
/// **Note**: Only the XX handshake pattern is currently guaranteed to be
/// interoperable with other libp2p implementations.
impl Protocol<X25519Spec> for X25519Spec {
    fn params_ik() -> ProtocolParams {
        X25519::params_ik()
    }

    fn params_ix() -> ProtocolParams {
        X25519::params_ix()
    }

    fn params_xx() -> ProtocolParams {
        X25519::params_xx()
    }

    fn public_from_bytes(bytes: &[u8]) -> Result<PublicKey<X25519Spec>, NoiseError> {
        if bytes.len() != 32 {
            return Err(NoiseError::InvalidKey);
        }
        let mut pk = [0u8; 32];
        pk.copy_from_slice(bytes);
        Ok(PublicKey(X25519Spec(pk)))
    }

    fn verify(
        id_pk: &identity::PublicKey,
        dh_pk: &PublicKey<X25519Spec>,
        sig: &Option<Vec<u8>>,
    ) -> bool {
        sig.as_ref().map_or(false, |s| {
            id_pk.verify(&[STATIC_KEY_DOMAIN.as_bytes(), dh_pk.as_ref()].concat(), s)
        })
    }

    fn sign(
        id_keys: &identity::Keypair,
        dh_pk: &PublicKey<X25519Spec>,
    ) -> Result<Vec<u8>, NoiseError> {
        Ok(id_keys.sign(&[STATIC_KEY_DOMAIN.as_bytes(), dh_pk.as_ref()].concat())?)
    }
}

#[doc(hidden)]
impl snow::types::Dh for Keypair<X25519Spec> {
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
