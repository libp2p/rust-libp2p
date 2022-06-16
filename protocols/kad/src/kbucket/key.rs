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

use crate::record;
use libp2p_core::multihash::Multihash;
use libp2p_core::PeerId;
use sha2::digest::generic_array::{typenum::U32, GenericArray};
use sha2::{Digest, Sha256};
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use uint::*;

construct_uint! {
    /// 256-bit unsigned integer.
    pub(super) struct U256(4);
}

pub trait PreimageIntoKeyBytes<P>: Debug + Send + Sync + 'static
where
    Self: Sized,
{
    fn preimage_into_key_bytes(p: &P) -> KeyBytes<Self>;
}

/// Marker struct used for default [`Key`] creation from preimage using Sha256 hashing of preimage's
/// bytes.
#[derive(Clone, Debug)]
pub struct Sha256Hash(GenericArray<u8, U32>);

// impl<P> PreimageIntoKeyBytes<P> for Sha256Hash
// where
//     P: Borrow<[u8]>,
// {
//     fn preimage_into_key_bytes(p: &P) -> KeyBytes<Self> {
//         KeyBytes::from_unchecked(Sha256::digest(p.borrow()))
//     }
// }

impl PreimageIntoKeyBytes<Multihash> for Sha256Hash {
    fn preimage_into_key_bytes(multihash: &Multihash) -> KeyBytes<Self> {
        KeyBytes::from_unchecked(Sha256::digest(multihash.to_bytes()))
    }
}

impl PreimageIntoKeyBytes<PeerId> for Sha256Hash {
    fn preimage_into_key_bytes(peer_id: &PeerId) -> KeyBytes<Self> {
        KeyBytes::from_unchecked(Sha256::digest(peer_id.to_bytes()))
    }
}

impl PreimageIntoKeyBytes<record::Key> for Sha256Hash {
    fn preimage_into_key_bytes(record_key: &record::Key) -> KeyBytes<Self> {
        KeyBytes::from_unchecked(Sha256::digest(record_key))
    }
}

impl PreimageIntoKeyBytes<Vec<u8>> for Sha256Hash {
    fn preimage_into_key_bytes(bytes: &Vec<u8>) -> KeyBytes<Self> {
        KeyBytes::from_unchecked(Sha256::digest(bytes))
    }
}

/// A `Key` in the DHT keyspace with preserved preimage.
///
/// Keys in the DHT keyspace identify both the participating nodes, as well as
/// the records stored in the DHT.
///
/// `Key`s have an XOR metric as defined in the Kademlia paper, i.e. the bitwise XOR of
/// the hash digests, interpreted as an integer. See [`Key::distance`].
#[derive(Debug)]
pub struct Key<P, C> {
    preimage: P,
    bytes: KeyBytes<C>,
    _convertor: PhantomData<C>,
}

impl<P, C> Clone for Key<P, C>
where
    P: Clone,
{
    fn clone(&self) -> Self {
        Self {
            preimage: self.preimage.clone(),
            bytes: self.bytes.clone(),
            _convertor: PhantomData::default(),
        }
    }
}

impl<P, C> Key<P, C>
where
    C: PreimageIntoKeyBytes<P>,
{
    /// Constructs a new `Key` by running the given value through a random
    /// oracle.
    ///
    /// The preimage of type `T` is preserved. See [`Key::preimage`] and
    /// [`Key::into_preimage`].
    pub fn new(preimage: P) -> Self {
        let bytes = C::preimage_into_key_bytes(&preimage);
        Self {
            preimage,
            bytes,
            _convertor: PhantomData::default(),
        }
    }
}

impl<P, C> Key<P, C> {
    /// Borrows the preimage of the key.
    pub fn preimage(&self) -> &P {
        &self.preimage
    }

    /// Converts the key into its preimage.
    pub fn into_preimage(self) -> P {
        self.preimage
    }

    /// Computes the distance of the keys according to the XOR metric.
    pub fn distance<U>(&self, other: &U) -> Distance
    where
        U: AsRef<KeyBytes<C>>,
    {
        self.bytes.distance(other)
    }

    /// Returns the uniquely determined key with the given distance to `self`.
    ///
    /// This implements the following equivalence:
    ///
    /// `self xor other = distance <==> other = self xor distance`
    pub fn for_distance(&self, d: Distance) -> KeyBytes<C> {
        self.bytes.for_distance(d)
    }

    /// Construct new `Key` from inputs without checking contents of arguments.
    pub fn from_unchecked(preimage: P, bytes: KeyBytes<C>) -> Self {
        Self {
            preimage,
            bytes,
            _convertor: PhantomData::default(),
        }
    }
}

impl<P, C> From<Key<P, C>> for KeyBytes<C> {
    fn from(key: Key<P, C>) -> KeyBytes<C> {
        key.bytes
    }
}

impl<P, C> AsRef<KeyBytes<C>> for Key<P, C> {
    fn as_ref(&self) -> &KeyBytes<C> {
        &self.bytes
    }
}

impl<P, U, C> PartialEq<Key<U, C>> for Key<P, C> {
    fn eq(&self, other: &Key<U, C>) -> bool {
        self.bytes == other.bytes
    }
}

impl<P, C> Eq for Key<P, C> {}

impl<P, C> Hash for Key<P, C> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.bytes.hash(state);
    }
}

/// The raw bytes of a key in the DHT keyspace.
#[derive(Debug)]
pub struct KeyBytes<C> {
    bytes: GenericArray<u8, U32>,
    _convertor: PhantomData<C>,
}

impl<C> Clone for KeyBytes<C> {
    fn clone(&self) -> Self {
        Self {
            bytes: self.bytes,
            _convertor: PhantomData::default(),
        }
    }
}

impl<C> KeyBytes<C> {
    /// Computes the distance of the keys according to the XOR metric.
    pub fn distance<U>(&self, other: &U) -> Distance
    where
        U: AsRef<Self>,
    {
        let a = U256::from(self.bytes.as_slice());
        let b = U256::from(other.as_ref().bytes.as_slice());
        Distance(a ^ b)
    }

    /// Returns the uniquely determined key with the given distance to `self`.
    ///
    /// This implements the following equivalence:
    ///
    /// `self xor other = distance <==> other = self xor distance`
    pub fn for_distance(&self, d: Distance) -> Self {
        let key_int = U256::from(self.bytes.as_slice()) ^ d.0;
        Self {
            bytes: GenericArray::from(<[u8; 32]>::from(key_int)),
            _convertor: PhantomData::default(),
        }
    }

    /// Construct new `KeyBytes` from input without checking contents of the argument.
    pub fn from_unchecked(bytes: GenericArray<u8, U32>) -> Self {
        Self {
            bytes,
            _convertor: PhantomData::default(),
        }
    }
}

impl<C> AsRef<KeyBytes<C>> for KeyBytes<C> {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl<C> PartialEq<KeyBytes<C>> for KeyBytes<C> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl<C> Eq for KeyBytes<C> {}

/// A distance between two keys in the DHT keyspace.
#[derive(Copy, Clone, PartialEq, Eq, Default, PartialOrd, Ord, Debug)]
pub struct Distance(pub(super) U256);

impl Distance {
    /// Returns the integer part of the base 2 logarithm of the [`Distance`].
    ///
    /// Returns `None` if the distance is zero.
    pub fn ilog2(&self) -> Option<u32> {
        (256 - self.0.leading_zeros()).checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::multihash::Code;
    use libp2p_core::PeerId;
    use quickcheck::*;
    use rand::Rng;

    impl<C> Arbitrary for Key<PeerId, C>
    where
        C: PreimageIntoKeyBytes<PeerId>,
    {
        fn arbitrary<G: Gen>(_: &mut G) -> Key<PeerId, C> {
            Key::new(PeerId::random())
        }
    }

    impl<C> Arbitrary for Key<Multihash, C>
    where
        C: PreimageIntoKeyBytes<Multihash>,
    {
        fn arbitrary<G: Gen>(_: &mut G) -> Key<Multihash, C> {
            let hash = rand::thread_rng().gen::<[u8; 32]>();
            Key::new(Multihash::wrap(Code::Sha2_256.into(), &hash).unwrap())
        }
    }

    #[test]
    fn identity() {
        fn prop<C>(a: Key<PeerId, C>) -> bool {
            a.distance(&a) == Distance::default()
        }
        quickcheck(prop::<Sha256Hash> as fn(_) -> _)
    }

    #[test]
    fn symmetry() {
        fn prop<C>(a: Key<PeerId, C>, b: Key<PeerId, C>) -> bool {
            a.distance(&b) == b.distance(&a)
        }
        quickcheck(prop::<Sha256Hash> as fn(_, _) -> _)
    }

    #[test]
    fn triangle_inequality() {
        fn prop<C>(a: Key<PeerId, C>, b: Key<PeerId, C>, c: Key<PeerId, C>) -> TestResult {
            let ab = a.distance(&b);
            let bc = b.distance(&c);
            let (ab_plus_bc, overflow) = ab.0.overflowing_add(bc.0);
            if overflow {
                TestResult::discard()
            } else {
                TestResult::from_bool(a.distance(&c) <= Distance(ab_plus_bc))
            }
        }
        quickcheck(prop::<Sha256Hash> as fn(_, _, _) -> _)
    }

    #[test]
    fn unidirectionality() {
        fn prop<C>(a: Key<PeerId, C>, b: Key<PeerId, C>) -> bool
        where
            C: PreimageIntoKeyBytes<PeerId>,
        {
            let d = a.distance(&b);
            (0..100).all(|_| {
                let c = Key::new(PeerId::random());
                a.distance(&c) != d || b == c
            })
        }
        quickcheck(prop::<Sha256Hash> as fn(_, _) -> _)
    }
}
