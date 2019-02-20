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

use crate::PublicKey;
use bs58;
use quick_error::quick_error;
use multihash;
use std::{fmt, str::FromStr};

/// Identifier of a peer of the network.
///
/// The data is a multihash of the public key of the peer.
// TODO: maybe keep things in decoded version?
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PeerId {
    multihash: multihash::Multihash,
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PeerId({})", self.to_base58())
    }
}

impl PeerId {
    /// Builds a `PeerId` from a public key.
    #[inline]
    pub fn from_public_key(key: PublicKey) -> PeerId {
        let key_enc = key.into_protobuf_encoding().expect("Protobuf encoding failed.");
        let multihash = multihash::encode(multihash::Hash::SHA2256, &key_enc)
            .expect("sha2-256 is always supported");
        PeerId { multihash }
    }

    /// Checks whether `data` is a valid `PeerId`. If so, returns the `PeerId`. If not, returns
    /// back the data as an error.
    #[inline]
    pub fn from_bytes(data: Vec<u8>) -> Result<PeerId, Vec<u8>> {
        match multihash::Multihash::from_bytes(data) {
            Ok(multihash) => {
                if multihash.algorithm() == multihash::Hash::SHA2256 {
                    Ok(PeerId { multihash })
                } else {
                    Err(multihash.into_bytes())
                }
            },
            Err(err) => Err(err.data),
        }
    }

    /// Turns a `Multihash` into a `PeerId`. If the multihash doesn't use the correct algorithm,
    /// returns back the data as an error.
    #[inline]
    pub fn from_multihash(data: multihash::Multihash) -> Result<PeerId, multihash::Multihash> {
        if data.algorithm() == multihash::Hash::SHA2256 {
            Ok(PeerId { multihash: data })
        } else {
            Err(data)
        }
    }

    /// Generates a random peer ID from a cryptographically secure PRNG.
    ///
    /// This is useful for randomly walking on a DHT, or for testing purposes.
    #[inline]
    pub fn random() -> PeerId {
        PeerId {
            multihash: multihash::Multihash::random(multihash::Hash::SHA2256)
        }
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// Note that this is not the same as the public key of the peer.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.multihash.into_bytes()
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// Note that this is not the same as the public key of the peer.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.multihash.as_bytes()
    }

    /// Returns a base-58 encoded string of this `PeerId`.
    #[inline]
    pub fn to_base58(&self) -> String {
        bs58::encode(self.multihash.as_bytes()).into_string()
    }

    /// Returns the raw bytes of the hash of this `PeerId`.
    #[inline]
    pub fn digest(&self) -> &[u8] {
        self.multihash.digest()
    }

    /// Checks whether the public key passed as parameter matches the public key of this `PeerId`.
    ///
    /// Returns `None` if this `PeerId`s hash algorithm is not supported when encoding the
    /// given public key, otherwise `Some` boolean as the result of an equality check.
    pub fn is_public_key(&self, public_key: &PublicKey) -> Option<bool> {
        let alg = self.multihash.algorithm();
        public_key.clone().into_protobuf_encoding().ok()
            .and_then(|pb| match multihash::encode(alg, &pb) {
                Ok(h) => Some(h == self.multihash),
                Err(multihash::EncodeError::UnsupportedType) => None
            })
    }
}

impl From<PublicKey> for PeerId {
    #[inline]
    fn from(key: PublicKey) -> PeerId {
        PeerId::from_public_key(key)
    }
}

impl PartialEq<multihash::Multihash> for PeerId {
    #[inline]
    fn eq(&self, other: &multihash::Multihash) -> bool {
        &self.multihash == other
    }
}

impl PartialEq<PeerId> for multihash::Multihash {
    #[inline]
    fn eq(&self, other: &PeerId) -> bool {
        self == &other.multihash
    }
}

impl AsRef<multihash::Multihash> for PeerId {
    #[inline]
    fn as_ref(&self) -> &multihash::Multihash {
        &self.multihash
    }
}

impl AsRef<[u8]> for PeerId {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Into<multihash::Multihash> for PeerId {
    #[inline]
    fn into(self) -> multihash::Multihash {
        self.multihash
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
    use crate::{PeerId, identity};

    #[test]
    fn peer_id_is_public_key() {
        let key = identity::Keypair::generate_ed25519().public();
        let peer_id = key.clone().into_peer_id();
        assert_eq!(peer_id.is_public_key(&key), Some(true));
    }

    #[test]
    fn peer_id_into_bytes_then_from_bytes() {
        let peer_id = identity::Keypair::generate_ed25519().public().into_peer_id();
        let second = PeerId::from_bytes(peer_id.clone().into_bytes()).unwrap();
        assert_eq!(peer_id, second);
    }

    #[test]
    fn peer_id_to_base58_then_back() {
        let peer_id = identity::Keypair::generate_ed25519().public().into_peer_id();
        let second: PeerId = peer_id.to_base58().parse().unwrap();
        assert_eq!(peer_id, second);
    }

    #[test]
    fn random_peer_id_is_valid() {
        for _ in 0 .. 5000 {
            let peer_id = PeerId::random();
            assert_eq!(peer_id, PeerId::from_bytes(peer_id.clone().into_bytes()).unwrap());
        }
    }
}
