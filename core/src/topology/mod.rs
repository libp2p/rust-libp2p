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

use std::collections::HashMap;
use {Multiaddr, PeerId, PublicKey};

/// Storage for the network topology.
///
/// The topology should also store information about the local node, including its public key, its
/// `PeerId`, and the addresses it's advertising.
pub trait Topology {
    /// Returns the addresses to try use to reach the given peer.
    ///
    /// > **Note**: Keep in mind that `peer` can be the local node.
    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr>;

    /// Adds an address that other nodes can use to connect to our local node.
    ///
    /// > **Note**: Should later be returned when calling `addresses_of_peer()` with the `PeerId`
    /// >           of the local node.
    fn add_local_external_addrs<TIter>(&mut self, addrs: TIter)
    where TIter: Iterator<Item = Multiaddr>;

    /// Returns the `PeerId` of the local node.
    fn local_peer_id(&self) -> &PeerId;

    /// Returns the public key of the local node.
    fn local_public_key(&self) -> &PublicKey;
}

/// Topology of the network stored in memory.
pub struct MemoryTopology {
    list: HashMap<PeerId, Vec<Multiaddr>>,
    local_peer_id: PeerId,
    local_public_key: PublicKey,
}

impl MemoryTopology {
    /// Creates an empty topology.
    #[inline]
    pub fn empty(pubkey: PublicKey) -> MemoryTopology {
        let local_peer_id = pubkey.clone().into_peer_id();

        MemoryTopology {
            list: Default::default(),
            local_peer_id,
            local_public_key: pubkey,
        }
    }

    /// Returns true if the topology is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Adds an address to the topology.
    #[inline]
    pub fn add_address(&mut self, peer: PeerId, addr: Multiaddr) {
        let addrs = self.list.entry(peer).or_insert_with(|| Vec::new());
        if addrs.iter().all(|a| a != &addr) {
            addrs.push(addr);
        }
    }

    /// Returns a list of all the known peers in the topology.
    #[inline]
    pub fn peers(&self) -> impl Iterator<Item = &PeerId> {
        self.list.keys()
    }

    /// Returns an iterator to all the entries in the topology.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&PeerId, &Multiaddr)> {
        self.list.iter().flat_map(|(p, l)| l.iter().map(move |ma| (p, ma)))
    }
}

impl Topology for MemoryTopology {
    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.list.get(peer).map(|v| v.clone()).unwrap_or(Vec::new())
    }

    fn add_local_external_addrs<TIter>(&mut self, addrs: TIter)
    where TIter: Iterator<Item = Multiaddr>
    {
        for addr in addrs {
            let id = self.local_peer_id.clone();
            self.add_address(id, addr);
        }
    }

    #[inline]
    fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    #[inline]
    fn local_public_key(&self) -> &PublicKey {
        &self.local_public_key
    }
}
