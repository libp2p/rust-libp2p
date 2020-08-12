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

//! Integration tests for the `Ping` network behaviour.

use libp2p_core::{
    Multiaddr,
    PeerId,
    identity,
    muxing::StreamMuxerBox,
    transport::{Transport, boxed::Boxed},
    upgrade
};
use libp2p_ping::*;
use libp2p_secio::SecioConfig;
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_tcp::TcpConfig;
use futures::{prelude::*, channel::mpsc};
use quickcheck::*;
use std::{io, num::NonZeroU8, time::Duration};

#[test]
fn ping_pong() {
    fn prop(count: NonZeroU8) {
        let cfg = PingConfig::new()
            .with_keep_alive(true)
            .with_interval(Duration::from_millis(10));

        let (peer1_id, trans) = mk_transport();
        let mut swarm1 = Swarm::new(trans, Ping::new(cfg.clone()), peer1_id.clone());

        let (peer2_id, trans) = mk_transport();
        let mut swarm2 = Swarm::new(trans, Ping::new(cfg), peer2_id.clone());

        let (mut tx, mut rx) = mpsc::channel::<Multiaddr>(1);

        let pid1 = peer1_id.clone();
        let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        Swarm::listen_on(&mut swarm1, addr).unwrap();

        let mut count1 = count.get();
        let mut count2 = count.get();

        let peer1 = async move {
            while let Some(_) = swarm1.next().now_or_never() {}

            for l in Swarm::listeners(&swarm1) {
                tx.send(l.clone()).await.unwrap();
            }

            loop {
                match swarm1.next().await {
                    PingEvent { peer, result: Ok(PingSuccess::Ping { rtt }) } => {
                        count1 -= 1;
                        if count1 == 0 {
                            return (pid1.clone(), peer, rtt)
                        }
                    },
                    PingEvent { result: Err(e), .. } => panic!("Ping failure: {:?}", e),
                    _ => {}
                }
            }
        };

        let pid2 = peer2_id.clone();
        let peer2 = async move {
            Swarm::dial_addr(&mut swarm2, rx.next().await.unwrap()).unwrap();

            loop {
                match swarm2.next().await {
                    PingEvent { peer, result: Ok(PingSuccess::Ping { rtt }) } => {
                        count2 -= 1;
                        if count2 == 0 {
                            return (pid2.clone(), peer, rtt)
                        }
                    },
                    PingEvent { result: Err(e), .. } => panic!("Ping failure: {:?}", e),
                    _ => {}
                }
            }
        };

        let result = future::select(Box::pin(peer1), Box::pin(peer2));
        let ((p1, p2, rtt), _) = async_std::task::block_on(result).factor_first();
        assert!(p1 == peer1_id && p2 == peer2_id || p1 == peer2_id && p2 == peer1_id);
        assert!(rtt < Duration::from_millis(50));
    }

    QuickCheck::new().tests(3).quickcheck(prop as fn(_))
}


/// Tests that the connection is closed upon a configurable
/// number of consecutive ping failures.
#[test]
fn max_failures() {
    fn prop(max_failures: NonZeroU8) {
        let cfg = PingConfig::new()
            .with_keep_alive(true)
            .with_interval(Duration::from_millis(10))
            .with_timeout(Duration::from_millis(0))
            .with_max_failures(max_failures.into());

        let (peer1_id, trans) = mk_transport();
        let mut swarm1 = Swarm::new(trans, Ping::new(cfg.clone()), peer1_id.clone());

        let (peer2_id, trans) = mk_transport();
        let mut swarm2 = Swarm::new(trans, Ping::new(cfg), peer2_id.clone());

        let (mut tx, mut rx) = mpsc::channel::<Multiaddr>(1);

        let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        Swarm::listen_on(&mut swarm1, addr).unwrap();

        let peer1 = async move {
            while let Some(_) = swarm1.next().now_or_never() {}

            for l in Swarm::listeners(&swarm1) {
                tx.send(l.clone()).await.unwrap();
            }

            let mut count1: u8 = 0;

            loop {
                match swarm1.next_event().await {
                    SwarmEvent::Behaviour(PingEvent {
                        result: Ok(PingSuccess::Ping { .. }), ..
                    }) => {
                        count1 = 0; // there may be an occasional success
                    }
                    SwarmEvent::Behaviour(PingEvent {
                        result: Err(_), ..
                    }) => {
                        count1 += 1;
                    }
                    SwarmEvent::ConnectionClosed { .. } => {
                        return count1
                    }
                    _ => {}
                }
            }
        };

        let peer2 = async move {
            Swarm::dial_addr(&mut swarm2, rx.next().await.unwrap()).unwrap();

            let mut count2: u8 = 0;

            loop {
                match swarm2.next_event().await {
                    SwarmEvent::Behaviour(PingEvent {
                        result: Ok(PingSuccess::Ping { .. }), ..
                    }) => {
                        count2 = 0; // there may be an occasional success
                    }
                    SwarmEvent::Behaviour(PingEvent {
                        result: Err(_), ..
                    }) => {
                        count2 += 1;
                    }
                    SwarmEvent::ConnectionClosed { .. } => {
                        return count2
                    }
                    _ => {}
                }
            }
        };

        let future = future::join(peer1, peer2);
        let (count1, count2) = async_std::task::block_on(future);
        assert_eq!(u8::max(count1, count2), max_failures.get() - 1);
    }

    QuickCheck::new().tests(3).quickcheck(prop as fn(_))
}


fn mk_transport() -> (
    PeerId,
    Boxed<
        (PeerId, StreamMuxerBox),
        io::Error
    >
) {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().into_peer_id();
    let transport = TcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        .authenticate(SecioConfig::new(id_keys))
        .multiplex(libp2p_yamux::Config::default())
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        .boxed();
    (peer_id, transport)
}
