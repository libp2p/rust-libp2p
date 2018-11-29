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

use kbucket::KBucketsPeerId;
use libp2p_core::{Multiaddr, PeerId, topology::MemoryTopology, topology::Topology};
use multihash::Multihash;
use protocol::KadConnectionType;
use std::vec;

/// Trait allowing retreival of information necessary for the Kadmelia system to work.
pub trait KademliaTopology: Topology {
    /// Iterator returned by `closest_peers`.
    type ClosestPeersIter: Iterator<Item = PeerId>;

    /// Iterator returned by `get_providers`.
    type GetProvidersIter: Iterator<Item = PeerId>;

    /// Adds an address discovered through Kademlia to the topology.
    fn add_kad_discovered_address(&mut self, peer: PeerId, addr: Multiaddr,
                                  connection_ty: KadConnectionType);

    /// Returns the known peers closest by XOR distance to the `target`.
    ///
    /// The `max` parameter is the maximum number of results that we are going to use. If more
    /// than `max` elements are returned, they will be ignored.
    fn closest_peers(&mut self, target: &Multihash, max: usize) -> Self::ClosestPeersIter;

    /// Registers the given peer as provider of the resource with the given ID.
    ///
    /// > **Note**: There is no `remove_provider` method. Implementations must include a
    /// >           time-to-live system so that entries disappear after a while.
    // TODO: specify the TTL? it has to match the timeout in the behaviour somehow, but this could
    //       also be handled by the user
    fn add_provider(&mut self, key: Multihash, peer_id: PeerId);

    /// Returns the list of providers that have been registered with `add_provider`.
    fn get_providers(&mut self, key: &Multihash) -> Self::GetProvidersIter;
}

// TODO: stupid idea to implement on `MemoryTopology`
impl KademliaTopology for MemoryTopology {
    type ClosestPeersIter = vec::IntoIter<PeerId>;
    type GetProvidersIter = vec::IntoIter<PeerId>;

    fn add_kad_discovered_address(&mut self, peer: PeerId, addr: Multiaddr, _: KadConnectionType) {
        self.add_address(peer, addr)
    }

    fn closest_peers(&mut self, target: &Multihash, _: usize) -> Self::ClosestPeersIter {
        let mut list = self.peers().cloned().collect::<Vec<_>>();
        list.sort_by(|a, b| target.distance_with(b.as_ref()).cmp(&target.distance_with(a.as_ref())));
        list.into_iter()
    }

    fn add_provider(&mut self, _: Multihash, _: PeerId) {
        unimplemented!()
    }

    fn get_providers(&mut self, _: &Multihash) -> Self::GetProvidersIter {
        unimplemented!()
    }
}
