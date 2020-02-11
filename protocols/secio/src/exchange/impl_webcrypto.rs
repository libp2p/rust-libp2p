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
use parity_send_wrapper::SendWrapper;
use std::{io, pin::Pin, task::Context, task::Poll};
use wasm_bindgen::prelude::*;

/// Opaque private key type. Contains the private key and the `SubtleCrypto` object.
pub type AgreementPrivateKey = SendSyncHack<(JsValue, web_sys::SubtleCrypto)>;

/// We use a `SendWrapper` from the `send_wrapper` crate around our JS data type. JavaScript data
/// types are not `Send`/`Sync`, but since WASM is single-threaded we know that we're only ever
/// going to access them from the same thread.
pub struct SendSyncHack<T>(SendWrapper<T>);

impl<T> Future for SendSyncHack<T>
where T: Future + Unpin {
    type Output = T::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.0.poll_unpin(cx)
    }
}

/// Generates a new key pair as part of the exchange.
///
/// Returns the opaque private key and the corresponding public key.
pub fn generate_agreement(algorithm: KeyAgreement)
    -> impl Future<Output = Result<(AgreementPrivateKey, Vec<u8>), SecioError>>
{
    let future = async move {
        // First step is to create the `SubtleCrypto` object.
        let crypto = build_crypto_future().await?;

        // We then generate the ephemeral key.
        let key_pair = {
            let obj = build_curve_obj(algorithm);

            let usages = js_sys::Array::new();
            usages.push(&JsValue::from_str("deriveKey"));
            usages.push(&JsValue::from_str("deriveBits"));

            let promise = crypto.generate_key_with_object(&obj, true, usages.as_ref())?;
            wasm_bindgen_futures::JsFuture::from(promise).await?
        };

        // WebCrypto has generated a key-pair. Let's split this key pair into a private key and a
        // public key.
        let (private, public) = {
            let private = js_sys::Reflect::get(&key_pair, &JsValue::from_str("privateKey"));
            let public = js_sys::Reflect::get(&key_pair, &JsValue::from_str("publicKey"));
            match (private, public) {
                (Ok(pr), Ok(pu)) => (pr, pu),
                (Err(err), _) => return Err(err),
                (_, Err(err)) => return Err(err),
            }
        };

        // Then we turn the public key into an `ArrayBuffer`.
        let public = {
            let promise = crypto.export_key("raw", &public.into())?;
            wasm_bindgen_futures::JsFuture::from(promise).await?
        };

        // And finally we convert this `ArrayBuffer` into a `Vec<u8>`.
        let public = js_sys::Uint8Array::new(&public);
        let mut public_buf = vec![0; public.length() as usize];
        public.copy_to(&mut public_buf);
        Ok((SendSyncHack(SendWrapper::new((private, crypto))), public_buf))
    };

    let future = future
        .map_err(|err| {
            SecioError::IoError(io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))
        });
    SendSyncHack(SendWrapper::new(Box::pin(future)))
}

/// Finish the agreement. On success, returns the shared key that both remote agreed upon.
pub fn agree(algorithm: KeyAgreement, key: AgreementPrivateKey, other_public_key: &[u8], out_size: usize)
    -> impl Future<Output = Result<Vec<u8>, SecioError>>
{
    let other_public_key = {
        // This unsafe is here because the lifetime of `other_public_key` must not outlive the
        // `tmp_view`. This is guaranteed by the fact that we clone this array right below.
        // See also https://github.com/rustwasm/wasm-bindgen/issues/1303
        let tmp_view = unsafe { js_sys::Uint8Array::view(other_public_key) };
        js_sys::Uint8Array::new(tmp_view.as_ref())
    };

    let future = async move {
        let (private_key, crypto) = key.0.take();

        // We start by importing the remote's public key into the WebCrypto world.
        let public_key = {
            // Note: contrary to what one might think, we shouldn't add the "deriveBits" usage.
            let promise = crypto
                .import_key_with_object(
                    "raw", &js_sys::Object::from(other_public_key.buffer()),
                    &build_curve_obj(algorithm), false, &js_sys::Array::new()
                )?;
            wasm_bindgen_futures::JsFuture::from(promise).await?
        };

        // We then derive the final private key.
        let bytes = {
            let derive_params = build_curve_obj(algorithm);
            let _ = js_sys::Reflect::set(derive_params.as_ref(), &JsValue::from_str("public"), &public_key);
            let promise = crypto
                .derive_bits_with_object(
                    &derive_params,
                    &web_sys::CryptoKey::from(private_key),
                    8 * out_size as u32
                )?;
            wasm_bindgen_futures::JsFuture::from(promise).await?
        };

        let bytes = js_sys::Uint8Array::new(&bytes);
        let mut buf = vec![0; bytes.length() as usize];
        bytes.copy_to(&mut buf);
        Ok(buf)
    };

    let future = future
        .map_err(|err: JsValue| {
            SecioError::IoError(io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))
        });
    SendSyncHack(SendWrapper::new(Box::pin(future)))
}

/// Builds a future that returns the `SubtleCrypto` object.
async fn build_crypto_future() -> Result<web_sys::SubtleCrypto, JsValue> {
    web_sys::window()
        .ok_or_else(|| JsValue::from_str("Window object not available"))
        .and_then(|window| window.crypto())
        .map(|crypto| crypto.subtle())
}

/// Builds a `EcKeyGenParams` object.
/// See https://developer.mozilla.org/en-US/docs/Web/API/EcKeyGenParams
fn build_curve_obj(algorithm: KeyAgreement) -> js_sys::Object {
    let obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(obj.as_ref(), &JsValue::from_str("name"), &JsValue::from_str("ECDH"));
    let _ = js_sys::Reflect::set(obj.as_ref(), &JsValue::from_str("namedCurve"), &JsValue::from_str(match algorithm {
        KeyAgreement::EcdhP256 => "P-256",
        KeyAgreement::EcdhP384 => "P-384",
    }));
    obj
}
