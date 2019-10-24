// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Implementation of the key agreement process using the `ring` library.

use crate::{KeyAgreement, SecioError};
use futures::{future, prelude::*};
use log::debug;
use ring::agreement as ring_agreement;
use ring::rand as ring_rand;

impl Into<&'static ring_agreement::Algorithm> for KeyAgreement {
    #[inline]
    fn into(self) -> &'static ring_agreement::Algorithm {
        match self {
            KeyAgreement::EcdhP256 => &ring_agreement::ECDH_P256,
            KeyAgreement::EcdhP384 => &ring_agreement::ECDH_P384,
        }
    }
}

/// Opaque private key type.
pub type AgreementPrivateKey = ring_agreement::EphemeralPrivateKey;

/// Generates a new key pair as part of the exchange.
///
/// Returns the opaque private key and the corresponding public key.
pub fn generate_agreement(algorithm: KeyAgreement) -> impl Future<Item = (AgreementPrivateKey, Vec<u8>), Error = SecioError> {
    let rng = ring_rand::SystemRandom::new();

    match ring_agreement::EphemeralPrivateKey::generate(algorithm.into(), &rng) {
        Ok(tmp_priv_key) => {
            let r = tmp_priv_key.compute_public_key()
                .map_err(|_| SecioError::EphemeralKeyGenerationFailed)
                .map(move |tmp_pub_key| (tmp_priv_key, tmp_pub_key.as_ref().to_vec()));
            future::result(r)
        },
        Err(_) => {
            debug!("failed to generate ECDH key");
            future::err(SecioError::EphemeralKeyGenerationFailed)
        },
    }
}

/// Finish the agreement. On success, returns the shared key that both remote agreed upon.
pub fn agree(algorithm: KeyAgreement, my_private_key: AgreementPrivateKey, other_public_key: &[u8], _out_size: usize)
    -> impl Future<Item = Vec<u8>, Error = SecioError>
{
    ring_agreement::agree_ephemeral(my_private_key,
                                    &ring_agreement::UnparsedPublicKey::new(algorithm.into(), other_public_key),
                                    SecioError::SecretGenerationFailed,
                                    |key_material| Ok(key_material.to_vec()))
        .into_future()
}
