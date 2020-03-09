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
use multihash::{self, Code, Sha2_256};
use rand::Rng;
use std::{convert::TryFrom, borrow::Borrow, fmt, hash, str::FromStr};

/// Public keys with byte-lengths smaller than `MAX_INLINE_KEY_LENGTH` will be
/// automatically used as the peer id using an identity multihash.
const _MAX_INLINE_KEY_LENGTH: usize = 42;

/// Identifier of a peer of the network.
///
/// The data is a multihash of the public key of the peer.
// TODO: maybe keep things in decoded version?
#[derive(Clone, Eq)]
pub struct PeerId {
    multihash: multihash::Multihash,
    /// A (temporary) "canonical" multihash if `multihash` is of type
    /// multihash::Hash::Identity, so that `Borrow<[u8]>` semantics
    /// can be given, i.e. a view of a byte representation whose
    /// equality is consistent with `PartialEq`.
    canonical: Option<multihash::Multihash>,
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
        // input (see `from_bytes`). Starting from version 0.16 rust-libp2p will compare
        // `PeerId`s of different hashes equal, which makes it possible to connect through
        // secio or noise to nodes with an identity hash. Starting from version 0.17, rust-libp2p
        // will switch to not hashing the key (i.e. the correct behaviour).
        // In other words, rust-libp2p 0.16 is compatible with all versions of rust-libp2p.
        // Rust-libp2p 0.12 and below is **NOT** compatible with rust-libp2p 0.17 and above.
        let (hash_algorithm, canonical_algorithm): (_, Option<Code>) = /*if key_enc.len() <= MAX_INLINE_KEY_LENGTH {
            (multihash::Hash::Identity, Some(multihash::Hash::SHA2256))
        } else {*/
            (Code::Sha2_256, None);
        //};

        let canonical = canonical_algorithm.map(|alg|
            alg.hasher().expect("SHA2-256 hasher is always supported").digest(&key_enc));

        let multihash = hash_algorithm.hasher()
            .expect("Identity and SHA-256 hasher are always supported").digest(&key_enc);

        PeerId { multihash, canonical }
    }

    /// Checks whether `data` is a valid `PeerId`. If so, returns the `PeerId`. If not, returns
    /// back the data as an error.
    pub fn from_bytes(data: Vec<u8>) -> Result<PeerId, Vec<u8>> {
        match multihash::Multihash::from_bytes(data) {
            Ok(multihash) => {
                if multihash.algorithm() == multihash::Code::Sha2_256 {
                    Ok(PeerId { multihash, canonical: None })
                }
                else if multihash.algorithm() == multihash::Code::Identity {
                    let canonical = Sha2_256::digest(&multihash.digest());
                    Ok(PeerId { multihash, canonical: Some(canonical) })
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
        if data.algorithm() == multihash::Code::Sha2_256 {
            Ok(PeerId { multihash: data, canonical: None })
        } else if data.algorithm() == multihash::Code::Identity {
            let canonical = Sha2_256::digest(data.digest());
            Ok(PeerId { multihash: data, canonical: Some(canonical) })
        } else {
            Err(data)
        }
    }

    /// Generates a random peer ID from a cryptographically secure PRNG.
    ///
    /// This is useful for randomly walking on a DHT, or for testing purposes.
    pub fn random() -> PeerId {
        let peer_id = rand::thread_rng().gen::<[u8; 32]>();
        PeerId {
            multihash: multihash::wrap(multihash::Code::Sha2_256, &peer_id),
            canonical: None,
        }
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// **NOTE:** This byte representation is not necessarily consistent with
    /// equality of peer IDs. That is, two peer IDs may be considered equal
    /// while having a different byte representation as per `into_bytes`.
    pub fn into_bytes(self) -> Vec<u8> {
        self.multihash.into_bytes()
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// **NOTE:** This byte representation is not necessarily consistent with
    /// equality of peer IDs. That is, two peer IDs may be considered equal
    /// while having a different byte representation as per `as_bytes`.
    pub fn as_bytes(&self) -> &[u8] {
        self.multihash.as_bytes()
    }

    /// Returns a base-58 encoded string of this `PeerId`.
    pub fn to_base58(&self) -> String {
        bs58::encode(self.borrow() as &[u8]).into_string()
    }

    /// Checks whether the public key passed as parameter matches the public key of this `PeerId`.
    ///
    /// Returns `None` if this `PeerId`s hash algorithm is not supported when encoding the
    /// given public key, otherwise `Some` boolean as the result of an equality check.
    pub fn is_public_key(&self, public_key: &PublicKey) -> Option<bool> {
        let alg = self.multihash.algorithm();
        let enc = public_key.clone().into_protobuf_encoding();
        Some(alg.hasher()?.digest(&enc) == self.multihash)
    }
}

impl hash::Hash for PeerId {
    fn hash<H>(&self, state: &mut H)
    where
        H: hash::Hasher
    {
        let digest = self.borrow() as &[u8];
        hash::Hash::hash(digest, state)
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
        let self_digest = self.borrow() as &[u8];
        let other_digest = other.borrow() as &[u8];
        self_digest == other_digest
    }
}

impl Borrow<[u8]> for PeerId {
    fn borrow(&self) -> &[u8] {
        self.canonical.as_ref().map_or(self.multihash.as_bytes(), |c| c.as_bytes())
    }
}

/// **NOTE:** This byte representation is not necessarily consistent with
/// equality of peer IDs. That is, two peer IDs may be considered equal
/// while having a different byte representation as per `AsRef<[u8]>`.
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
        let mh1 = multihash::Sha2_256::digest(&random_bytes);
        let mh2 = multihash::Identity::digest(&random_bytes);
        let peer_id1 = PeerId::try_from(mh1).unwrap();
        let peer_id2 = PeerId::try_from(mh2).unwrap();
        assert_eq!(peer_id1, peer_id2);
        assert_eq!(peer_id2, peer_id1);
    }

    #[test]
    fn peer_id_identity_hashes_equal_to_sha2256() {
        let random_bytes = (0..64).map(|_| rand::random::<u8>()).collect::<Vec<u8>>();
        let mh1 = multihash::Sha2_256::digest(&random_bytes);
        let mh2 = multihash::Identity::digest(&random_bytes);
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
        use multihash::Code;
        use quickcheck::{Arbitrary, Gen};

        #[derive(Debug, Clone, PartialEq, Eq)]
        struct HashAlgo(Code);

        impl Arbitrary for HashAlgo {
            fn arbitrary<G: Gen>(g: &mut G) -> Self {
                match g.next_u32() % 4 { // make Hash::Identity more likely
                    0 => HashAlgo(Code::Sha2_256),
                    _ => HashAlgo(Code::Identity)
                }
            }
        }

        fn property(data: Vec<u8>, algo1: HashAlgo, algo2: HashAlgo) -> bool {
            let a = PeerId::try_from(algo1.0.hasher().unwrap().digest(&data)).unwrap();
            let b = PeerId::try_from(algo2.0.hasher().unwrap().digest(&data)).unwrap();

            if algo1 == algo2 || algo1.0 == Code::Identity || algo2.0 == Code::Identity {
                a == b
            } else {
                a != b
            }
        }

        quickcheck::quickcheck(property as fn(Vec<u8>, HashAlgo, HashAlgo) -> bool)
    }
}
