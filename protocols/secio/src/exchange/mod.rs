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

//! This module handles the key agreement process. Typically ECDH.

use futures::prelude::*;
use crate::SecioError;

#[path = "impl_ring.rs"]
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
mod platform;
#[path = "impl_webcrypto.rs"]
#[cfg(any(target_os = "emscripten", target_os = "unknown"))]
mod platform;

/// Possible key agreement algorithms.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyAgreement {
    EcdhP256,
    EcdhP384
}

/// Opaque private key type.
pub struct AgreementPrivateKey(platform::AgreementPrivateKey);

/// Generates a new key pair as part of the exchange.
///
/// Returns the opaque private key and the corresponding public key.
#[inline]
pub fn generate_agreement(algorithm: KeyAgreement) -> impl Future<Item = (AgreementPrivateKey, Vec<u8>), Error = SecioError> {
    platform::generate_agreement(algorithm).map(|(pr, pu)| (AgreementPrivateKey(pr), pu))
}

/// Finish the agreement. On success, returns the shared key that both remote agreed upon.
#[inline]
pub fn agree(algorithm: KeyAgreement, my_private_key: AgreementPrivateKey, other_public_key: &[u8], out_size: usize)
    -> impl Future<Item = Vec<u8>, Error = SecioError>
{
    platform::agree(algorithm, my_private_key.0, other_public_key, out_size)
}

