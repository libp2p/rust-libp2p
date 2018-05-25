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

//! This module contains a wrapper around `Peerstore` that implements the identify and kademlia
//! interfaces.

use std::ops::Deref;
use std::time::Duration;
use std::vec::IntoIter as VecIntoIter;
use core::{Multiaddr, PeerId};
use identify::{IdentifyInfo, IdentifyPeerInterface};
use kad::KademliaPeerInterface;
use peerstore::{Peerstore, PeerAccess};

#[derive(Clone)]
pub struct PeerstoreWrapper<PRef>(pub PRef);

impl<PRef, PStore> KademliaPeerInterface for PeerstoreWrapper<PRef>
where PRef: Deref<Target = PStore>,
      for<'r> &'r PStore: Peerstore,
{
    type AddrsIter = VecIntoIter<Multiaddr>;
    type PeersIter = VecIntoIter<PeerId>;

    fn add_addrs<I>(&self, peer: &PeerId, multiaddrs: I)
        where I: Iterator<Item = Multiaddr>
    {
        self.0.peer_or_create(peer)
            .add_addrs(multiaddrs, Duration::from_secs(3600));
    }

    fn peer_addrs(&self, peer_id: &PeerId) -> Self::AddrsIter {
        match self.0.peer(peer_id) {
            Some(peer) => peer.addrs().collect::<Vec<_>>().into_iter(),
            None => Vec::new().into_iter(),
        }
    }

    fn peers(&self) -> Self::PeersIter {
        self.0.peers().collect::<Vec<_>>().into_iter()
    }
}

impl<PRef, PStore> IdentifyPeerInterface for PeerstoreWrapper<PRef>
where PRef: Deref<Target = PStore>,
      for<'r> &'r PStore: Peerstore,
{
    type Iter = VecIntoIter<Multiaddr>;

    fn store_info(&self, info: &IdentifyInfo, multiaddr: Multiaddr) {
        let peer_id = PeerId::from_public_key(&info.public_key);
        self.0.peer_or_create(&peer_id)
            .add_addr(multiaddr, Duration::from_secs(3600));
    }

    fn peer_id_by_multiaddress(&self, multiaddr: &Multiaddr) -> Option<PeerId> {
        for peer_id in self.0.peers() {
            let peer = match self.0.peer(&peer_id) {
                Some(p) => p,
                None => continue,
            };

            if peer.addrs().any(|addr| &addr == multiaddr) {
                return Some(peer_id);
            }
        }

        None
    }

    fn multiaddresses_by_peer_id(&self, peer_id: PeerId) -> Self::Iter {
        match self.0.peer(&peer_id) {
            Some(peer) => peer.addrs().collect::<Vec<_>>().into_iter(),
            None => Vec::new().into_iter(),
        }
    }
}
