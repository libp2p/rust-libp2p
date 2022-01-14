// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! Records and record storage abstraction of the libp2p Kademlia DHT.

pub mod store;

use bytes::Bytes;
use instant::Instant;
use libp2p_core::{multihash::Multihash, Multiaddr, PeerId};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};

/// The (opaque) key of a record.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(crate = "_serde"))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key(Bytes);

impl Key {
    /// Creates a new key from the bytes of the input.
    pub fn new<K: AsRef<[u8]>>(key: &K) -> Self {
        Key(Bytes::copy_from_slice(key.as_ref()))
    }

    /// Copies the bytes of the key into a new vector.
    pub fn to_vec(&self) -> Vec<u8> {
        Vec::from(&self.0[..])
    }
}

impl Borrow<[u8]> for Key {
    fn borrow(&self) -> &[u8] {
        &self.0[..]
    }
}

impl AsRef<[u8]> for Key {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl From<Vec<u8>> for Key {
    fn from(v: Vec<u8>) -> Key {
        Key(Bytes::from(v))
    }
}

impl From<Multihash> for Key {
    fn from(m: Multihash) -> Key {
        Key::from(m.to_bytes())
    }
}

/// A record stored in the DHT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Key of the record.
    pub key: Key,
    /// Value of the record.
    pub value: Vec<u8>,
    /// The (original) publisher of the record.
    pub publisher: Option<PeerId>,
    /// The expiration time as measured by a local, monotonic clock.
    pub expires: Option<Instant>,
}

impl Record {
    /// Creates a new record for insertion into the DHT.
    pub fn new<K>(key: K, value: Vec<u8>) -> Self
    where
        K: Into<Key>,
    {
        Record {
            key: key.into(),
            value,
            publisher: None,
            expires: None,
        }
    }

    /// Checks whether the record is expired w.r.t. the given `Instant`.
    pub fn is_expired(&self, now: Instant) -> bool {
        self.expires.map_or(false, |t| now >= t)
    }
}

/// A record stored in the DHT whose value is the ID of a peer
/// who can provide the value on-demand.
///
/// Note: Two [`ProviderRecord`]s as well as their corresponding hashes are
/// equal iff their `key` and `provider` fields are equal. See the [`Hash`] and
/// [`PartialEq`] implementations.
#[derive(Clone, Debug)]
pub struct ProviderRecord {
    /// The key whose value is provided by the provider.
    pub key: Key,
    /// The provider of the value for the key.
    pub provider: PeerId,
    /// The expiration time as measured by a local, monotonic clock.
    pub expires: Option<Instant>,
    /// The known addresses that the provider may be listening on.
    pub addresses: Vec<Multiaddr>,
}

impl Hash for ProviderRecord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.provider.hash(state);
    }
}

impl PartialEq for ProviderRecord {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.provider == other.provider
    }
}

impl Eq for ProviderRecord {}

impl ProviderRecord {
    /// Creates a new provider record for insertion into a `RecordStore`.
    pub fn new<K>(key: K, provider: PeerId, addresses: Vec<Multiaddr>) -> Self
    where
        K: Into<Key>,
    {
        ProviderRecord {
            key: key.into(),
            provider,
            expires: None,
            addresses,
        }
    }

    /// Checks whether the provider record is expired w.r.t. the given `Instant`.
    pub fn is_expired(&self, now: Instant) -> bool {
        self.expires.map_or(false, |t| now >= t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::multihash::Code;
    use quickcheck::*;
    use rand::Rng;
    use std::time::Duration;

    impl Arbitrary for Key {
        fn arbitrary<G: Gen>(_: &mut G) -> Key {
            let hash = rand::thread_rng().gen::<[u8; 32]>();
            Key::from(Multihash::wrap(Code::Sha2_256.into(), &hash).unwrap())
        }
    }

    impl Arbitrary for Record {
        fn arbitrary<G: Gen>(g: &mut G) -> Record {
            Record {
                key: Key::arbitrary(g),
                value: Vec::arbitrary(g),
                publisher: if g.gen() {
                    Some(PeerId::random())
                } else {
                    None
                },
                expires: if g.gen() {
                    Some(Instant::now() + Duration::from_secs(g.gen_range(0, 60)))
                } else {
                    None
                },
            }
        }
    }

    impl Arbitrary for ProviderRecord {
        fn arbitrary<G: Gen>(g: &mut G) -> ProviderRecord {
            ProviderRecord {
                key: Key::arbitrary(g),
                provider: PeerId::random(),
                expires: if g.gen() {
                    Some(Instant::now() + Duration::from_secs(g.gen_range(0, 60)))
                } else {
                    None
                },
                addresses: vec![],
            }
        }
    }
}
