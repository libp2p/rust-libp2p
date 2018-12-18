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

//! Implementation of the key agreement process using the WebCrypto API.

use crate::{KeyAgreement, SecioError};
use futures::prelude::*;
use futures::sync::oneshot;
use stdweb::{self, Reference, web::ArrayBuffer, web::TypedArray};

/// Opaque private key type.
pub type AgreementPrivateKey = Reference;

/// Generates a new key pair as part of the exchange.
///
/// Returns the opaque private key and the corresponding public key.
pub fn generate_agreement(algorithm: KeyAgreement) -> impl Future<Item = (AgreementPrivateKey, Vec<u8>), Error = SecioError> {
    // Making sure we are initialized before we dial. Initialization is protected by a simple
    // boolean static variable, so it's not a problem to call it multiple times and the cost
    // is negligible.
    stdweb::initialize();

    let (tx, rx) = oneshot::channel();
    let mut tx = Some(tx);

    let curve = match algorithm {
        KeyAgreement::EcdhP256 => "P-256",
        KeyAgreement::EcdhP384 => "P-384",
    };

    let send = move |private, public| {
        let _ = tx.take()
            .expect("JavaScript promise has been resolved twice")       // TODO: prove
            .send((private, public));
    };

    js!{
        var send = @{send};

        let obj = {
            name : "ECDH",
            namedCurve: @{curve},
        };

        window.crypto.subtle
            .generateKey("ECDH", true, ["deriveKey", "deriveBits"])
            .then(function(key) {
                window.crypto.subtle.exportKey("raw", key.publicKey)
                    .then(function(pubkey) { send(key.privateKey, pubkey) })
            });
    };

    rx
        .map(move |(private, public): (AgreementPrivateKey, Reference)| {
            // TODO: is this actually true? the WebCrypto specs are blurry
            let array = public.downcast::<ArrayBuffer>()
                .expect("The output of crypto.subtle.exportKey is always an ArrayBuffer");
            (private, Vec::<u8>::from(array))
        })
        .map_err(|_| unreachable!())
}

/// Finish the agreement. On success, returns the shared key that both remote agreed upon.
pub fn agree(algorithm: KeyAgreement, key: AgreementPrivateKey, other_public_key: &[u8], out_size: usize)
    -> impl Future<Item = Vec<u8>, Error = SecioError>
{
    let (tx, rx) = oneshot::channel();
    let mut tx = Some(tx);

    let curve = match algorithm {
        KeyAgreement::EcdhP256 => "P-256",
        KeyAgreement::EcdhP384 => "P-384",
    };

    let other_public_key = TypedArray::from(other_public_key).buffer();

    let out_size = out_size as u32;

    let send = move |out: Reference| {
        let _ = tx.take()
            .expect("JavaScript promise has been resolved twice")       // TODO: prove
            .send(out);
    };

    js!{
        var key = @{key};
        var other_public_key = @{other_public_key};
        var send = @{send};
        var curve = @{curve};
        var out_size = @{out_size};

        let import_params = {
            name : "ECDH",
            namedCurve: curve,
        };

        window.crypto.subtle.importKey("raw", other_public_key, import_params, false, ["deriveBits"])
            .then(function(public_key) {
                let derive_params = {
                    name : "ECDH",
                    namedCurve: curve,
                    public: public_key,
                };

                window.crypto.subtle.deriveBits(derive_params, key, out_size)
            })
            .then(function(bits) {
                send(new Uint8Array(bits));
            });
    };

    rx
        .map(move |buffer| {
            Vec::<u8>::from(buffer.downcast::<ArrayBuffer>().
                expect("We put the bits into a Uint8Array, which can be casted into \
                        an ArrayBuffer"))
        })
        .map_err(|_| unreachable!())
}
