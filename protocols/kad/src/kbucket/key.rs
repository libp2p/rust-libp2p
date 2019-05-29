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

use bigint::U256;
use libp2p_core::PeerId;
use multihash::Multihash;
use sha2::{Digest, Sha256, digest::generic_array::{GenericArray, typenum::U32}};

/// A `Key` is a cryptographic hash, identifying both the nodes participating in
/// the Kademlia DHT, as well as records stored in the DHT.
///
/// The set of all `Key`s defines the Kademlia keyspace.
///
/// `Key`s have an XOR metric as defined in the Kademlia paper, i.e. the bitwise XOR of
/// the hash digests, interpreted as an integer. See [`Key::distance`].
///
/// A `Key` preserves the preimage of type `T` of the hash function. See [`Key::preimage`].
#[derive(Clone, Debug)]
pub struct Key<T> {
    preimage: T,
    hash: GenericArray<u8, U32>,
}

impl<T> PartialEq for Key<T> {
    fn eq(&self, other: &Key<T>) -> bool {
        self.hash == other.hash
    }
}

impl<T> Eq for Key<T> {}

impl<TPeerId> AsRef<Key<TPeerId>> for Key<TPeerId> {
    fn as_ref(&self) -> &Key<TPeerId> {
        self
    }
}

impl<T> Key<T> {
    /// Construct a new `Key` by hashing the bytes of the given `preimage`.
    ///
    /// The preimage of type `T` is preserved. See [`Key::preimage`] and
    /// [`Key::into_preimage`].
    pub fn new(preimage: T) -> Key<T>
    where
        T: AsRef<[u8]>
    {
        let hash = Sha256::digest(preimage.as_ref());
        Key { preimage, hash }
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
    pub fn distance<U>(&self, other: &Key<U>) -> Distance {
        let a = U256::from(self.hash.as_ref());
        let b = U256::from(other.hash.as_ref());
        Distance(a ^ b)
    }
}

impl From<Multihash> for Key<Multihash> {
    fn from(h: Multihash) -> Self {
        let k = Key::new(h.clone().into_bytes());
        Key { preimage: h, hash: k.hash }
    }
}

impl From<PeerId> for Key<PeerId> {
    fn from(peer_id: PeerId) -> Self {
        Key::new(peer_id)
    }
}

/// A distance between two `Key`s.
#[derive(Copy, Clone, PartialEq, Eq, Default, PartialOrd, Ord, Debug)]
pub struct Distance(pub(super) bigint::U256);

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;

    impl Arbitrary for Key<PeerId> {
        fn arbitrary<G: Gen>(_: &mut G) -> Key<PeerId> {
            Key::from(PeerId::random())
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
