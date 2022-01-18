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

mod util;

use futures::prelude::*;
use libp2p_core::multiaddr::multiaddr;
use libp2p_core::DialOpts;
use libp2p_core::{
    connection::PendingConnectionError,
    multiaddr::Protocol,
    network::{NetworkConfig, NetworkEvent},
    ConnectedPoint, Endpoint, PeerId,
};
use rand::seq::SliceRandom;
use std::{io, task::Poll};
use util::{test_network, TestHandler};

#[test]
fn deny_incoming_connec() {
    // Checks whether refusing an incoming connection on a swarm triggers the correct events.

    let mut swarm1 = test_network(NetworkConfig::default());
    let mut swarm2 = test_network(NetworkConfig::default());

    swarm1.listen_on("/memory/0".parse().unwrap()).unwrap();

    let address = futures::executor::block_on(future::poll_fn(|cx| match swarm1.poll(cx) {
        Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) => {
            Poll::Ready(listen_addr)
        }
        Poll::Pending => Poll::Pending,
        _ => panic!("Was expecting the listen address to be reported"),
    }));

    swarm2
        .dial(
            TestHandler(),
            DialOpts::peer_id(*swarm1.local_peer_id())
                .addresses(vec![address.clone()])
                .build(),
        )
        .unwrap();

    futures::executor::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        match swarm1.poll(cx) {
            Poll::Ready(NetworkEvent::IncomingConnection { connection, .. }) => drop(connection),
            Poll::Ready(_) => unreachable!(),
            Poll::Pending => (),
        }

        match swarm2.poll(cx) {
            Poll::Ready(NetworkEvent::DialError {
                peer_id,
                error: PendingConnectionError::Transport(errors),
                handler: _,
            }) => {
                assert_eq!(&peer_id, swarm1.local_peer_id());
                assert_eq!(
                    errors.get(0).expect("One error.").0,
                    address.clone().with(Protocol::P2p(peer_id.into()))
                );
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(_) => unreachable!(),
            Poll::Pending => (),
        }

        Poll::Pending
    }))
    .unwrap();
}

#[test]
fn invalid_peer_id() {
    // Checks whether dialing an address containing the wrong peer id raises an error
    // for the expected peer id instead of the obtained peer id.

    let mut swarm1 = test_network(NetworkConfig::default());
    let mut swarm2 = test_network(NetworkConfig::default());

    swarm1.listen_on("/memory/0".parse().unwrap()).unwrap();

    let address = futures::executor::block_on(future::poll_fn(|cx| match swarm1.poll(cx) {
        Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) => {
            Poll::Ready(listen_addr)
        }
        Poll::Pending => Poll::Pending,
        _ => panic!("Was expecting the listen address to be reported"),
    }));

    let other_id = PeerId::random();
    let other_addr = address.with(Protocol::P2p(other_id.into()));

    swarm2.dial(TestHandler(), other_addr.clone()).unwrap();

    let (peer_id, error) = futures::executor::block_on(future::poll_fn(|cx| {
        if let Poll::Ready(NetworkEvent::IncomingConnection { connection, .. }) = swarm1.poll(cx) {
            swarm1.accept(connection, TestHandler()).unwrap();
        }

        match swarm2.poll(cx) {
            Poll::Ready(NetworkEvent::DialError { peer_id, error, .. }) => {
                Poll::Ready((peer_id, error))
            }
            Poll::Ready(x) => panic!("unexpected {:?}", x),
            Poll::Pending => Poll::Pending,
        }
    }));
    assert_eq!(peer_id, other_id);
    match error {
        PendingConnectionError::WrongPeerId { obtained, endpoint } => {
            assert_eq!(obtained, *swarm1.local_peer_id());
            assert_eq!(
                endpoint,
                ConnectedPoint::Dialer {
                    address: other_addr,
                    role_override: Endpoint::Dialer,
                }
            );
        }
        x => panic!("wrong error {:?}", x),
    }
}

#[test]
fn dial_self() {
    // Check whether dialing ourselves correctly fails.
    //
    // Dialing the same address we're listening should result in three events:
    //
    // - The incoming connection notification (before we know the incoming peer ID).
    // - The connection error for the dialing endpoint (once we've determined that it's our own ID).
    // - The connection error for the listening endpoint (once we've determined that it's our own ID).
    //
    // The last two can happen in any order.

    let mut swarm = test_network(NetworkConfig::default());
    swarm.listen_on("/memory/0".parse().unwrap()).unwrap();

    let local_address = futures::executor::block_on(future::poll_fn(|cx| match swarm.poll(cx) {
        Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) => {
            Poll::Ready(listen_addr)
        }
        Poll::Pending => Poll::Pending,
        _ => panic!("Was expecting the listen address to be reported"),
    }));

    swarm.dial(TestHandler(), local_address.clone()).unwrap();

    let mut got_dial_err = false;
    let mut got_inc_err = false;
    futures::executor::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        loop {
            match swarm.poll(cx) {
                Poll::Ready(NetworkEvent::DialError {
                    peer_id,
                    error: PendingConnectionError::WrongPeerId { .. },
                    ..
                }) => {
                    assert_eq!(&peer_id, swarm.local_peer_id());
                    assert!(!got_dial_err);
                    got_dial_err = true;
                    if got_inc_err {
                        return Poll::Ready(Ok(()));
                    }
                }
                Poll::Ready(NetworkEvent::IncomingConnectionError { local_addr, .. }) => {
                    assert!(!got_inc_err);
                    assert_eq!(local_addr, local_address);
                    got_inc_err = true;
                    if got_dial_err {
                        return Poll::Ready(Ok(()));
                    }
                }
                Poll::Ready(NetworkEvent::IncomingConnection { connection, .. }) => {
                    assert_eq!(&connection.local_addr, &local_address);
                    swarm.accept(connection, TestHandler()).unwrap();
                }
                Poll::Ready(ev) => {
                    panic!("Unexpected event: {:?}", ev)
                }
                Poll::Pending => break Poll::Pending,
            }
        }
    }))
    .unwrap();
}

#[test]
fn dial_self_by_id() {
    // Trying to dial self by passing the same `PeerId` shouldn't even be possible in the first
    // place.
    let mut swarm = test_network(NetworkConfig::default());
    let peer_id = *swarm.local_peer_id();
    assert!(swarm.peer(peer_id).into_disconnected().is_none());
}

#[test]
fn multiple_addresses_err() {
    // Tries dialing multiple addresses, and makes sure there's one dialing error per address.

    let target = PeerId::random();

    let mut swarm = test_network(NetworkConfig::default());

    let mut addresses = Vec::new();
    for _ in 0..3 {
        addresses.push(multiaddr![Ip4([0, 0, 0, 0]), Tcp(rand::random::<u16>())]);
    }
    for _ in 0..5 {
        addresses.push(multiaddr![Udp(rand::random::<u16>())]);
    }
    addresses.shuffle(&mut rand::thread_rng());

    swarm
        .dial(
            TestHandler(),
            DialOpts::peer_id(target)
                .addresses(addresses.clone())
                .build(),
        )
        .unwrap();

    futures::executor::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        loop {
            match swarm.poll(cx) {
                Poll::Ready(NetworkEvent::DialError {
                    peer_id,
                    // multiaddr,
                    error: PendingConnectionError::Transport(errors),
                    handler: _,
                }) => {
                    assert_eq!(peer_id, target);

                    let failed_addresses =
                        errors.into_iter().map(|(addr, _)| addr).collect::<Vec<_>>();
                    assert_eq!(
                        failed_addresses,
                        addresses
                            .clone()
                            .into_iter()
                            .map(|addr| addr.with(Protocol::P2p(target.into())))
                            .collect::<Vec<_>>()
                    );

                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(_) => unreachable!(),
                Poll::Pending => break Poll::Pending,
            }
        }
    }))
    .unwrap();
}
