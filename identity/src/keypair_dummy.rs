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

use crate::error::{DecodingError, SigningError};

#[derive(Debug, Clone)]
pub enum Keypair {}

impl Keypair {
    pub fn sign(&self, _: &[u8]) -> Result<Vec<u8>, SigningError> {
        unreachable!("Can never construct empty enum")
    }

    pub fn public(&self) -> PublicKey {
        unreachable!("Can never construct empty enum")
    }

    pub fn to_protobuf_encoding(&self) -> Result<Vec<u8>, DecodingError> {
        unreachable!("Can never construct empty enum")
    }

    pub fn from_protobuf_encoding(_: &[u8]) -> Result<Keypair, DecodingError> {
        Err(DecodingError::missing_feature(
            "ecdsa|rsa|ed25519|secp256k1",
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PublicKey {}

impl PublicKey {
    #[must_use]
    pub fn verify(&self, _: &[u8], _: &[u8]) -> bool {
        unreachable!("Can never construct empty enum")
    }

    pub fn to_protobuf_encoding(&self) -> Vec<u8> {
        unreachable!("Can never construct empty enum")
    }

    pub fn from_protobuf_encoding(_: &[u8]) -> Result<PublicKey, DecodingError> {
        Err(DecodingError::missing_feature(
            "ecdsa|rsa|ed25519|secp256k1",
        ))
    }

    #[cfg(feature = "peerid")]
    pub fn to_peer_id(&self) -> crate::PeerId {
        unreachable!("Can never construct empty enum")
    }
}
