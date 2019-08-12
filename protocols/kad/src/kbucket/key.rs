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

use uint::*;
use libp2p_core::PeerId;
use multihash::Multihash;
use sha2::{Digest, Sha256};
use sha2::digest::generic_array::{GenericArray, typenum::U32};
use std::hash::{Hash, Hasher};

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
        self.bytes.0.hash(state);
    }
}

/// The raw bytes of a key in the DHT keyspace.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct KeyBytes(GenericArray<u8, U32>);

impl KeyBytes {
    /// Creates a new key in the DHT keyspace by running the given
    /// value through a random oracle.
    pub fn new<T>(value: T) -> Self
    where
        T: AsRef<[u8]>
    {
        KeyBytes(Sha256::digest(value.as_ref()))
    }

    /// Computes the distance of the keys according to the XOR metric.
    pub fn distance<U>(&self, other: &U) -> Distance
    where
        U: AsRef<KeyBytes>
    {
        let a = U256::from(self.0.as_ref());
        let b = U256::from(other.as_ref().0.as_ref());
        Distance(a ^ b)
    }

    /// Returns the uniquely determined key with the given distance to `self`.
    ///
    /// This implements the following equivalence:
    ///
    /// `self xor other = distance <==> other = self xor distance`
    pub fn for_distance(&self, d: Distance) -> KeyBytes {
        let key_int = U256::from(self.0.as_ref()) ^ d.0;
        KeyBytes(GenericArray::from(<[u8; 32]>::from(key_int)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;
    use multihash::Hash::SHA2256;

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
}
