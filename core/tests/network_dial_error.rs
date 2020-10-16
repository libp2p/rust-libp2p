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
use libp2p_core::identity;
use libp2p_core::multiaddr::{multiaddr, Multiaddr};
use libp2p_core::{
    Network,
    PeerId,
    Transport,
    connection::PendingConnectionError,
    muxing::StreamMuxerBox,
    network::{NetworkEvent, NetworkConfig},
    transport,
    upgrade,
};
use libp2p_noise as noise;
use rand::Rng;
use rand::seq::SliceRandom;
use std::{io, task::Poll};
use util::TestHandler;

type TestNetwork = Network<TestTransport, (), (), TestHandler>;
type TestTransport = transport::Boxed<(PeerId, StreamMuxerBox)>;

fn new_network(cfg: NetworkConfig) -> TestNetwork {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let noise_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(&local_key).unwrap();
    let transport: TestTransport = libp2p_tcp::TcpConfig::new()
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(libp2p_mplex::MplexConfig::new())
        .boxed()
        .and_then(|(peer, mplex), _| {
            // Gracefully close the connection to allow protocol
            // negotiation to complete.
            util::CloseMuxer::new(mplex).map_ok(move |mplex| (peer, mplex))
        })
        .boxed();
    TestNetwork::new(transport, local_public_key.into(), cfg)
}

#[test]
fn deny_incoming_connec() {
    // Checks whether refusing an incoming connection on a swarm triggers the correct events.

    let mut swarm1 = new_network(NetworkConfig::default());
    let mut swarm2 = new_network(NetworkConfig::default());

    swarm1.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let address = async_std::task::block_on(future::poll_fn(|cx| {
        if let Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) = swarm1.poll(cx) {
            Poll::Ready(listen_addr)
        } else {
            panic!("Was expecting the listen address to be reported")
        }
    }));

    swarm2
        .peer(swarm1.local_peer_id().clone())
        .dial(address.clone(), Vec::new(), TestHandler())
        .unwrap();

    async_std::task::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        match swarm1.poll(cx) {
            Poll::Ready(NetworkEvent::IncomingConnection { connection, .. }) => drop(connection),
            Poll::Ready(_) => unreachable!(),
            Poll::Pending => (),
        }

        match swarm2.poll(cx) {
            Poll::Ready(NetworkEvent::DialError {
                attempts_remaining: 0,
                peer_id,
                multiaddr,
                error: PendingConnectionError::Transport(_)
            }) => {
                assert_eq!(peer_id, *swarm1.local_peer_id());
                assert_eq!(multiaddr, address);
                return Poll::Ready(Ok(()));
            },
            Poll::Ready(_) => unreachable!(),
            Poll::Pending => (),
        }

        Poll::Pending
    })).unwrap();
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

    let mut swarm = new_network(NetworkConfig::default());
    swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let (local_address, mut swarm) = async_std::task::block_on(
        future::lazy(move |cx| {
            if let Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) = swarm.poll(cx) {
                Ok::<_, void::Void>((listen_addr, swarm))
            } else {
                panic!("Was expecting the listen address to be reported")
            }
        }))
        .unwrap();

    swarm.dial(&local_address, TestHandler()).unwrap();

    let mut got_dial_err = false;
    let mut got_inc_err = false;
    async_std::task::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        loop {
            match swarm.poll(cx) {
                Poll::Ready(NetworkEvent::UnknownPeerDialError {
                    multiaddr,
                    error: PendingConnectionError::InvalidPeerId { .. },
                    ..
                }) => {
                    assert!(!got_dial_err);
                    assert_eq!(multiaddr, local_address);
                    got_dial_err = true;
                    if got_inc_err {
                        return Poll::Ready(Ok(()))
                    }
                },
                Poll::Ready(NetworkEvent::IncomingConnectionError {
                    local_addr, ..
                }) => {
                    assert!(!got_inc_err);
                    assert_eq!(local_addr, local_address);
                    got_inc_err = true;
                    if got_dial_err {
                       return Poll::Ready(Ok(()))
                    }
                },
                Poll::Ready(NetworkEvent::IncomingConnection { connection, .. }) => {
                    assert_eq!(&connection.local_addr, &local_address);
                    swarm.accept(connection, TestHandler()).unwrap();
                },
                Poll::Ready(ev) => {
                    panic!("Unexpected event: {:?}", ev)
                }
                Poll::Pending => break Poll::Pending,
            }
        }
    })).unwrap();
}

#[test]
fn dial_self_by_id() {
    // Trying to dial self by passing the same `PeerId` shouldn't even be possible in the first
    // place.
    let mut swarm = new_network(NetworkConfig::default());
    let peer_id = swarm.local_peer_id().clone();
    assert!(swarm.peer(peer_id).into_disconnected().is_none());
}

#[test]
fn multiple_addresses_err() {
    // Tries dialing multiple addresses, and makes sure there's one dialing error per address.

    let mut swarm = new_network(NetworkConfig::default());

    let mut addresses = Vec::new();
    for _ in 0 .. 3 {
        addresses.push(multiaddr![Ip4([0, 0, 0, 0]), Tcp(rand::random::<u16>())]);
    }
    for _ in 0 .. 5 {
        addresses.push(multiaddr![Udp(rand::random::<u16>())]);
    }
    addresses.shuffle(&mut rand::thread_rng());

    let first = addresses[0].clone();
    let rest = (&addresses[1..]).iter().cloned();

    let target = PeerId::random();
    swarm.peer(target.clone())
        .dial(first, rest, TestHandler())
        .unwrap();

    async_std::task::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        loop {
            match swarm.poll(cx) {
                Poll::Ready(NetworkEvent::DialError {
                    attempts_remaining,
                    peer_id,
                    multiaddr,
                    error: PendingConnectionError::Transport(_)
                }) => {
                    assert_eq!(peer_id, target);
                    let expected = addresses.remove(0);
                    assert_eq!(multiaddr, expected);
                    if addresses.is_empty() {
                        assert_eq!(attempts_remaining, 0);
                        return Poll::Ready(Ok(()));
                    } else {
                        assert_eq!(attempts_remaining, addresses.len() as u32);
                    }
                },
                Poll::Ready(_) => unreachable!(),
                Poll::Pending => break Poll::Pending,
            }
        }
    })).unwrap();
}

#[test]
fn connection_limit() {
    let outgoing_per_peer_limit = rand::thread_rng().gen_range(1, 10);
    let outgoing_limit = 2 * outgoing_per_peer_limit;

    let mut cfg = NetworkConfig::default();
    cfg.set_outgoing_per_peer_limit(outgoing_per_peer_limit);
    cfg.set_outgoing_limit(outgoing_limit);
    let mut network = new_network(cfg);

    let target = PeerId::random();
    for _ in 0 .. outgoing_per_peer_limit {
        network.peer(target.clone())
            .dial(Multiaddr::empty(), Vec::new(), TestHandler())
            .ok()
            .expect("Unexpected connection limit.");
    }

    let err = network.peer(target)
        .dial(Multiaddr::empty(), Vec::new(), TestHandler())
        .expect_err("Unexpected dialing success.");

    assert_eq!(err.current, outgoing_per_peer_limit);
    assert_eq!(err.limit, outgoing_per_peer_limit);

    let target2 = PeerId::random();
    for _ in outgoing_per_peer_limit .. outgoing_limit {
        network.peer(target2.clone())
            .dial(Multiaddr::empty(), Vec::new(), TestHandler())
            .ok()
            .expect("Unexpected connection limit.");
    }

    let err = network.peer(target2)
        .dial(Multiaddr::empty(), Vec::new(), TestHandler())
        .expect_err("Unexpected dialing success.");

    assert_eq!(err.current, outgoing_limit);
    assert_eq!(err.limit, outgoing_limit);
}
