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
use cid::multibase::Base;
use cid::{Cid, CidGeneric, Error as CidError, Version};
use multihash::{Code, Error as MhError, Multihash, MultihashDigest};
use rand::Rng;
use std::{convert::TryFrom, fmt, str::FromStr};
use thiserror::Error;

/// Public keys with byte-lengths smaller than `MAX_INLINE_KEY_LENGTH` will be
/// automatically used as the peer id using an identity multihash.
const MAX_INLINE_KEY_LENGTH: usize = 42;

/// CIDv1 MUST use the libp2p key codec to identify the content of the CID.
const LIBP2P_KEY_CODEC: u64 = 0x72;

/// Identifier of a peer of the network.
///
/// The data is a cid of the public key of the peer.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PeerId {
    cid: Cid,
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PeerId").field(&self.to_base58()).finish()
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_base58().fmt(f)
    }
}

impl PeerId {
    /// Builds a `PeerId` from a public key.
    pub fn from_public_key(key: &PublicKey) -> PeerId {
        let key_enc = key.to_protobuf_encoding();

        let hash_algorithm = if key_enc.len() <= MAX_INLINE_KEY_LENGTH {
            Code::Identity
        } else {
            Code::Sha2_256
        };

        let multihash = hash_algorithm.digest(&key_enc);

        let cid = Cid::new_v1(LIBP2P_KEY_CODEC, multihash);

        PeerId { cid }
    }

    /// Parses a `PeerId` from bytes.
    ///
    /// In the case that the bytes cannot be
    /// interpreted as a multihash or a CID,
    /// a `PeerIdError` will be returned.
    pub fn from_bytes(data: &[u8]) -> Result<PeerId, PeerIdError> {
        if PeerId::is_data_multihash(data) {
            PeerId::from_multihash(Multihash::from_bytes(data)?)
                .map_err(|mh| PeerIdError::from(MhError::UnsupportedCode(mh.code())))
        } else {
            PeerId::from_cid(CidGeneric::try_from(data)?)
                .map_err(|_| PeerIdError::from(CidError::ParsingError))
        }
    }

    /// Check whether the raw bytes is
    /// a multihash in the form of `CIDv0`
    /// or in the form of an `identity` multihash
    fn is_data_multihash(data: &[u8]) -> bool {
        let is_cid_v0 = data.len() == 34 && data[0] == 0x12 && data[1] == 0x20;
        let is_identity_multihash = data[0] == 0x00;
        is_cid_v0 || is_identity_multihash
    }

    /// Tries to turn a `Multihash` into a `PeerId`.
    ///
    /// If the multihash does not use a valid hashing algorithm for peer IDs,
    /// or the hash value does not satisfy the constraints for a hashed
    /// peer ID, it is returned as an `Err`.
    pub fn from_multihash(multihash: Multihash) -> Result<PeerId, Multihash> {
        match Code::try_from(multihash.code()) {
            Ok(Code::Sha2_256) => Ok(PeerId {
                cid: Cid::new_v1(LIBP2P_KEY_CODEC, multihash),
            }),
            Ok(Code::Identity) if multihash.digest().len() <= MAX_INLINE_KEY_LENGTH => Ok(PeerId {
                cid: Cid::new_v1(LIBP2P_KEY_CODEC, multihash),
            }),
            _ => Err(multihash),
        }
    }

    /// Tries to turn a `Cid` into a `PeerId`.
    ///
    /// If the `Cid` is valid, then a `Some(PeerId)` will be
    /// returned, otherwise, `Err(Cid)` is returned.
    /// 
    /// A `Cid` is considered valid if and only if:
    /// * the `Cid` is a [`Cidv0`] with a valid `multihash`, or
    /// * the `Cid` is a [`Cidv1`] with:
    ///  1. a valid `multihash`, and
    ///  2. the codec for [`libp2p-key`].
    /// 
    /// A `multihash` is considered valid if and only if:
    /// 1. it uses a valid hashing algorithm for peer IDs, and
    /// 2. it satisfies the constraints for a hashed peerID.
    /// 
    /// [`Cidv0`]: https://github.com/multiformats/cid#cidv0
    /// [`Cidv1`]: https://github.com/multiformats/cid#cidv1
    /// [`libp2p-key`]: https://github.com/multiformats/multicodec/blob/master/table.csv
    pub fn from_cid(cid: Cid) -> Result<PeerId, Cid> {
        let multihash = *cid.hash();

        match cid.version() {
            Version::V0 => PeerId::from_multihash(multihash).map_err(|_| cid),
            Version::V1 => {
                if cid.codec() == LIBP2P_KEY_CODEC {
                    PeerId::from_multihash(multihash).map_err(|_| cid)
                } else {
                    Err(cid)
                }
            }
        }
    }

    /// Generates a random peer ID from a cryptographically secure PRNG.
    ///
    /// This is useful for randomly walking on a DHT, or for testing purposes.
    pub fn random() -> PeerId {
        let peer_id = rand::thread_rng().gen::<[u8; 32]>();
        let multihash = Multihash::wrap(Code::Identity.into(), &peer_id)
            .expect("The digest size is never too large");
        PeerId {
            cid: Cid::new_v1(LIBP2P_KEY_CODEC, multihash),
        }
    }

    /// Returns a raw bytes representation of the contents of the `PeerId`.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.cid.hash().to_bytes()
    }

    /// Returns a raw bytes representation of the `PeerId`.
    pub fn to_cidv1_bytes(&self) -> Vec<u8> {
        self.cid.to_bytes()
    }

    /// Returns a base-58 raw encoded string of this `PeerId`.
    ///
    /// This corresponds to the old peer ID [representation].
    ///
    /// [representation]: https://github.com/libp2p/specs/blob/master/RFC/0001-text-peerid-cid.md
    pub fn to_base58(&self) -> String {
        bs58::encode(self.to_bytes()).into_string()
    }

    /// Returns a `base32` multibase encoded string of this `PeerId`.
    pub fn to_base32(&self) -> String {
        self.cid.to_string_of_base(Base::Base32Lower).unwrap()
    }

    /// Checks whether the public key passed as parameter matches the public key of this `PeerId`.
    ///
    /// Returns `None` if this `PeerId`s hash algorithm is not supported when encoding the
    /// given public key, otherwise `Some` boolean as the result of an equality check.
    pub fn is_public_key(&self, public_key: &PublicKey) -> Option<bool> {
        let alg = Code::try_from(self.cid.hash().code())
            .expect("Internal multihash is always a valid `Code`");
        let enc = public_key.to_protobuf_encoding();
        Some(alg.digest(&enc) == *self.cid.hash())
    }
}

impl From<PublicKey> for PeerId {
    fn from(key: PublicKey) -> PeerId {
        PeerId::from_public_key(&key)
    }
}

impl From<&PublicKey> for PeerId {
    fn from(key: &PublicKey) -> PeerId {
        PeerId::from_public_key(key)
    }
}

impl TryFrom<Vec<u8>> for PeerId {
    type Error = Vec<u8>;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        PeerId::from_bytes(&value).map_err(|_| value)
    }
}

impl TryFrom<Multihash> for PeerId {
    type Error = Multihash;

    fn try_from(value: Multihash) -> Result<Self, Self::Error> {
        PeerId::from_multihash(value)
    }
}

impl AsRef<Multihash> for PeerId {
    fn as_ref(&self) -> &Multihash {
        self.cid.hash()
    }
}

impl From<PeerId> for Multihash {
    fn from(peer_id: PeerId) -> Self {
        *peer_id.cid.hash()
    }
}

impl From<PeerId> for Vec<u8> {
    fn from(peer_id: PeerId) -> Self {
        peer_id.to_bytes()
    }
}

#[derive(Debug)]
pub enum PeerIdError {
    // Variant to group errors produced by `multihash`
    MultihashError(MhError),
    // Variant to group errors produced by `cid`
    CidError(CidError),
}

impl fmt::Display for PeerIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MultihashError(_) => write!(f, "Bytes cannot be decoded as a multihash"),
            Self::CidError(_) => write!(f, "Bytes cannot be decoded as a CID"),
        }
    }
}

impl std::error::Error for PeerIdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MultihashError(inner) => Some(inner),
            Self::CidError(inner) => Some(inner),
        }
    }
}

impl From<MhError> for PeerIdError {
    fn from(e: MhError) -> Self {
        Self::MultihashError(e)
    }
}

impl From<CidError> for PeerIdError {
    fn from(e: CidError) -> Self {
        Self::CidError(e)
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("base-58 decode error: {0}")]
    B58(#[from] bs58::decode::Error),
    #[error("cid decoding error: {0}")]
    CidStr(#[from] cid::Error),
    #[error("decoding multihash failed")]
    MultiHash,
    #[error("decoding cid failed")]
    Cid,
}

impl FromStr for PeerId {
    type Err = ParseError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("Qm") || s.starts_with('1') {
            let bytes = bs58::decode(s).into_vec()?;
            PeerId::from_bytes(&bytes).map_err(|_| ParseError::MultiHash)
        } else {
            let cid = Cid::from_str(s)?;
            PeerId::from_cid(cid).map_err(|_| ParseError::Cid)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{identity, PeerId};

    #[test]
    fn peer_id_is_public_key() {
        let key = identity::Keypair::generate_ed25519().public();
        let peer_id = key.to_peer_id();
        assert_eq!(peer_id.is_public_key(&key), Some(true));
    }

    #[test]
    fn peer_id_into_bytes_then_from_bytes() {
        let peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();
        let second = PeerId::from_bytes(&peer_id.to_bytes()).unwrap();
        assert_eq!(peer_id, second);
    }

    #[test]
    fn peer_id_to_base58_then_back() {
        let peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();
        let second: PeerId = peer_id.to_base58().parse().unwrap();
        assert_eq!(peer_id, second);
    }

    #[test]
    fn random_peer_id_is_valid() {
        for _ in 0..5000 {
            let peer_id = PeerId::random();
            assert_eq!(peer_id, PeerId::from_bytes(&peer_id.to_bytes()).unwrap());
        }
    }

    #[test]
    fn peer_id_to_base32_then_back() {
        let peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();
        let second: PeerId = peer_id.to_base32().parse().unwrap();
        assert_eq!(peer_id, second);
    }

    #[test]
    fn peer_id_to_cidv1_bytes_then_back() {
        let peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();
        let second: PeerId = PeerId::from_bytes(&peer_id.to_cidv1_bytes()).unwrap();
        assert_eq!(peer_id, second);
    }

    #[test]
    fn cid_string_to_peer_id() {
        use std::str::FromStr;

        let cid_base32 = "bafzbeie5745rpv2m6tjyuugywy4d5ewrqgqqhfnf445he3omzpjbx5xqxe";
        let peer_id = PeerId::from_str(cid_base32).unwrap();

        assert_eq!(cid_base32, peer_id.to_base32());
    }

    #[test]
    fn identity_multihash_string_to_peer_id() {
        use std::str::FromStr;

        let identity_multihash_base58 = "12D3KooWD3eckifWpRn9wQpMG9R9hX3sD158z7EqHWmweQAJU5SA";
        let peer_id = PeerId::from_str(identity_multihash_base58).unwrap();

        assert_eq!(identity_multihash_base58, peer_id.to_base58());
    }

    #[test]
    fn sha256_multihash_string_to_peer_id() {
        use std::str::FromStr;

        let sha256_multihash_base58 = "QmYyQSo1c1Ym7orWxLYvCrM2EmxFTANf8wXmmE7DWjhx5N";
        let peer_id = PeerId::from_str(sha256_multihash_base58).unwrap();

        assert_eq!(sha256_multihash_base58, peer_id.to_base58());
    }

    #[test]
    fn cid_bytes_to_peer_id() {
        use cid::Cid;
        use std::str::FromStr;

        let cid_bytes =
            Cid::from_str("bafzbeie5745rpv2m6tjyuugywy4d5ewrqgqqhfnf445he3omzpjbx5xqxe")
                .unwrap()
                .to_bytes();
        let peer_id = PeerId::from_bytes(&cid_bytes).unwrap();

        assert_eq!(cid_bytes, peer_id.to_cidv1_bytes());
    }

    #[test]
    fn peer_ids_from_base58_and_from_base32_are_equal() {
        use std::str::FromStr;

        let peer_id_as_base58_str = "QmYyQSo1c1Ym7orWxLYvCrM2EmxFTANf8wXmmE7DWjhx5N";
        let peer_id_as_base32_str = "bafzbeie5745rpv2m6tjyuugywy4d5ewrqgqqhfnf445he3omzpjbx5xqxe";

        let peer_id_from_base58_str = PeerId::from_str(peer_id_as_base58_str).unwrap();
        let peer_id_from_base32_str = PeerId::from_str(peer_id_as_base32_str).unwrap();

        assert_eq!(peer_id_from_base58_str, peer_id_from_base32_str);
    }

    #[test]
    fn peer_id_from_base58_str_then_back() {
        use std::str::FromStr;

        let cidv1_peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();

        assert_eq!(cidv1_peer_id, PeerId::from_str(&cidv1_peer_id.to_base58()).unwrap());
    }
}
