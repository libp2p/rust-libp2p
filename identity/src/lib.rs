// Copyright 2019 Parity Technologies (UK) Ltd.
// Copyright 2023 Protocol Labs.
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

//! A node's network identity keys.
//!
//! Such identity keys can be randomly generated on every startup,
//! but using already existing, fixed keys is usually required.
//! Though libp2p uses other crates (e.g. `ed25519_dalek`) internally,
//! such details are not exposed as part of libp2p's public interface
//! to keep them easily upgradable or replaceable (e.g. to `ed25519_zebra`).
//! Consequently, keys of external ed25519 or secp256k1 crates cannot be
//! directly converted into libp2p network identities.
//! Instead, loading fixed keys must use the standard, thus more portable
//! binary representation of the specific key type
//! (e.g. [ed25519 binary format](https://datatracker.ietf.org/doc/html/rfc8032#section-5.1.5)).
//! All key types have functions to enable conversion to/from their binary representations.

mod proto {
    include!("generated/mod.rs");
    pub use self::keys_proto::*;
}

#[cfg(feature = "ecdsa")]
pub mod ecdsa;

#[cfg(feature = "ed25519")]
pub mod ed25519;

#[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
pub mod rsa;

#[cfg(feature = "secp256k1")]
pub mod secp256k1;

mod error;
#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
mod keypair;
#[cfg(all(
    not(feature = "ecdsa"),
    not(feature = "secp256k1"),
    not(feature = "ed25519"),
    not(feature = "rsa")
))]
#[path = "./keypair_dummy.rs"]
mod keypair;
#[cfg(feature = "peerid")]
mod peer_id;

#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
impl zeroize::Zeroize for proto::PrivateKey {
    fn zeroize(&mut self) {
        self.Data.zeroize();
    }
}

#[cfg(any(
    feature = "ecdsa",
    feature = "secp256k1",
    feature = "ed25519",
    feature = "rsa"
))]
impl From<&PublicKey> for proto::PublicKey {
    fn from(key: &PublicKey) -> Self {
        #[allow(deprecated)]
        match key {
            #[cfg(feature = "ed25519")]
            PublicKey::Ed25519(key) => proto::PublicKey {
                Type: proto::KeyType::Ed25519,
                Data: key.encode().to_vec(),
            },
            #[cfg(all(feature = "rsa", not(target_arch = "wasm32")))]
            PublicKey::Rsa(key) => proto::PublicKey {
                Type: proto::KeyType::RSA,
                Data: key.encode_x509(),
            },
            #[cfg(feature = "secp256k1")]
            PublicKey::Secp256k1(key) => proto::PublicKey {
                Type: proto::KeyType::Secp256k1,
                Data: key.encode().to_vec(),
            },
            #[cfg(feature = "ecdsa")]
            PublicKey::Ecdsa(key) => proto::PublicKey {
                Type: proto::KeyType::ECDSA,
                Data: key.encode_der(),
            },
        }
    }
}

pub use error::{DecodingError, SigningError};
pub use keypair::{Keypair, PublicKey};
#[cfg(feature = "peerid")]
pub use peer_id::{ParseError, PeerId};
