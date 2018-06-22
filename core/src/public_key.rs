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

use PeerId;

/// Public key used by the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicKey {
    /// DER format.
    Rsa(Vec<u8>),
    /// Format = ???
    // TODO: ^
    Ed25519(Vec<u8>),
    /// Format = ???
    // TODO: ^
    Secp256k1(Vec<u8>),
}

impl PublicKey {
    /// Turns this public key into a raw representation.
    #[inline]
    pub fn as_raw(&self) -> PublicKeyBytesSlice {
        match self {
            PublicKey::Rsa(ref data) => PublicKeyBytesSlice(data),
            PublicKey::Ed25519(ref data) => PublicKeyBytesSlice(data),
            PublicKey::Secp256k1(ref data) => PublicKeyBytesSlice(data),
        }
    }

    /// Turns this public key into a raw representation.
    #[inline]
    pub fn into_raw(self) -> PublicKeyBytes {
        match self {
            PublicKey::Rsa(data) => PublicKeyBytes(data),
            PublicKey::Ed25519(data) => PublicKeyBytes(data),
            PublicKey::Secp256k1(data) => PublicKeyBytes(data),
        }
    }

    /// Builds a `PeerId` corresponding to the public key of the node.
    #[inline]
    pub fn to_peer_id(&self) -> PeerId {
        self.as_raw().into()
    }
}

impl From<PublicKey> for PeerId {
    #[inline]
    fn from(key: PublicKey) -> PeerId {
        key.to_peer_id()
    }
}

impl From<PublicKey> for PublicKeyBytes {
    #[inline]
    fn from(key: PublicKey) -> PublicKeyBytes {
        key.into_raw()
    }
}

/// The raw bytes of a public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKeyBytes(pub Vec<u8>);

impl PublicKeyBytes {
    /// Turns this into a `PublicKeyBytesSlice`.
    #[inline]
    pub fn as_slice(&self) -> PublicKeyBytesSlice {
        PublicKeyBytesSlice(&self.0)
    }

    /// Turns this into a `PeerId`.
    #[inline]
    pub fn to_peer_id(&self) -> PeerId {
        self.as_slice().into()
    }
}

/// The raw bytes of a public key.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PublicKeyBytesSlice<'a>(pub &'a [u8]);

impl<'a> PublicKeyBytesSlice<'a> {
    /// Turns this into a `PublicKeyBytes`.
    #[inline]
    pub fn to_owned(&self) -> PublicKeyBytes {
        PublicKeyBytes(self.0.to_owned())
    }

    /// Turns this into a `PeerId`.
    #[inline]
    pub fn to_peer_id(&self) -> PeerId {
        PeerId::from_public_key(*self)
    }
}

impl<'a> PartialEq<PublicKeyBytes> for PublicKeyBytesSlice<'a> {
    #[inline]
    fn eq(&self, other: &PublicKeyBytes) -> bool {
        self.0 == &other.0[..]
    }
}

impl<'a> PartialEq<PublicKeyBytesSlice<'a>> for PublicKeyBytes {
    #[inline]
    fn eq(&self, other: &PublicKeyBytesSlice<'a>) -> bool {
        self.0 == &other.0[..]
    }
}
