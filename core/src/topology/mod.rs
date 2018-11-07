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
use {Multiaddr, PeerId};

/// Storage for the network topology.
pub trait Topology {
    /// Adds a discovered address to the topology.
    fn add_discovered_address(&mut self, peer: &PeerId, addr: Multiaddr);
    /// Returns the addresses to try use to reach the given peer.
    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr>;
}

/// Topology of the network stored in memory.
pub struct MemoryTopology {
    list: HashMap<PeerId, Vec<Multiaddr>>,
}

impl MemoryTopology {
    /// Creates an empty topology.
    #[inline]
    pub fn empty() -> MemoryTopology {
        MemoryTopology {
            list: Default::default()
        }
    }
}

impl Default for MemoryTopology {
    #[inline]
    fn default() -> MemoryTopology {
        MemoryTopology::empty()
    }
}

impl Topology for MemoryTopology {
    fn add_discovered_address(&mut self, peer: &PeerId, addr: Multiaddr) {
        self.list.entry(peer.clone()).or_insert_with(|| Vec::new()).push(addr);
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.list.get(peer).map(|v| v.clone()).unwrap_or(Vec::new())
    }
}
