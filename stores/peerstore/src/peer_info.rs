// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! The objective of the `peerstore` crate is to provide a key-value storage. Keys are peer IDs,
//! and the `PeerInfo` struct in this module is the value.
//!
//! Note that the `PeerInfo` struct implements `PartialOrd` in a dummy way so that it can be stored
//! in a `Datastore`.
//! If the `PeerInfo` struct ever gets exposed to the public API of the crate, we may want to give
//! more thoughts about this.

use multiaddr::Multiaddr;
use std::cmp::Ordering;
use std::time::SystemTime;
use TTL;

/// Information about a peer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    // Adresses, and the time at which they will be considered expired.
    addrs: Vec<(Multiaddr, SystemTime)>,
}

impl PeerInfo {
    /// Builds a new empty `PeerInfo`.
    #[inline]
    pub fn new() -> PeerInfo {
        PeerInfo { addrs: vec![] }
    }

    /// Returns the list of the non-expired addresses stored in this `PeerInfo`.
    ///
    /// > **Note**: Keep in mind that this function is racy because addresses can expire between
    /// >           the moment when you get them and the moment when you process them.
    #[inline]
    pub fn addrs<'a>(&'a self) -> impl Iterator<Item = &'a Multiaddr> + 'a {
        let now = SystemTime::now();
        self.addrs.iter().filter_map(move |(addr, expires)| {
            if *expires >= now {
                Some(addr)
            } else {
                None
            }
        })
    }

    /// Sets the list of addresses and their time-to-live.
    ///
    /// This removes all previously-stored addresses and replaces them with new ones.
    #[inline]
    pub fn set_addrs<I>(&mut self, addrs: I)
    where
        I: IntoIterator<Item = (Multiaddr, TTL)>,
    {
        let now = SystemTime::now();
        self.addrs = addrs
            .into_iter()
            .map(move |(addr, ttl)| (addr, now + ttl))
            .collect();
    }

    /// Adds a single address and its time-to-live.
    ///
    /// If the peer info already knows about that address, then what happens depends on the
    /// `behaviour` parameter.
    pub fn add_addr(&mut self, addr: Multiaddr, ttl: TTL, behaviour: AddAddrBehaviour) {
        let expires = SystemTime::now() + ttl;

        if let Some(&mut (_, ref mut existing_expires)) =
            self.addrs.iter_mut().find(|&&mut (ref a, _)| a == &addr)
        {
            if behaviour == AddAddrBehaviour::OverwriteTtl || *existing_expires < expires {
                *existing_expires = expires;
            }
            return;
        }

        self.addrs.push((addr, expires));
    }
}

/// Behaviour of the `add_addr` function.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AddAddrBehaviour {
    /// Always overwrite the existing TTL.
    OverwriteTtl,
    /// Don't overwrite if the TTL is larger.
    IgnoreTtlIfInferior,
}

// The reason why we need to implement the PartialOrd trait is that the datastore library (a
// key-value storage) which we use allows performing queries where the results can be ordered.
//
// Since the struct that implements PartialOrd is internal and since we never use this ordering
// feature, I think it's ok to have this code.
impl PartialOrd for PeerInfo {
    #[inline]
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        None
    }
}


#[cfg(test)]
mod tests {
    extern crate serde_json;
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn ser_and_deser() {
        let peer_info = PeerInfo {
            addrs: vec![(
                "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>().unwrap(),
                UNIX_EPOCH,
            )],
        };
        let serialized = serde_json::to_string(&peer_info).unwrap();
        let deserialized: PeerInfo = serde_json::from_str(&serialized).unwrap();
        assert_eq!(peer_info, deserialized);
    }
}