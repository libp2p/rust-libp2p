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

use libp2p_core::PeerId;
use multihash::Multihash;
use std::hash::{Hash, Hasher};
use wasm_timer::Instant;

/// A record stored in the DHT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Key of the record.
    pub key: Multihash,
    /// Value of the record.
    pub value: Vec<u8>,
    /// The (original) publisher of the record.
    pub publisher: Option<PeerId>,
    /// The expiration time as measured by a local, monotonic clock.
    pub expires: Option<Instant>,
}

impl Record {
    /// Creates a new record for insertion into the DHT.
    pub fn new(key: Multihash, value: Vec<u8>) -> Self {
        Record {
            key,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRecord {
    /// The key whose value is provided by the provider.
    pub key: Multihash,
    /// The provider of the value for the key.
    pub provider: PeerId,
    /// The expiration time as measured by a local, monotonic clock.
    pub expires: Option<Instant>,
}

impl Hash for ProviderRecord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.provider.hash(state);
    }
}

impl ProviderRecord {
    /// Creates a new provider record for insertion into a `RecordStore`.
    pub fn new(key: Multihash, provider: PeerId) -> Self {
        ProviderRecord {
            key, provider, expires: None
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
    use quickcheck::*;
    use multihash::Hash::SHA2256;
    use rand::Rng;
    use std::time::Duration;

    impl Arbitrary for Record {
        fn arbitrary<G: Gen>(g: &mut G) -> Record {
            Record {
                key: Multihash::random(SHA2256),
                value: Vec::arbitrary(g),
                publisher: if g.gen() { Some(PeerId::random()) } else { None },
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
                key: Multihash::random(SHA2256),
                provider: PeerId::random(),
                expires: if g.gen() {
                    Some(Instant::now() + Duration::from_secs(g.gen_range(0, 60)))
                } else {
                    None
                },
            }
        }
    }
}

