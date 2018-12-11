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

use libp2p_core::{topology::MemoryTopology, Multiaddr, PeerId};

/// Trait required on the topology for the identify system to store addresses.
pub trait IdentifyTopology {
    /// Adds to the topology an address discovered through identification.
    fn add_identify_discovered_addrs<TIter>(&mut self, peer: &PeerId, addr: TIter)
    where
        TIter: Iterator<Item = Multiaddr>;
}

impl IdentifyTopology for MemoryTopology {
    fn add_identify_discovered_addrs<TIter>(&mut self, peer: &PeerId, addr: TIter)
    where
        TIter: Iterator<Item = Multiaddr>,
    {
        for addr in addr {
            self.add_address(peer.clone(), addr);
        }
    }
}
