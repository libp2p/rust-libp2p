// Copyright 2021 Protocol Labs.
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

mod util;

use futures::executor::block_on;
use futures::future::poll_fn;
use futures::ready;
use libp2p_core::{
    multiaddr::Protocol,
    network::{NetworkConfig, NetworkEvent},
    ConnectedPoint, DialOpts,
};
use quickcheck::*;
use rand07::Rng;
use std::num::NonZeroU8;
use std::task::Poll;
use util::{test_network, TestHandler};

#[test]
fn concurrent_dialing() {
    #[derive(Clone, Debug)]
    struct DialConcurrencyFactor(NonZeroU8);

    impl Arbitrary for DialConcurrencyFactor {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            Self(NonZeroU8::new(g.gen_range(1, 11)).unwrap())
        }
    }

    fn prop(concurrency_factor: DialConcurrencyFactor) {
        block_on(async {
            let mut network_1 = test_network(NetworkConfig::default());
            let mut network_2 = test_network(
                NetworkConfig::default().with_dial_concurrency_factor(concurrency_factor.0),
            );

            // Listen on `concurrency_factor + 1` addresses.
            //
            // `+ 1` to ensure a subset of addresses is dialed by network_2.
            let num_listen_addrs = concurrency_factor.0.get() + 2;
            let mut network_1_listen_addresses = Vec::new();
            for _ in 0..num_listen_addrs {
                network_1.listen_on("/memory/0".parse().unwrap()).unwrap();

                poll_fn(|cx| match ready!(network_1.poll(cx)) {
                    NetworkEvent::NewListenerAddress { listen_addr, .. } => {
                        network_1_listen_addresses.push(listen_addr);
                        return Poll::Ready(());
                    }
                    _ => panic!("Expected `NewListenerAddress` event."),
                })
                .await;
            }

            // Have network 2 dial network 1 and wait for network 1 to receive the incoming
            // connections.
            network_2
                .dial(
                    TestHandler(),
                    DialOpts::peer_id(*network_1.local_peer_id())
                        .addresses(network_1_listen_addresses.clone().into())
                        .build(),
                )
                .unwrap();
            let mut network_1_incoming_connections = Vec::new();
            for i in 0..concurrency_factor.0.get() {
                poll_fn(|cx| {
                    match network_2.poll(cx) {
                        Poll::Ready(e) => panic!("Unexpected event: {:?}", e),
                        Poll::Pending => {}
                    }

                    match ready!(network_1.poll(cx)) {
                        NetworkEvent::IncomingConnection { connection, .. } => {
                            assert_eq!(
                                connection.local_addr, network_1_listen_addresses[i as usize],
                                "Expect network 2 to prioritize by address order."
                            );

                            network_1_incoming_connections.push(connection);
                            return Poll::Ready(());
                        }
                        _ => panic!("Expected `NewListenerAddress` event."),
                    }
                })
                .await;
            }

            // Have network 1 accept the incoming connection and wait for network 1 and network 2 to
            // report a shared established connection.
            let accepted_addr = network_1_incoming_connections[0].local_addr.clone();
            network_1
                .accept(network_1_incoming_connections.remove(0), TestHandler())
                .unwrap();
            let mut network_1_connection_established = false;
            let mut network_2_connection_established = false;
            poll_fn(|cx| {
                match network_2.poll(cx) {
                    Poll::Ready(NetworkEvent::ConnectionEstablished {
                        connection,
                        concurrent_dial_errors,
                        ..
                    }) => {
                        match connection.endpoint() {
                            ConnectedPoint::Dialer { address, .. } => {
                                assert_eq!(
                                    *address,
                                    accepted_addr
                                        .clone()
                                        .with(Protocol::P2p((*network_1.local_peer_id()).into()))
                                )
                            }
                            ConnectedPoint::Listener { .. } => panic!("Expected dialer."),
                        }
                        assert!(concurrent_dial_errors.unwrap().is_empty());
                        network_2_connection_established = true;
                        if network_1_connection_established {
                            return Poll::Ready(());
                        }
                    }
                    Poll::Ready(e) => panic!("Expected `ConnectionEstablished` event: {:?}.", e),
                    Poll::Pending => {}
                }

                match ready!(network_1.poll(cx)) {
                    NetworkEvent::ConnectionEstablished {
                        connection,
                        concurrent_dial_errors,
                        ..
                    } => {
                        match connection.endpoint() {
                            ConnectedPoint::Listener { local_addr, .. } => {
                                assert_eq!(*local_addr, accepted_addr)
                            }
                            ConnectedPoint::Dialer { .. } => panic!("Expected listener."),
                        }
                        assert!(concurrent_dial_errors.is_none());
                        network_1_connection_established = true;
                        if network_2_connection_established {
                            return Poll::Ready(());
                        }
                    }
                    e => panic!("Expected `ConnectionEstablished` event: {:?}.", e),
                }

                Poll::Pending
            })
            .await;
        })
    }

    QuickCheck::new().quickcheck(prop as fn(_) -> _);
}
