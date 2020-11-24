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
use bytes::Bytes;
use thiserror::Error;
use multihash::{Code, Multihash, MultihashDigest};
use rand::Rng;
use std::{convert::TryFrom, borrow::Borrow, fmt, hash, str::FromStr, cmp};

/// Public keys with byte-lengths smaller than `MAX_INLINE_KEY_LENGTH` will be
/// automatically used as the peer id using an identity multihash.
const MAX_INLINE_KEY_LENGTH: usize = 42;

/// Identifier of a peer of the network.
///
/// The data is a multihash of the public key of the peer.
// TODO: maybe keep things in decoded version?
#[derive(Clone, Eq)]
pub struct PeerId {
    multihash: Bytes,
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

impl cmp::PartialOrd for PeerId {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(Ord::cmp(self, other))
    }
}

impl cmp::Ord for PeerId {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // must use borrow, because as_bytes is not consistent with equality
        let lhs: &[u8] = self.borrow();
        let rhs: &[u8] = other.borrow();
        lhs.cmp(rhs)
    }
}

impl PeerId {
    /// Builds a `PeerId` from a public key.
    pub fn from_public_key(key: PublicKey) -> PeerId {
        let key_enc = key.into_protobuf_encoding();

        let hash_algorithm = if key_enc.len() <= MAX_INLINE_KEY_LENGTH {
            Code::Identity
        } else {
            Code::Sha2_256
        };

        let multihash = hash_algorithm.digest(&key_enc).to_bytes().into();

        PeerId { multihash }
    }

    /// Checks whether `data` is a valid `PeerId`. If so, returns the `PeerId`. If not, returns
    /// back the data as an error.
    pub fn from_bytes(data: Vec<u8>) -> Result<PeerId, Vec<u8>> {
        match Multihash::from_bytes(&data) {
            Ok(multihash) => PeerId::from_multihash(multihash).map_err(|_| data),
            Err(_err) => Err(data),
        }
    }

    /// Tries to turn a `Multihash` into a `PeerId`.
    ///
    /// If the multihash does not use a valid hashing algorithm for peer IDs,
    /// or the hash value does not satisfy the constraints for a hashed
    /// peer ID, it is returned as an `Err`.
    pub fn from_multihash(multihash: Multihash) -> Result<PeerId, Multihash> {
        match Code::try_from(multihash.code()) {
            Ok(Code::Sha2_256) => Ok(PeerId { multihash: multihash.to_bytes().into() }),
            Ok(Code::Identity) if multihash.digest().len() <= MAX_INLINE_KEY_LENGTH
                => Ok(PeerId { multihash: multihash.to_bytes().into() }),
            _ => Err(multihash)
        }
    }

    /// Generates a random peer ID from a cryptographically secure PRNG.
    ///
    /// This is useful for randomly walking on a DHT, or for testing purposes.
    pub fn random() -> PeerId {
        let peer_id = rand::thread_rng().gen::<[u8; 32]>();
        PeerId {
            multihash: Multihash::wrap(Code::Identity.into(), &peer_id)
                .expect("The digest size is never too large").to_bytes().into()
        }
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// **NOTE:** This byte representation is not necessarily consistent with
    /// equality of peer IDs. That is, two peer IDs may be considered equal
    /// while having a different byte representation as per `into_bytes`.
    pub fn into_bytes(self) -> Vec<u8> {
        self.multihash.to_vec()
    }

    /// Returns a raw bytes representation of this `PeerId`.
    ///
    /// **NOTE:** This byte representation is not necessarily consistent with
    /// equality of peer IDs. That is, two peer IDs may be considered equal
    /// while having a different byte representation as per `as_bytes`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.multihash
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
        let multihash = Multihash::from_bytes(&self.multihash)
           .expect("Internal multihash is always a valid");
        let alg = Code::try_from(multihash.code())
            .expect("Internal multihash is always a valid `Code`");
        let enc = public_key.clone().into_protobuf_encoding();
        Some(alg.digest(&enc) == multihash)
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

impl TryFrom<Multihash> for PeerId {
    type Error = Multihash;

    fn try_from(value: Multihash) -> Result<Self, Self::Error> {
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
        &self.multihash
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

impl From<PeerId> for Multihash {
    fn from(peer_id: PeerId) -> Self {
        Multihash::from_bytes(&peer_id.multihash)
            .expect("PeerIds always contain valid Multihashes")
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
