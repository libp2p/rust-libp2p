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

//! Implementation of the `Peerstore` trait that simple stores peers in memory.

use super::TTL;
use PeerId;
use multiaddr::Multiaddr;
use owning_ref::OwningRefMut;
use peer_info::{AddAddrBehaviour, PeerInfo};
use peerstore::{PeerAccess, PeerAccessWeights, Peerstore};
use rand;
use std::collections::HashMap;
use std::iter;
use std::sync::{Mutex, MutexGuard};
use std::vec::IntoIter as VecIntoIter;

/// Implementation of the `Peerstore` trait that simply stores the peer information in memory.
#[derive(Debug)]
pub struct MemoryPeerstoreWeights {
    store: Mutex<HashMap<PeerId, PeerInfo<u32>>>,
}

impl MemoryPeerstoreWeights {
    /// Initializes a new `MemoryPeerstoreWeights`. The database is initially empty.
    #[inline]
    pub fn empty() -> MemoryPeerstoreWeights {
        MemoryPeerstoreWeights {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryPeerstoreWeights {
    #[inline]
    fn default() -> MemoryPeerstoreWeights {
        MemoryPeerstoreWeights::empty()
    }
}

impl<'a> Peerstore for &'a MemoryPeerstoreWeights {
    type PeerAccess = MemoryPeerstoreWeightsAccess<'a>;
    type PeersIter = VecIntoIter<PeerId>;

    fn peer(self, peer_id: &PeerId) -> Option<Self::PeerAccess> {
        let lock = self.store.lock().unwrap();
        OwningRefMut::new(lock)
            .try_map_mut(|n| n.get_mut(peer_id).ok_or(()))
            .ok()
            .map(MemoryPeerstoreWeightsAccess)
    }

    fn peer_or_create(self, peer_id: &PeerId) -> Self::PeerAccess {
        let lock = self.store.lock().unwrap();
        let r = OwningRefMut::new(lock)
            .map_mut(|n| n.entry(peer_id.clone()).or_insert_with(|| PeerInfo::new(0)));
        MemoryPeerstoreWeightsAccess(r)
    }

    fn random_peer(self) -> Option<(PeerId, Self::PeerAccess)> {
        let lock = self.store.lock().unwrap();
        let sum = lock.iter().fold(0u64, |a, v| a + *v.1.extra() as u64);
        if sum == 0 {
            return None;
        }

        // TODO: should be optimized to not iterate on the keys
        let mut num = rand::random::<u64>() % sum;
        let id = 'outer: loop {
            for elem in lock.iter() {
                let val = *elem.1.extra() as u64;
                if num <= val {
                    break 'outer elem.0.clone();
                } else {
                    num -= val;
                }
            }
        };

        let access = OwningRefMut::new(lock)
            .try_map_mut(|n| n.get_mut(&id).ok_or(()))
            .expect("mem peerstore entry is always valid");
        Some((id, MemoryPeerstoreWeightsAccess(access)))
    }

    fn peers(self) -> Self::PeersIter {
        let lock = self.store.lock().unwrap();
        lock.keys().cloned().collect::<Vec<_>>().into_iter()
    }
}

// Note: Rust doesn't provide a `MutexGuard::map` method, otherwise we could directly store a
//          `MutexGuard<'a, (&'a PeerId, &'a PeerInfo)>`.
pub struct MemoryPeerstoreWeightsAccess<'a>(
    OwningRefMut<MutexGuard<'a, HashMap<PeerId, PeerInfo<u32>>>, PeerInfo<u32>>,
);

impl<'a> PeerAccess for MemoryPeerstoreWeightsAccess<'a> {
    type AddrsIter = VecIntoIter<Multiaddr>;

    #[inline]
    fn addrs(&self) -> Self::AddrsIter {
        self.0.addrs().cloned().collect::<Vec<_>>().into_iter()
    }

    #[inline]
    fn add_addr(&mut self, addr: Multiaddr, ttl: TTL) {
        self.0
            .add_addr(addr, ttl, AddAddrBehaviour::IgnoreTtlIfInferior);
    }

    #[inline]
    fn set_addr_ttl(&mut self, addr: Multiaddr, ttl: TTL) {
        self.0.add_addr(addr, ttl, AddAddrBehaviour::OverwriteTtl);
    }

    #[inline]
    fn clear_addrs(&mut self) {
        self.0.set_addrs(iter::empty());
    }
}

impl<'a> PeerAccessWeights for MemoryPeerstoreWeightsAccess<'a> {
    #[inline]
    fn weight(&self) -> u32 {
        *self.0.extra()
    }

    #[inline]
    fn set_weight(&mut self, value: u32) {
        *self.0.extra_mut() = value;
    }
}

#[cfg(test)]
mod tests {
    peerstore_tests!({ ::memory_peerstore::MemoryPeerstoreWeights::empty() });
}
