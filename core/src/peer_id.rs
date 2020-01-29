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
use thiserror::Error;
use multihash;
use std::{convert::TryFrom, fmt, hash, str::FromStr};

/// Public keys with byte-lengths smaller than `MAX_INLINE_KEY_LENGTH` will be
/// automatically used as the peer id using an identity multihash.
const MAX_INLINE_KEY_LENGTH: usize = 42;

/// Identifier of a peer of the network.
///
/// The data is a multihash of the public key of the peer.
// TODO: maybe keep things in decoded version?
#[derive(Clone, Eq)]
pub struct PeerId {
    multihash: multihash::Multihash,
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PeerId")
            .field(&self.to_base58())
            .finish()
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_base58().fmt(f)
    }
}

impl PeerId {
    /// Builds a `PeerId` from a public key.
    pub fn from_public_key(key: PublicKey) -> PeerId {
        let key_enc = key.into_protobuf_encoding();

        // Note: before 0.12, this was incorrectly implemented and `SHA2256` was always used.
        // Starting from version 0.13, rust-libp2p accepts both hashed and non-hashed keys as
        // input (see `from_bytes`). Starting from version 0.16, rust-libp2p will switch to
        // not hashing the key (a.k.a. the correct behaviour).
        // In other words, rust-libp2p 0.13 is compatible with all versions of rust-libp2p.
        // Rust-libp2p 0.12 and below is **NOT** compatible with rust-libp2p 0.16 and above.
        let hash_algorithm = if key_enc.len() <= MAX_INLINE_KEY_LENGTH {
            multihash::Hash::Identity
        } else {
            multihash::Hash::SHA2256
        };

        let multihash = multihash::encode(hash_algorithm, &key_enc)
            .expect("identity and sha2-256 are always supported by known public key types");
        PeerId { multihash }
    }

    /// Checks whether `data` is a valid `PeerId`. If so, returns the `PeerId`. If not, returns
    /// back the data as an error.
    pub fn from_bytes(data: Vec<u8>) -> Result<PeerId, Vec<u8>> {
        match multihash::Multihash::from_bytes(data) {
            Ok(multihash) => {
                if multihash.algorithm() == multihash::Hash::SHA2256
                    || multihash.algorithm() == multihash::Hash::Identity
                {
                    Ok(PeerId { multihash })
                } else {
                    Err(multihash.into_bytes())
                }
            }
            Err(err) => Err(err.data),
        }
    }

    /// Turns a `Multihash` into a `PeerId`. If the multihash doesn't use the correct algorithm,
    /// returns back the data as an error.
    pub fn from_multihash(data: multihash::Multihash) -> Result<PeerId, multihash::Multihash> {
        if data.algorithm() == multihash::Hash::SHA2256 || data.algorithm() == multihash::Hash::Identity {
            Ok(PeerId { multihash: data })
        } else {
            Err(data)
        }
    }

    /// Generates a random peer ID from a cryptographically secure PRNG.
    ///
    /// This is useful for randomly walking on a DHT, or for testing purposes.
    pub fn random() -> PeerId {
        PeerId {
            multihash: multihash::Multihash::random(multihash::Hash::SHA2256)
        }
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// Note that this is not the same as the public key of the peer.
    pub fn into_bytes(self) -> Vec<u8> {
        self.multihash.into_bytes()
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// Note that this is not the same as the public key of the peer.
    pub fn as_bytes(&self) -> &[u8] {
        self.multihash.as_bytes()
    }

    /// Returns a base-58 encoded string of this `PeerId`.
    pub fn to_base58(&self) -> String {
        bs58::encode(self.multihash.as_bytes()).into_string()
    }

    /// Checks whether the public key passed as parameter matches the public key of this `PeerId`.
    ///
    /// Returns `None` if this `PeerId`s hash algorithm is not supported when encoding the
    /// given public key, otherwise `Some` boolean as the result of an equality check.
    pub fn is_public_key(&self, public_key: &PublicKey) -> Option<bool> {
        let alg = self.multihash.algorithm();
        let enc = public_key.clone().into_protobuf_encoding();
        match multihash::encode(alg, &enc) {
            Ok(h) => Some(h == self.multihash),
            Err(multihash::EncodeError::UnsupportedType) => None,
            Err(multihash::EncodeError::UnsupportedInputLength) => None,
        }
    }
}

impl hash::Hash for PeerId {
    fn hash<H>(&self, state: &mut H)
    where
        H: hash::Hasher
    {
        match self.multihash.algorithm() {
            multihash::Hash::Identity => {
                let sha256 = multihash::encode(multihash::Hash::SHA2256, self.multihash.digest())
                    .expect("encoding a SHA2256 multihash never fails; qed");
                hash::Hash::hash(sha256.digest(), state)
            },
            multihash::Hash::SHA2256 => {
                hash::Hash::hash(self.multihash.digest(), state)
            },
            _ => unreachable!("PeerId can only be built from Identity or SHA2256; qed")
        }
    }
}

impl From<PublicKey> for PeerId {
    #[inline]
    fn from(key: PublicKey) -> PeerId {
        PeerId::from_public_key(key)
    }
}

impl TryFrom<Vec<u8>> for PeerId {
    type Error = Vec<u8>;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        PeerId::from_bytes(value)
    }
}

impl TryFrom<multihash::Multihash> for PeerId {
    type Error = multihash::Multihash;

    fn try_from(value: multihash::Multihash) -> Result<Self, Self::Error> {
        PeerId::from_multihash(value)
    }
}

impl PartialEq<PeerId> for PeerId {
    fn eq(&self, other: &PeerId) -> bool {
        match (self.multihash.algorithm(), other.multihash.algorithm()) {
            (multihash::Hash::SHA2256, multihash::Hash::SHA2256) => {
                self.multihash.digest() == other.multihash.digest()
            },
            (multihash::Hash::Identity, multihash::Hash::Identity) => {
                self.multihash.digest() == other.multihash.digest()
            },
            (multihash::Hash::SHA2256, multihash::Hash::Identity) => {
                multihash::encode(multihash::Hash::SHA2256, other.multihash.digest())
                    .map(|mh| mh == self.multihash)
                    .unwrap_or(false)
            },
            (multihash::Hash::Identity, multihash::Hash::SHA2256) => {
                multihash::encode(multihash::Hash::SHA2256, self.multihash.digest())
                    .map(|mh| mh == other.multihash)
                    .unwrap_or(false)
            },
            _ => false
        }
    }
}

// TODO: The semantics of that function aren't very precise. It is possible for two `PeerId`s to
//       compare equal while their bytes representation are not. Right now, this `AsRef`
//       implementation is only used to define precedence over two `PeerId`s in case of a
//       simultaneous connection between two nodes. Since the simultaneous connection system
//       is planned to be removed (https://github.com/libp2p/rust-libp2p/issues/912), we went for
//       we keeping that function with the intent of removing it as soon as possible.
impl AsRef<[u8]> for PeerId {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl From<PeerId> for multihash::Multihash {
    fn from(peer_id: PeerId) -> Self {
        peer_id.multihash
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("base-58 decode error: {0}")]
    B58(#[from] bs58::decode::Error),
    #[error("decoding multihash failed")]
    MultiHash,
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
    use std::{convert::TryFrom as _, hash::{self, Hasher as _}};

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

    #[test]
    fn peer_id_identity_equal_to_sha2256() {
        let random_bytes = (0..64).map(|_| rand::random::<u8>()).collect::<Vec<u8>>();
        let mh1 = multihash::encode(multihash::Hash::SHA2256, &random_bytes).unwrap();
        let mh2 = multihash::encode(multihash::Hash::Identity, &random_bytes).unwrap();
        let peer_id1 = PeerId::try_from(mh1).unwrap();
        let peer_id2 = PeerId::try_from(mh2).unwrap();
        assert_eq!(peer_id1, peer_id2);
        assert_eq!(peer_id2, peer_id1);
    }

    #[test]
    fn peer_id_identity_hashes_equal_to_sha2256() {
        let random_bytes = (0..64).map(|_| rand::random::<u8>()).collect::<Vec<u8>>();
        let mh1 = multihash::encode(multihash::Hash::SHA2256, &random_bytes).unwrap();
        let mh2 = multihash::encode(multihash::Hash::Identity, &random_bytes).unwrap();
        let peer_id1 = PeerId::try_from(mh1).unwrap();
        let peer_id2 = PeerId::try_from(mh2).unwrap();

        let mut hasher1 = fnv::FnvHasher::with_key(0);
        hash::Hash::hash(&peer_id1, &mut hasher1);
        let mut hasher2 = fnv::FnvHasher::with_key(0);
        hash::Hash::hash(&peer_id2, &mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[test]
    fn peer_id_equal_across_algorithms() {
        use multihash::Hash;
        use quickcheck::{Arbitrary, Gen};

        #[derive(Debug, Clone, PartialEq, Eq)]
        struct HashAlgo(Hash);

        impl Arbitrary for HashAlgo {
            fn arbitrary<G: Gen>(g: &mut G) -> Self {
                match g.next_u32() % 4 { // make Hash::Identity more likely
                    0 => HashAlgo(Hash::SHA2256),
                    _ => HashAlgo(Hash::Identity)
                }
            }
        }

        fn property(data: Vec<u8>, algo1: HashAlgo, algo2: HashAlgo) -> bool {
            let a = PeerId::try_from(multihash::encode(algo1.0, &data).unwrap()).unwrap();
            let b = PeerId::try_from(multihash::encode(algo2.0, &data).unwrap()).unwrap();

            if algo1 == algo2 || algo1.0 == Hash::Identity || algo2.0 == Hash::Identity {
                a == b
            } else {
                a != b
            }
        }

        quickcheck::quickcheck(property as fn(Vec<u8>, HashAlgo, HashAlgo) -> bool)
    }
}
