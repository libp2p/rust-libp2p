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

use arrayvec::ArrayVec;
use uint::*;
use libp2p_core::PeerId;
use multihash::Multihash;
use sha2::{Digest, Sha256};
use sha2::digest::generic_array::{GenericArray, typenum::U32};
use std::convert::TryFrom;
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::FromIterator;

construct_uint! {
    /// 256-bit unsigned integer.
    pub(super) struct U256(4);
}

/// A `Key` in the DHT keyspace with preserved preimage.
///
/// Keys in the DHT keyspace identify both the participating nodes, as well as
/// the records stored in the DHT.
///
/// `Key`s have an XOR metric as defined in the Kademlia paper, i.e. the bitwise XOR of
/// the hash digests, interpreted as an integer. See [`Key::distance`].
#[derive(Clone, Debug)]
pub struct Key<T> {
    preimage: T,
    bytes: KeyBytes,
}

impl<T> Key<T> {
    /// Constructs a new `Key` by running the given value through a random
    /// oracle.
    ///
    /// The preimage of type `T` is preserved. See [`Key::preimage`] and
    /// [`Key::into_preimage`].
    pub fn new(preimage: T) -> Key<T>
    where
        T: AsRef<[u8]>
    {
        let bytes = KeyBytes::new(&preimage);
        Key { preimage, bytes }
    }

    /// Borrows the preimage of the key.
    pub fn preimage(&self) -> &T {
        &self.preimage
    }

    /// Converts the key into its preimage.
    pub fn into_preimage(self) -> T {
        self.preimage
    }

    /// Computes the distance of the keys according to the XOR metric.
    pub fn distance<U>(&self, other: &U) -> Distance
    where
        U: AsRef<KeyBytes>
    {
        self.bytes.distance(other)
    }

    /// Returns the uniquely determined key with the given distance to `self`.
    ///
    /// This implements the following equivalence:
    ///
    /// `self xor other = distance <==> other = self xor distance`
    pub fn for_distance(&self, d: Distance) -> KeyBytes {
        self.bytes.for_distance(d)
    }

    /// Creates a new key by establishing a fixed `num_bytes` prefix,
    /// computed by running the `src` bytes through a random oracle.
    ///
    /// Any keys `k1` and `k2` with the same prefix satisfy:
    ///
    /// `k1.distance(k2) < 2^(8 * (32 - num_bytes))`
    ///
    /// # Panics
    ///
    /// Panics if `num_bytes > 32`.
    pub fn with_prefix(self, prefix: KeyPrefix) -> Self {
        let bytes = self.bytes.with_prefix(prefix);
        Key { bytes, .. self }
    }

    /// Gets the prefix of the key.
    pub fn prefix(&self) -> KeyPrefix {
        self.bytes.prefix()
    }
}

impl<T> Into<KeyBytes> for Key<T> {
    fn into(self) -> KeyBytes {
        self.bytes
    }
}

impl From<Multihash> for Key<Multihash> {
    fn from(m: Multihash) -> Self {
        Key::new(m)
    }
}

impl From<PeerId> for Key<PeerId> {
    fn from(p: PeerId) -> Self {
        Key::new(p)
    }
}

impl<T> AsRef<KeyBytes> for Key<T> {
    fn as_ref(&self) -> &KeyBytes {
        &self.bytes
    }
}

impl<T, U> PartialEq<Key<U>> for Key<T> {
    fn eq(&self, other: &Key<U>) -> bool {
        self.bytes == other.bytes
    }
}

impl<T> Eq for Key<T> {}

impl<T> Hash for Key<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

/// The raw bytes of a key in the DHT keyspace.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct KeyBytes {
    bytes: GenericArray<u8, U32>,
    prefix_len: u8,
}

impl Hash for KeyBytes {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl KeyBytes {
    /// Creates a new key in the DHT keyspace by running the given
    /// value through a random oracle.
    pub fn new<T>(value: T) -> Self
    where
        T: AsRef<[u8]>
    {
        KeyBytes {
            bytes: Sha256::digest(value.as_ref()),
            prefix_len: 0
        }
    }

    /// Computes the distance of the keys according to the XOR metric.
    pub fn distance<U>(&self, other: &U) -> Distance
    where
        U: AsRef<KeyBytes>
    {
        let a = U256::from(self.bytes.as_ref());
        let b = U256::from(other.as_ref().bytes.as_ref());
        Distance(a ^ b)
    }

    /// Returns the uniquely determined key with the given distance to `self`.
    ///
    /// This implements the following equivalence:
    ///
    /// `self xor other = distance <==> other = self xor distance`
    pub fn for_distance(&self, d: Distance) -> KeyBytes {
        let key_int = U256::from(self.bytes.as_ref()) ^ d.0;
        KeyBytes {
            bytes: GenericArray::from(<[u8; 32]>::from(key_int)),
            prefix_len: 0,
        }
    }

    /// Creates a new key by establishing a fixed `num_bytes` prefix,
    /// computed by running the `src` bytes through a random oracle.
    ///
    /// Any keys `k1` and `k2` with the same prefix satisfy:
    ///
    /// `k1.distance(k2) < 2^(8 * (32 - num_bytes))`
    ///
    /// # Panics
    ///
    /// Panics if `num_bytes > 32`.
    pub fn with_prefix(mut self, prefix: KeyPrefix) -> KeyBytes {
        let dst_prefix = self.bytes.as_mut().split_at_mut(prefix.0.len()).0;
        dst_prefix.copy_from_slice(&prefix.0);
        self.prefix_len = prefix.0.len() as u8;
        self
    }

    /// Gets the prefix of the key.
    pub fn prefix(&self) -> KeyPrefix {
        KeyPrefix(ArrayVec::from_iter(
            self.bytes[0..self.prefix_len as usize].iter().cloned()))
    }
}

impl AsRef<KeyBytes> for KeyBytes {
    fn as_ref(&self) -> &KeyBytes {
        self
    }
}

/// A distance between two keys in the DHT keyspace.
#[derive(Copy, Clone, PartialEq, Eq, Default, PartialOrd, Ord, Debug)]
pub struct Distance(pub(super) U256);

/// A prefix of a key in the DHT keyspace.
///
/// Equipping multiple keys with the same prefix creates
/// additional structure in the DHT keyspace by intentionally
/// grouping these keys closer together.
///
/// By default keys have an empty (0-length) prefix, resulting
/// in a fully uniformly random distribution of keys in the
/// keyspace. The more keys are equipped with prefixes, and
/// the longer these prefixes, the more clustered the distribution
/// of keys in the DHT keyspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPrefix(ArrayVec<[u8; 32]>);

impl KeyPrefix {
    /// Creates a new key prefix of length `num_bytes` by truncating
    /// the output of running the `src` bytes through a random oracle
    /// with 32 byte output range.
    ///
    /// # Panics
    ///
    /// Panics if `num_bytes > 32`.
    pub fn new<P: AsRef<[u8]>>(src: P, num_bytes: usize) -> Self {
        assert!(num_bytes <= 32);
        let src = Sha256::digest(src.as_ref());
        KeyPrefix(ArrayVec::from_iter((&src[..num_bytes]).iter().cloned()))
    }

    /// Creates a raw byte vector from the key prefix.
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl Default for KeyPrefix {
    fn default() -> Self {
        KeyPrefix(ArrayVec::new())
    }
}

/// Possible errors from reconstructing a `KeyPrefix` from
/// its raw byte representation.
#[derive(Debug)]
pub enum InvalidKeyPrefix {
    PrefixTooLong
}

impl fmt::Display for InvalidKeyPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidKeyPrefix::PrefixTooLong => write!(f, "Key prefix is too long.")
        }
    }
}

impl Error for InvalidKeyPrefix {}

impl TryFrom<&[u8]> for KeyPrefix {
    type Error = InvalidKeyPrefix;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() > 32 {
            Err(InvalidKeyPrefix::PrefixTooLong)
        } else {
            Ok(KeyPrefix(ArrayVec::from_iter(bytes.iter().cloned())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;
    use multihash::Hash::SHA2256;
    use rand::Rng;

    impl Arbitrary for KeyBytes {
        fn arbitrary<G: Gen>(g: &mut G) -> KeyBytes {
            KeyBytes::new(Vec::<u8>::arbitrary(g))
        }
    }

    impl Arbitrary for KeyPrefix {
        fn arbitrary<G: Gen>(g: &mut G) -> KeyPrefix {
            KeyPrefix::new(Vec::<u8>::arbitrary(g), g.gen_range(0, 32))
        }
    }

    impl Arbitrary for Key<PeerId> {
        fn arbitrary<G: Gen>(_: &mut G) -> Key<PeerId> {
            Key::from(PeerId::random())
        }
    }

    impl Arbitrary for Key<Multihash> {
        fn arbitrary<G: Gen>(_: &mut G) -> Key<Multihash> {
            Key::from(Multihash::random(SHA2256))
        }
    }

    #[test]
    fn identity() {
        fn prop(a: Key<PeerId>) -> bool {
            a.distance(&a) == Distance::default()
        }
        quickcheck(prop as fn(_) -> _)
    }

    #[test]
    fn symmetry() {
        fn prop(a: Key<PeerId>, b: Key<PeerId>) -> bool {
            a.distance(&b) == b.distance(&a)
        }
        quickcheck(prop as fn(_,_) -> _)
    }

    #[test]
    fn triangle_inequality() {
        fn prop(a: Key<PeerId>, b: Key<PeerId>, c: Key<PeerId>) -> TestResult {
            let ab = a.distance(&b);
            let bc = b.distance(&c);
            let (ab_plus_bc, overflow) = ab.0.overflowing_add(bc.0);
            if overflow {
                TestResult::discard()
            } else {
                TestResult::from_bool(a.distance(&c) <= Distance(ab_plus_bc))
            }
        }
        quickcheck(prop as fn(_,_,_) -> _)
    }

    #[test]
    fn unidirectionality() {
        fn prop(a: Key<PeerId>, b: Key<PeerId>) -> bool {
            let d = a.distance(&b);
            (0 .. 100).all(|_| {
                let c = Key::from(PeerId::random());
                a.distance(&c) != d || b == c
            })
        }
        quickcheck(prop as fn(_,_) -> _)
    }

    #[test]
    fn prefix_max_distance() {
        fn prop(keys: Vec<KeyBytes>, prefix: KeyPrefix) {
            let num_bytes = prefix.0.len();
            let keys = keys.into_iter()
                .map(|k| k.with_prefix(prefix.clone()))
                .collect::<Vec<_>>();
            for k1 in &keys {
                for k2 in &keys {
                    assert!(k1.distance(k2).0.leading_zeros() >= (num_bytes * 8) as u32);
                }
            }
        }
        quickcheck(prop as fn(_,_))
    }
}
