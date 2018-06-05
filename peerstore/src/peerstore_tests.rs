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

//! Contains the `peerstore_tests!` macro which will test an implementation of `Peerstore`.
//! You need to pass as first parameter a way to create the `Peerstore` that you want to test.
//!
//! You can also pass as additional parameters a list of statements that will be inserted before
//! each test. This allows you to have the peerstore builder use variables created by these
//! statements.

#![cfg(test)]

macro_rules! peerstore_tests {
    ({$create_peerstore:expr} $({$stmt:stmt})*) => {
        extern crate multihash;
        use std::thread;
        use std::time::Duration;
        use {Peerstore, PeerAccess, PeerId};
        use libp2p_core::PublicKeyBytesSlice;
        use multiaddr::Multiaddr;

        #[test]
        fn initially_empty() {
            $($stmt;)*
            let peer_store = $create_peerstore;
            let peer_id = PeerId::from_public_key(PublicKeyBytesSlice(&[1, 2, 3]));
            assert_eq!(peer_store.peers().count(), 0);
            assert!(peer_store.peer(&peer_id).is_none());
        }

        #[test]
        fn set_then_get_addr() {
            $($stmt;)*
            let peer_store = $create_peerstore;
            let peer_id = PeerId::from_public_key(PublicKeyBytesSlice(&[1, 2, 3]));
            let addr = "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>().unwrap();

            peer_store.peer_or_create(&peer_id).add_addr(addr.clone(), Duration::from_millis(5000));

            let addrs = peer_store.peer(&peer_id).unwrap().addrs().collect::<Vec<_>>();
            assert_eq!(addrs, &[addr]);
        }

        #[test]
        fn add_expired_addr() {
            // Add an already-expired address to a peer.
            $($stmt;)*
            let peer_store = $create_peerstore;
            let peer_id = PeerId::from_public_key(PublicKeyBytesSlice(&[1, 2, 3]));
            let addr = "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>().unwrap();

            peer_store.peer_or_create(&peer_id).add_addr(addr.clone(), Duration::from_millis(0));
            thread::sleep(Duration::from_millis(2));

            let addrs = peer_store.peer(&peer_id).unwrap().addrs();
            assert_eq!(addrs.count(), 0);
        }

        #[test]
        fn clear_addrs() {
            $($stmt;)*
            let peer_store = $create_peerstore;
            let peer_id = PeerId::from_public_key(PublicKeyBytesSlice(&[1, 2, 3]));
            let addr = "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>().unwrap();

            peer_store.peer_or_create(&peer_id)
                .add_addr(addr.clone(), Duration::from_millis(5000));
            peer_store.peer(&peer_id).unwrap().clear_addrs();

            let addrs = peer_store.peer(&peer_id).unwrap().addrs();
            assert_eq!(addrs.count(), 0);
        }

        #[test]
        fn no_update_ttl() {
            $($stmt;)*
            let peer_store = $create_peerstore;
            let peer_id = PeerId::from_public_key(PublicKeyBytesSlice(&[1, 2, 3]));

            let addr1 = "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>().unwrap();
            let addr2 = "/ip4/0.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();

            peer_store.peer_or_create(&peer_id)
                .add_addr(addr1.clone(), Duration::from_millis(5000));
            peer_store.peer_or_create(&peer_id)
                .add_addr(addr2.clone(), Duration::from_millis(5000));
            assert_eq!(peer_store.peer(&peer_id).unwrap().addrs().count(), 2);

            // `add_addr` must not overwrite the TTL because it's already higher
            peer_store.peer_or_create(&peer_id).add_addr(addr1.clone(), Duration::from_millis(0));
            thread::sleep(Duration::from_millis(2));
            assert_eq!(peer_store.peer(&peer_id).unwrap().addrs().count(), 2);
        }

        #[test]
        fn force_update_ttl() {
            $($stmt;)*
            let peer_store = $create_peerstore;
            let peer_id = PeerId::from_public_key(PublicKeyBytesSlice(&[1, 2, 3]));

            let addr1 = "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>().unwrap();
            let addr2 = "/ip4/0.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();

            peer_store.peer_or_create(&peer_id)
                .add_addr(addr1.clone(), Duration::from_millis(5000));
            peer_store.peer_or_create(&peer_id)
                .add_addr(addr2.clone(), Duration::from_millis(5000));
            assert_eq!(peer_store.peer(&peer_id).unwrap().addrs().count(), 2);

            peer_store.peer_or_create(&peer_id)
                .set_addr_ttl(addr1.clone(), Duration::from_millis(0));
            thread::sleep(Duration::from_millis(2));
            assert_eq!(peer_store.peer(&peer_id).unwrap().addrs().count(), 1);
        }
    };
}
