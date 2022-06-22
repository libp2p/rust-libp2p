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

//! Legacy Noise protocols based on X25519.
//!
//! **Note**: This set of protocols is not interoperable with other
//! libp2p implementations.

use crate::{NoiseConfig, NoiseError, Protocol, ProtocolParams};
use curve25519_dalek::edwards::CompressedEdwardsY;
use lazy_static::lazy_static;
use libp2p_core::UpgradeInfo;
use libp2p_core::{identity, identity::ed25519};
use rand::Rng;
use sha2::{Digest, Sha512};
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};
use zeroize::Zeroize;

use super::*;

lazy_static! {
    static ref PARAMS_IK: ProtocolParams = "Noise_IK_25519_ChaChaPoly_SHA256"
        .parse()
        .map(ProtocolParams)
        .expect("Invalid protocol name");
    static ref PARAMS_IX: ProtocolParams = "Noise_IX_25519_ChaChaPoly_SHA256"
        .parse()
        .map(ProtocolParams)
        .expect("Invalid protocol name");
    static ref PARAMS_XX: ProtocolParams = "Noise_XX_25519_ChaChaPoly_SHA256"
        .parse()
        .map(ProtocolParams)
        .expect("Invalid protocol name");
}

/// A X25519 key.
#[derive(Clone)]
pub struct X25519([u8; 32]);

impl AsRef<[u8]> for X25519 {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Zeroize for X25519 {
    fn zeroize(&mut self) {
        self.0.zeroize()
    }
}

impl UpgradeInfo for NoiseConfig<IX, X25519> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ix/25519/chachapoly/sha256/0.1.0")
    }
}

impl UpgradeInfo for NoiseConfig<XX, X25519> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/xx/25519/chachapoly/sha256/0.1.0")
    }
}

impl<R> UpgradeInfo for NoiseConfig<IK, X25519, R> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ik/25519/chachapoly/sha256/0.1.0")
    }
}

/// Legacy Noise protocol for X25519.
///
/// **Note**: This `Protocol` provides no configuration that
/// is interoperable  with other libp2p implementations.
/// See [`crate::X25519Spec`] instead.
impl Protocol<X25519> for X25519 {
    fn params_ik() -> ProtocolParams {
        PARAMS_IK.clone()
    }

    fn params_ix() -> ProtocolParams {
        PARAMS_IX.clone()
    }

    fn params_xx() -> ProtocolParams {
        PARAMS_XX.clone()
    }

    fn public_from_bytes(bytes: &[u8]) -> Result<PublicKey<X25519>, NoiseError> {
        if bytes.len() != 32 {
            return Err(NoiseError::InvalidKey);
        }
        let mut pk = [0u8; 32];
        pk.copy_from_slice(bytes);
        Ok(PublicKey(X25519(pk)))
    }

    fn linked(id_pk: &identity::PublicKey, dh_pk: &PublicKey<X25519>) -> bool {
        if let identity::PublicKey::Ed25519(ref p) = id_pk {
            PublicKey::from_ed25519(p).as_ref() == dh_pk.as_ref()
        } else {
            false
        }
    }
}

impl Keypair<X25519> {
    /// An "empty" keypair as a starting state for DH computations in `snow`,
    /// which get manipulated through the `snow::types::Dh` interface.
    pub(super) fn default() -> Self {
        Keypair {
            secret: SecretKey(X25519([0u8; 32])),
            public: PublicKey(X25519([0u8; 32])),
        }
    }

    /// Create a new X25519 keypair.
    pub fn new() -> Keypair<X25519> {
        let mut sk_bytes = [0u8; 32];
        rand::thread_rng().fill(&mut sk_bytes);
        let sk = SecretKey(X25519(sk_bytes)); // Copy
        sk_bytes.zeroize();
        Self::from(sk)
    }

    /// Creates an X25519 `Keypair` from an [`identity::Keypair`], if possible.
    ///
    /// The returned keypair will be [associated with](KeypairIdentity) the
    /// given identity keypair.
    ///
    /// Returns `None` if the given identity keypair cannot be used as an X25519 keypair.
    ///
    /// > **Note**: If the identity keypair is already used in the context
    /// > of other cryptographic protocols outside of Noise, it should be preferred to
    /// > create a new static X25519 keypair for use in the Noise protocol.
    /// >
    /// > See also:
    /// >
    /// >  * [Noise: Static Key Reuse](http://www.noiseprotocol.org/noise.html#security-considerations)
    pub fn from_identity(id_keys: &identity::Keypair) -> Option<AuthenticKeypair<X25519>> {
        match id_keys {
            identity::Keypair::Ed25519(p) => {
                let kp = Keypair::from(SecretKey::from_ed25519(&p.secret()));
                let id = KeypairIdentity {
                    public: id_keys.public(),
                    signature: None,
                };
                Some(AuthenticKeypair {
                    keypair: kp,
                    identity: id,
                })
            }
            _ => None,
        }
    }
}

impl Default for Keypair<X25519> {
    fn default() -> Self {
        Self::new()
    }
}

/// Promote a X25519 secret key into a keypair.
impl From<SecretKey<X25519>> for Keypair<X25519> {
    fn from(secret: SecretKey<X25519>) -> Keypair<X25519> {
        let public = PublicKey(X25519(x25519((secret.0).0, X25519_BASEPOINT_BYTES)));
        Keypair { secret, public }
    }
}

impl PublicKey<X25519> {
    /// Construct a curve25519 public key from an Ed25519 public key.
    pub fn from_ed25519(pk: &ed25519::PublicKey) -> Self {
        PublicKey(X25519(
            CompressedEdwardsY(pk.encode())
                .decompress()
                .expect("An Ed25519 public key is a valid point by construction.")
                .to_montgomery()
                .0,
        ))
    }
}

impl SecretKey<X25519> {
    /// Construct a X25519 secret key from a Ed25519 secret key.
    ///
    /// > **Note**: If the Ed25519 secret key is already used in the context
    /// > of other cryptographic protocols outside of Noise, it should be preferred
    /// > to create a new keypair for use in the Noise protocol.
    /// >
    /// > See also:
    /// >
    /// >  * [Noise: Static Key Reuse](http://www.noiseprotocol.org/noise.html#security-considerations)
    /// >  * [Ed25519 to Curve25519](https://libsodium.gitbook.io/doc/advanced/ed25519-curve25519)
    pub fn from_ed25519(ed25519_sk: &ed25519::SecretKey) -> Self {
        // An Ed25519 public key is derived off the left half of the SHA512 of the
        // secret scalar, hence a matching conversion of the secret key must do
        // the same to yield a Curve25519 keypair with the same public key.
        // let ed25519_sk = ed25519::SecretKey::from(ed);
        let mut curve25519_sk: [u8; 32] = [0; 32];
        let hash = Sha512::digest(ed25519_sk.as_ref());
        curve25519_sk.copy_from_slice(&hash[..32]);
        let sk = SecretKey(X25519(curve25519_sk)); // Copy
        curve25519_sk.zeroize();
        sk
    }
}

#[doc(hidden)]
impl snow::types::Dh for Keypair<X25519> {
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
        self.secret = SecretKey(X25519(secret)); // Copy
        self.public = PublicKey(X25519(x25519(secret, X25519_BASEPOINT_BYTES)));
        secret.zeroize();
    }

    fn generate(&mut self, rng: &mut dyn snow::types::Random) {
        let mut secret = [0u8; 32];
        rng.fill_bytes(&mut secret);
        self.secret = SecretKey(X25519(secret)); // Copy
        self.public = PublicKey(X25519(x25519(secret, X25519_BASEPOINT_BYTES)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::identity::ed25519;
    use quickcheck::*;
    use sodiumoxide::crypto::sign;
    use std::os::raw::c_int;
    use x25519_dalek::StaticSecret;

    // ed25519 to x25519 keypair conversion must yield the same results as
    // obtained through libsodium.
    #[test]
    fn prop_ed25519_to_x25519_matches_libsodium() {
        fn prop() -> bool {
            let ed25519 = ed25519::Keypair::generate();
            let x25519 = Keypair::from(SecretKey::from_ed25519(&ed25519.secret()));

            let sodium_sec = ed25519_sk_to_curve25519(&sign::SecretKey(ed25519.encode()));
            let sodium_pub =
                ed25519_pk_to_curve25519(&sign::PublicKey(ed25519.public().encode().clone()));

            let our_pub = x25519.public.0;
            // libsodium does the [clamping] of the scalar upon key construction,
            // just like x25519-dalek, but this module uses the raw byte-oriented x25519
            // function from x25519-dalek, as defined in RFC7748, so "our" secret scalar
            // must be clamped before comparing it to the one computed by libsodium.
            // That happens in `StaticSecret::from`.
            //
            // [clamping]: http://www.lix.polytechnique.fr/~smith/ECC/#scalar-clamping
            let our_sec = StaticSecret::from((x25519.secret.0).0).to_bytes();

            sodium_sec.as_ref() == Some(&our_sec) && sodium_pub.as_ref() == Some(&our_pub.0)
        }

        quickcheck(prop as fn() -> _);
    }

    // The x25519 public key obtained through ed25519 keypair conversion
    // (and thus derived from the converted secret key) must match the x25519
    // public key derived directly from the ed25519 public key.
    #[test]
    fn prop_public_ed25519_to_x25519_matches() {
        fn prop() -> bool {
            let ed25519 = ed25519::Keypair::generate();
            let x25519 = Keypair::from(SecretKey::from_ed25519(&ed25519.secret()));
            let x25519_public = PublicKey::from_ed25519(&ed25519.public());
            x25519.public == x25519_public
        }

        quickcheck(prop as fn() -> _);
    }

    // Bindings to libsodium's ed25519 to curve25519 key conversions, to check that
    // they agree with the conversions performed in this module.

    extern "C" {
        pub fn crypto_sign_ed25519_pk_to_curve25519(c: *mut u8, e: *const u8) -> c_int;
        pub fn crypto_sign_ed25519_sk_to_curve25519(c: *mut u8, e: *const u8) -> c_int;
    }

    pub fn ed25519_pk_to_curve25519(k: &sign::PublicKey) -> Option<[u8; 32]> {
        let mut out = [0u8; 32];
        unsafe {
            if crypto_sign_ed25519_pk_to_curve25519(out.as_mut_ptr(), (&k.0).as_ptr()) == 0 {
                Some(out)
            } else {
                None
            }
        }
    }

    pub fn ed25519_sk_to_curve25519(k: &sign::SecretKey) -> Option<[u8; 32]> {
        let mut out = [0u8; 32];
        unsafe {
            if crypto_sign_ed25519_sk_to_curve25519(out.as_mut_ptr(), (&k.0).as_ptr()) == 0 {
                Some(out)
            } else {
                None
            }
        }
    }
}
