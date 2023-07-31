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

//! Errors during identity key operations.

use std::error::Error;
use std::fmt;

use crate::KeyType;

/// An error during decoding of key material.
#[derive(Debug)]
pub struct DecodingError {
    msg: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl DecodingError {
    #[allow(dead_code)]
    pub(crate) fn missing_feature(feature_name: &'static str) -> Self {
        Self {
            msg: format!("cargo feature `{feature_name}` is not enabled"),
            source: None,
        }
    }

    #[cfg(any(
        feature = "ecdsa",
        feature = "secp256k1",
        feature = "ed25519",
        feature = "rsa"
    ))]
    pub(crate) fn failed_to_parse<E, S>(what: &'static str, source: S) -> Self
    where
        E: Error + Send + Sync + 'static,
        S: Into<Option<E>>,
    {
        Self {
            msg: format!("failed to parse {what}"),
            source: match source.into() {
                None => None,
                Some(e) => Some(Box::new(e)),
            },
        }
    }

    #[cfg(any(
        feature = "ecdsa",
        feature = "secp256k1",
        feature = "ed25519",
        feature = "rsa"
    ))]
    pub(crate) fn bad_protobuf(
        what: &'static str,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            msg: format!("failed to decode {what} from protobuf"),
            source: Some(Box::new(source)),
        }
    }

    #[cfg(any(
        all(feature = "rsa", not(target_arch = "wasm32")),
        feature = "secp256k1",
        feature = "ecdsa"
    ))]
    pub(crate) fn encoding_unsupported(key_type: &'static str) -> Self {
        Self {
            msg: format!("encoding {key_type} key to Protobuf is unsupported"),
            source: None,
        }
    }
}

impl fmt::Display for DecodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key decoding error: {}", self.msg)
    }
}

impl Error for DecodingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &dyn Error)
    }
}

/// An error during signing of a message.
#[derive(Debug)]
pub struct SigningError {
    msg: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

/// An error during encoding of key material.
impl SigningError {
    #[cfg(any(feature = "secp256k1", feature = "rsa"))]
    pub(crate) fn new<S: ToString>(msg: S) -> Self {
        Self {
            msg: msg.to_string(),
            source: None,
        }
    }

    #[cfg(feature = "rsa")]
    pub(crate) fn source(self, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            source: Some(Box::new(source)),
            ..self
        }
    }
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key signing error: {}", self.msg)
    }
}

impl Error for SigningError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &dyn Error)
    }
}

/// Error produced when failing to convert [`Keypair`](crate::Keypair) to a more concrete keypair.
#[derive(Debug)]
pub struct OtherVariantError {
    actual: KeyType,
}

impl OtherVariantError {
    #[allow(dead_code)] // This is used but the cfg is too complicated to write ..
    pub(crate) fn new(actual: KeyType) -> OtherVariantError {
        OtherVariantError { actual }
    }
}

impl fmt::Display for OtherVariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!(
            "Cannot convert to the given type, the actual key type inside is {}",
            self.actual
        ))
    }
}

impl Error for OtherVariantError {}
