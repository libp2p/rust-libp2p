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

use bs58;
use multihash;
use std::{fmt, str::FromStr};
use {PublicKeyBytes, PublicKeyBytesSlice};

/// Identifier of a peer of the network.
///
/// The data is a multihash of the public key of the peer.
// TODO: maybe keep things in decoded version?
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PeerId {
    multihash: Vec<u8>,
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PeerId({})", self.to_base58())
    }
}

impl PeerId {
    /// Builds a `PeerId` from a public key.
    #[inline]
    pub fn from_public_key(public_key: PublicKeyBytesSlice) -> PeerId {
        let data = multihash::encode(multihash::Hash::SHA2256, public_key.0)
            .expect("sha2-256 is always supported");
        PeerId { multihash: data }
    }

    /// Checks whether `data` is a valid `PeerId`. If so, returns the `PeerId`. If not, returns
    /// back the data as an error.
    #[inline]
    pub fn from_bytes(data: Vec<u8>) -> Result<PeerId, Vec<u8>> {
        match multihash::decode(&data) {
            Ok(_) => Ok(PeerId { multihash: data }),
            Err(_) => Err(data),
        }
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// Note that this is not the same as the public key of the peer.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.multihash
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// Note that this is not the same as the public key of the peer.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.multihash
    }

    /// Returns a base-58 encoded string of this `PeerId`.
    #[inline]
    pub fn to_base58(&self) -> String {
        bs58::encode(&self.multihash).into_string()
    }

    /// Returns the raw bytes of the hash of this `PeerId`.
    #[inline]
    pub fn hash(&self) -> &[u8] {
        multihash::decode(&self.multihash)
            .expect("our inner value should always be valid")
            .digest
    }

    /// Checks whether the public key passed as parameter matches the public key of this `PeerId`.
    ///
    /// Returns `None` if this `PeerId`s hash algorithm is not supported when encoding the
    /// given public key, otherwise `Some` boolean as the result of an equality check.
    pub fn is_public_key(&self, public_key: PublicKeyBytesSlice) -> Option<bool> {
        let alg = multihash::decode(&self.multihash)
            .expect("our inner value should always be valid")
            .alg;
        match multihash::encode(alg, public_key.0) {
            Ok(compare) => Some(compare == self.multihash),
            Err(multihash::Error::UnsupportedType) => None,
            Err(_) => Some(false),
        }
    }
}

impl From<PublicKeyBytes> for PeerId {
    #[inline]
    fn from(pubkey: PublicKeyBytes) -> PeerId {
        PublicKeyBytesSlice(&pubkey.0).into()
    }
}

impl<'a> From<PublicKeyBytesSlice<'a>> for PeerId {
    #[inline]
    fn from(pubkey: PublicKeyBytesSlice<'a>) -> PeerId {
        PeerId::from_public_key(pubkey)
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum ParseError {
        B58(e: bs58::decode::DecodeError) {
            display("base-58 decode error: {}", e)
            cause(e)
            from()
        }
        MultiHash {
            display("decoding multihash failed")
        }
    }
}

impl FromStr for PeerId {
    type Err = ParseError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = bs58::decode(s).into_vec()?;
        PeerId::from_bytes(bytes).map_err(|_| ParseError::MultiHash)
    }
}

#[cfg(test)]
mod tests {
    use rand::random;
    use {PeerId, PublicKeyBytes, PublicKeyBytesSlice};

    #[test]
    fn pubkey_as_slice_to_owned() {
        let key = PublicKeyBytes((0 .. 2048).map(|_| -> u8 { random() }).collect());
        assert_eq!(key.clone().as_slice().to_owned(), key);
    }

    #[test]
    fn peer_id_is_public_key() {
        let key = (0 .. 2048).map(|_| -> u8 { random() }).collect::<Vec<u8>>();
        let peer_id = PeerId::from_public_key(PublicKeyBytesSlice(&key));
        assert_eq!(peer_id.is_public_key(PublicKeyBytesSlice(&key)), Some(true));
    }

    #[test]
    fn pubkey_to_peer_id() {
        let key = PublicKeyBytes((0 .. 2048).map(|_| -> u8 { random() }).collect());
        let peer_id = key.to_peer_id();
        assert_eq!(peer_id.is_public_key(key.as_slice()), Some(true));
    }

    #[test]
    fn peer_id_into_bytes_then_from_bytes() {
        let peer_id = PublicKeyBytes((0 .. 2048).map(|_| -> u8 { random() }).collect()).to_peer_id();
        let second = PeerId::from_bytes(peer_id.clone().into_bytes()).unwrap();
        assert_eq!(peer_id, second);
    }

    #[test]
    fn peer_id_to_base58_then_back() {
        let peer_id = PublicKeyBytes((0 .. 2048).map(|_| -> u8 { random() }).collect()).to_peer_id();
        let second: PeerId = peer_id.to_base58().parse().unwrap();
        assert_eq!(peer_id, second);
    }
}
