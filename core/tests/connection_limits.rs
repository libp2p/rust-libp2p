// Copyright 2020 Parity Technologies (UK) Ltd.
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

use futures::{future::poll_fn, ready};
use libp2p_core::multiaddr::{multiaddr, Multiaddr};
use libp2p_core::{
    connection::PendingConnectionError,
    network::{ConnectionLimits, DialError, NetworkConfig, NetworkEvent},
    PeerId,
};
use quickcheck::*;
use rand::Rng;
use std::task::Poll;
use util::{test_network, TestHandler};

#[test]
fn max_outgoing() {
    let outgoing_limit = rand::thread_rng().gen_range(1, 10);

    let limits = ConnectionLimits::default().with_max_pending_outgoing(Some(outgoing_limit));
    let cfg = NetworkConfig::default().with_connection_limits(limits);
    let mut network = test_network(cfg);

    let addr: Multiaddr = "/memory/1234".parse().unwrap();

    let target = PeerId::random();
    for _ in 0..outgoing_limit {
        network
            .peer(target.clone())
            .dial(vec![addr.clone()], TestHandler())
            .ok()
            .expect("Unexpected connection limit.");
    }

    match network
        .peer(target.clone())
        .dial(vec![addr.clone()], TestHandler())
        .expect_err("Unexpected dialing success.")
    {
        DialError::ConnectionLimit { limit, handler: _ } => {
            assert_eq!(limit.current, outgoing_limit);
            assert_eq!(limit.limit, outgoing_limit);
        }
        e => panic!("Unexpected error: {:?}", e),
    }

    let info = network.info();
    assert_eq!(info.num_peers(), 0);
    assert_eq!(
        info.connection_counters().num_pending_outgoing(),
        outgoing_limit
    );

    // Abort all dialing attempts.
    let mut peer = network
        .peer(target.clone())
        .into_dialing()
        .expect("Unexpected peer state");

    let mut attempts = peer.attempts();
    while let Some(attempt) = attempts.next() {
        attempt.abort();
    }

    assert_eq!(
        network.info().connection_counters().num_pending_outgoing(),
        0
    );
}

#[test]
fn max_established_incoming() {
    #[derive(Debug, Clone)]
    struct Limit(u32);

    impl Arbitrary for Limit {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            Self(g.gen_range(1, 10))
        }
    }

    fn config(limit: u32) -> NetworkConfig {
        let limits = ConnectionLimits::default().with_max_established_incoming(Some(limit));
        NetworkConfig::default().with_connection_limits(limits)
    }

    fn prop(limit: Limit) {
        let limit = limit.0;

        let mut network1 = test_network(config(limit));
        let mut network2 = test_network(config(limit));

        let _ = network1.listen_on(multiaddr![Memory(0u64)]).unwrap();
        let listen_addr =
            async_std::task::block_on(poll_fn(|cx| match ready!(network1.poll(cx)) {
                NetworkEvent::NewListenerAddress { listen_addr, .. } => Poll::Ready(listen_addr),
                e => panic!("Unexpected network event: {:?}", e),
            }));

        // Spawn and block on the dialer.
        async_std::task::block_on({
            let mut n = 0;
            let _ = network2.dial(&listen_addr, TestHandler()).unwrap();

            let mut expected_closed = false;
            let mut network_1_established = false;
            let mut network_2_established = false;
            let mut network_1_limit_reached = false;
            let mut network_2_limit_reached = false;
            poll_fn(move |cx| {
                loop {
                    let mut network_1_pending = false;
                    let mut network_2_pending = false;

                    match network1.poll(cx) {
                        Poll::Ready(NetworkEvent::IncomingConnection { connection, .. }) => {
                            network1.accept(connection, TestHandler()).unwrap();
                        }
                        Poll::Ready(NetworkEvent::ConnectionEstablished { .. }) => {
                            network_1_established = true;
                        }
                        Poll::Ready(NetworkEvent::IncomingConnectionError {
                            error: PendingConnectionError::ConnectionLimit(err),
                            ..
                        }) => {
                            assert_eq!(err.limit, limit);
                            assert_eq!(err.limit, err.current);
                            let info = network1.info();
                            let counters = info.connection_counters();
                            assert_eq!(counters.num_established_incoming(), limit);
                            assert_eq!(counters.num_established(), limit);
                            network_1_limit_reached = true;
                        }
                        Poll::Pending => {
                            network_1_pending = true;
                        }
                        e => panic!("Unexpected network event: {:?}", e),
                    }

                    match network2.poll(cx) {
                        Poll::Ready(NetworkEvent::ConnectionEstablished { .. }) => {
                            network_2_established = true;
                        }
                        Poll::Ready(NetworkEvent::ConnectionClosed { .. }) => {
                            assert!(expected_closed);
                            let info = network2.info();
                            let counters = info.connection_counters();
                            assert_eq!(counters.num_established_outgoing(), limit);
                            assert_eq!(counters.num_established(), limit);
                            network_2_limit_reached = true;
                        }
                        Poll::Pending => {
                            network_2_pending = true;
                        }
                        e => panic!("Unexpected network event: {:?}", e),
                    }

                    if network_1_pending && network_2_pending {
                        return Poll::Pending;
                    }

                    if network_1_established && network_2_established {
                        network_1_established = false;
                        network_2_established = false;

                        if n <= limit {
                            // Dial again until the limit is exceeded.
                            n += 1;
                            network2.dial(&listen_addr, TestHandler()).unwrap();

                            if n == limit {
                                // The the next dialing attempt exceeds the limit, this
                                // is the connection we expected to get closed.
                                expected_closed = true;
                            }
                        } else {
                            panic!("Expect networks not to establish connections beyond the limit.")
                        }
                    }

                    if network_1_limit_reached && network_2_limit_reached {
                        return Poll::Ready(());
                    }
                }
            })
        });
    }

    quickcheck(prop as fn(_));
}
