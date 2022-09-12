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

extern crate core;

use futures::prelude::*;
use libp2p_core::{
    identity,
    muxing::StreamMuxerBox,
    transport::{self, Transport},
    upgrade, PeerId,
};
use libp2p_mplex as mplex;
use libp2p_noise as noise;
use libp2p_ping as ping;
use libp2p_swarm::{DummyBehaviour, KeepAlive, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use libp2p_tcp::{GenTcpConfig, TcpTransport};
use libp2p_yamux as yamux;
use quickcheck::*;
use rand::prelude::*;
use std::{num::NonZeroU8, time::Duration};

#[test]
fn ping_pong() {
    fn prop(count: NonZeroU8, muxer: MuxerChoice) {
        let cfg = ping::Config::new()
            .with_keep_alive(true)
            .with_interval(Duration::from_millis(10));

        let (peer1_id, trans) = mk_transport(muxer);
        let mut swarm1 = Swarm::new(trans, ping::Behaviour::new(cfg.clone()), peer1_id.clone());

        let (peer2_id, trans) = mk_transport(muxer);
        let mut swarm2 = Swarm::new(trans, ping::Behaviour::new(cfg), peer2_id.clone());

        async_std::task::block_on(async {
            swarm1.listen_on_random_localhost_tcp_port().await;
            swarm2.block_on_connection(&mut swarm1).await;

            for _ in 0..count.get() {
                let (e1, e2) =
                    futures::future::join(swarm1.next_within(5), swarm2.next_within(5)).await;

                let e1 = e1.try_into_behaviour_event().unwrap();
                let e2 = e2.try_into_behaviour_event().unwrap();

                assert_eq!(e1.peer, peer2_id);
                assert_eq!(e2.peer, peer1_id);

                let e1 = e1.result.expect("ping failure");
                let e2 = e2.result.expect("ping failure");

                match (e1, e2) {
                    (
                        ping::Success::Ping { rtt: peer1_rtt },
                        ping::Success::Ping { rtt: peer2_rtt },
                    ) => {
                        assert!(peer1_rtt < Duration::from_millis(50));
                        assert!(peer2_rtt < Duration::from_millis(50));
                    }
                    _ => {}
                }
            }
        });
    }

    QuickCheck::new().tests(10).quickcheck(prop as fn(_, _))
}

/// Tests that the connection is closed upon a configurable
/// number of consecutive ping failures.
#[test]
fn max_failures() {
    fn prop(max_failures: NonZeroU8, muxer: MuxerChoice) {
        let cfg = ping::Config::new()
            .with_keep_alive(true)
            .with_interval(Duration::from_millis(10))
            .with_timeout(Duration::from_millis(0))
            .with_max_failures(max_failures.into());

        let (peer1_id, trans) = mk_transport(muxer);
        let mut swarm1 = Swarm::new(trans, ping::Behaviour::new(cfg.clone()), peer1_id.clone());

        let (peer2_id, trans) = mk_transport(muxer);
        let mut swarm2 = Swarm::new(trans, ping::Behaviour::new(cfg), peer2_id.clone());

        async_std::task::block_on(async {
            swarm1.listen_on_random_localhost_tcp_port().await;
            swarm2.block_on_connection(&mut swarm1).await;
        });

        let future = future::join(
            count_consecutive_ping_failures_until_connection_closed(swarm1),
            count_consecutive_ping_failures_until_connection_closed(swarm2),
        );
        let (count1, count2) = async_std::task::block_on(future);
        assert_eq!(u8::max(count1, count2), max_failures.get() - 1);
    }

    QuickCheck::new().tests(10).quickcheck(prop as fn(_, _))
}

async fn count_consecutive_ping_failures_until_connection_closed(
    mut swarm: Swarm<ping::Behaviour>,
) -> u8 {
    let mut failure_count = 0;

    loop {
        match swarm.next_within(5).await {
            SwarmEvent::Behaviour(ping::Event {
                result: Ok(ping::Success::Ping { .. }),
                ..
            }) => {
                failure_count = 0; // there may be an occasional success
            }
            SwarmEvent::Behaviour(ping::Event { result: Err(_), .. }) => {
                failure_count += 1;
            }
            SwarmEvent::ConnectionClosed { .. } => {
                return failure_count;
            }
            _ => {}
        }
    }
}

#[test]
fn unsupported_doesnt_fail() {
    let (peer1_id, trans) = mk_transport(MuxerChoice::Mplex);
    let mut swarm1 = Swarm::new(
        trans,
        DummyBehaviour::with_keep_alive(KeepAlive::Yes),
        peer1_id.clone(),
    );

    let (peer2_id, trans) = mk_transport(MuxerChoice::Mplex);
    let mut swarm2 = Swarm::new(
        trans,
        ping::Behaviour::new(ping::Config::new().with_keep_alive(true)),
        peer2_id.clone(),
    );

    let result = async_std::task::block_on(async {
        swarm1.listen_on_random_localhost_tcp_port().await;
        swarm2.block_on_connection(&mut swarm1).await;

        loop {
            match swarm2.next_within(5).await {
                SwarmEvent::Behaviour(ping::Event {
                    result: Err(ping::Failure::Unsupported),
                    ..
                }) => {
                    swarm2.disconnect_peer_id(peer1_id).unwrap();
                }
                SwarmEvent::ConnectionClosed { cause: Some(e), .. } => {
                    break Err(e);
                }
                SwarmEvent::ConnectionClosed { cause: None, .. } => {
                    break Ok(());
                }
                _ => {}
            }
        }
    });

    result.expect("node with ping should not fail connection due to unsupported protocol");
}

fn mk_transport(muxer: MuxerChoice) -> (PeerId, transport::Boxed<(PeerId, StreamMuxerBox)>) {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().to_peer_id();
    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    (
        peer_id,
        TcpTransport::new(GenTcpConfig::default().nodelay(true))
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
            .multiplex(match muxer {
                MuxerChoice::Yamux => upgrade::EitherUpgrade::A(yamux::YamuxConfig::default()),
                MuxerChoice::Mplex => upgrade::EitherUpgrade::B(mplex::MplexConfig::default()),
            })
            .boxed(),
    )
}

#[derive(Debug, Copy, Clone)]
enum MuxerChoice {
    Mplex,
    Yamux,
}

impl Arbitrary for MuxerChoice {
    fn arbitrary<G: Gen>(g: &mut G) -> MuxerChoice {
        *[MuxerChoice::Mplex, MuxerChoice::Yamux].choose(g).unwrap()
    }
}
