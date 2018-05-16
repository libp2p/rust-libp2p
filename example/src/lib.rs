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

extern crate libp2p_peerstore;
extern crate libp2p_core;
extern crate multiaddr;

use libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
use multiaddr::Multiaddr;
use std::time::Duration;

/// Stores initial addresses on the given peer store. Uses a very large timeout.
pub fn ipfs_bootstrap<P>(peer_store: P)
where
    P: Peerstore + Clone,
{
    const ADDRESSES: &[&str] = &[
        "/ip4/127.0.0.1/tcp/4001/ipfs/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ",
        // TODO: add some bootstrap nodes here
    ];

    let ttl = Duration::from_secs(100 * 365 * 24 * 3600);

    for address in ADDRESSES.iter() {
        let mut multiaddr = address
            .parse::<Multiaddr>()
            .expect("failed to parse hard-coded multiaddr");

        let ipfs_component = multiaddr.pop().expect("hard-coded multiaddr is empty");
        let peer = match ipfs_component {
            multiaddr::AddrComponent::IPFS(key) => {
                PeerId::from_bytes(key).expect("invalid peer id")
            }
            _ => panic!("hard-coded multiaddr didn't end with /ipfs/"),
        };

        peer_store
            .clone()
            .peer_or_create(&peer)
            .add_addr(multiaddr, ttl.clone());
    }
}
