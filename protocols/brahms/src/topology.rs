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

use libp2p_core::{topology::MemoryTopology, topology::Topology, Multiaddr, PeerId};
use std::vec;

/// Trait that must be implemented on a topology for it to be usable with `Brahms`.
pub trait BrahmsTopology: Topology {
    /// Iterator returned by `initial_view`.
    type InitialViewIter: Iterator<Item = PeerId>;

    /// Adds an address discovered through Brahms to the topology.
    ///
    /// > **Warning**: Keep in mind that the peer id and address are untrusted. They don't
    /// >              necessarily match, and in fact don't necessarily correspond to anything
    /// >              that is guaranteed to exist.
    ///
    /// > **Note**: The value of `peer` can never be the ID of the local peer.
    fn add_brahms_discovered_address(&mut self, peer: PeerId, addr: Multiaddr);

    /// Returns the initial view of the network. `max` is the maximum number of elements that the
    /// iterator has to return. The method can return more entries, but any additional element
    /// will be ignored.
    ///
    /// The list of nodes in the initial view is pretty important for the safety of the network,
    /// as all the nodes that are discovered by Brahms afterwards will be derived from this initial
    /// view. In other words, if all the nodes of the initial view are malicious, then Brahms may
    /// only ever discover malicious nodes. However Brahms is designed so that a single
    /// well-behaving node is enough for the entire network of well-behaving nodes to be
    /// discovered.
    fn initial_view(&self, max: usize) -> Self::InitialViewIter;
}

impl BrahmsTopology for MemoryTopology {
    type InitialViewIter = vec::IntoIter<PeerId>;

    #[inline]
    fn add_brahms_discovered_address(&mut self, peer: PeerId, addr: Multiaddr) {
        self.add_address(peer, addr)
    }

    #[inline]
    fn initial_view(&self, max: usize) -> Self::InitialViewIter {
        // TODO: type of the the iterator returned by `peers()` cannot be expressed because Rust
        //       doesn't have the `type T = impl Trait;` syntax yet, so we collect into a Vec,
        //       which is inefficient
        self.peers()
            .cloned()
            .take(max)
            .collect::<Vec<_>>()
            .into_iter()
    }
}
