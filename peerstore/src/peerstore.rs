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

use multiaddr::Multiaddr;
use {PeerId, TTL};

/// Implemented on objects that store peers.
///
/// Note that the methods of this trait take by ownership (ie. `self` instead of `&self` or
/// `&mut self`). This was made so that the associated types could hold `self` internally and
/// because Rust doesn't have higher-ranked trait bounds yet.
///
/// Therefore this trait should likely be implemented on `&'a ConcretePeerstore` instead of
/// on `ConcretePeerstore`.
pub trait Peerstore {
    /// Grants access to the a peer inside the peer store.
    type PeerAccess: PeerAccess;
    /// List of the peers in this peer store.
    type PeersIter: Iterator<Item = PeerId>;

    /// Grants access to a peer by its ID.
    fn peer(self, peer_id: &PeerId) -> Option<Self::PeerAccess>;

    /// Grants access to a peer by its ID or creates it.
    fn peer_or_create(self, peer_id: &PeerId) -> Self::PeerAccess;

    /// Returns a list of peers in this peer store.
    ///
    /// Keep in mind that the trait implementation may allow new peers to be added or removed at
    /// any time. If that is the case, you have to take into account that this is only an
    /// indication.
    fn peers(self) -> Self::PeersIter;
}

/// Implemented on objects that represent an open access to a peer stored in a peer store.
///
/// The exact semantics of "open access" depend on the trait implementation.
pub trait PeerAccess {
    /// Iterator returned by `addrs`.
    type AddrsIter: Iterator<Item = Multiaddr>;

    /// Returns all known and non-expired addresses for a given peer.
    ///
    /// > **Note**: Keep in mind that this function is racy because addresses can expire between
    /// >           the moment when you get them and the moment when you process them.
    fn addrs(&self) -> Self::AddrsIter;

    /// Adds an address to a peer.
    ///
    /// If the manager already has this address stored and with a longer TTL, then the operation
    /// is a no-op.
    fn add_addr(&mut self, addr: Multiaddr, ttl: TTL);

    // Similar to calling `add_addr` multiple times in a row.
    #[inline]
    fn add_addrs<I>(&mut self, addrs: I, ttl: TTL)
    where
        I: IntoIterator<Item = Multiaddr>,
    {
        for addr in addrs.into_iter() {
            self.add_addr(addr, ttl);
        }
    }

    /// Sets the TTL of an address of a peer. Adds the address if it is currently unknown.
    ///
    /// Contrary to `add_addr`, this operation is never a no-op.
    #[inline]
    fn set_addr_ttl(&mut self, addr: Multiaddr, ttl: TTL);

    // Similar to calling `set_addr_ttl` multiple times in a row.
    fn set_addrs_ttl<I>(&mut self, addrs: I, ttl: TTL)
    where
        I: IntoIterator<Item = Multiaddr>,
    {
        for addr in addrs.into_iter() {
            self.add_addr(addr, ttl);
        }
    }

    /// Removes all previously stored addresses.
    fn clear_addrs(&mut self);
}
