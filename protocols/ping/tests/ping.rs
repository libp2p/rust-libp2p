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

use futures::prelude::*;
use libp2p_ping as ping;
use libp2p_swarm::keep_alive;
use libp2p_swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use quickcheck::*;
use std::{num::NonZeroU8, time::Duration};

#[test]
fn ping_pong() {
    fn prop(count: NonZeroU8) {
        let cfg = ping::Config::new().with_interval(Duration::from_millis(10));

        let mut swarm1 = Swarm::new_ephemeral(|_| Behaviour::new(cfg.clone()));
        let mut swarm2 = Swarm::new_ephemeral(|_| Behaviour::new(cfg.clone()));

        async_std::task::block_on(async {
            swarm1.listen().await;
            swarm2.connect(&mut swarm1).await;

            for _ in 0..count.get() {
                let (e1, e2) = match libp2p_swarm_test::drive(&mut swarm1, &mut swarm2).await {
                    ([BehaviourEvent::Ping(e1)], [BehaviourEvent::Ping(e2)]) => (e1, e2),
                    events => panic!("Unexpected events: {events:?}"),
                };

                assert_eq!(&e1.peer, swarm2.local_peer_id());
                assert_eq!(&e2.peer, swarm1.local_peer_id());

                assert_ping_rtt_less_than_50ms(e1);
                assert_ping_rtt_less_than_50ms(e2);
            }
        });
    }

    QuickCheck::new().tests(10).quickcheck(prop as fn(_))
}

fn assert_ping_rtt_less_than_50ms(e: ping::Event) {
    let success = e.result.expect("a ping success");

    if let ping::Success::Ping { rtt } = success {
        assert!(rtt < Duration::from_millis(50))
    }
}

/// Tests that the connection is closed upon a configurable
/// number of consecutive ping failures.
#[test]
fn max_failures() {
    fn prop(max_failures: NonZeroU8) {
        let cfg = ping::Config::new()
            .with_interval(Duration::from_millis(10))
            .with_timeout(Duration::from_millis(0))
            .with_max_failures(max_failures.into());

        let mut swarm1 = Swarm::new_ephemeral(|_| Behaviour::new(cfg.clone()));
        let mut swarm2 = Swarm::new_ephemeral(|_| Behaviour::new(cfg.clone()));

        let (count1, count2) = async_std::task::block_on(async {
            swarm1.listen().await;
            swarm2.connect(&mut swarm1).await;

            future::join(
                count_ping_failures_until_connection_closed(swarm1),
                count_ping_failures_until_connection_closed(swarm2),
            )
            .await
        });

        assert_eq!(u8::max(count1, count2), max_failures.get() - 1);
    }

    QuickCheck::new().tests(10).quickcheck(prop as fn(_))
}

async fn count_ping_failures_until_connection_closed(mut swarm: Swarm<Behaviour>) -> u8 {
    let mut failure_count = 0;

    loop {
        match swarm.next_swarm_event().await {
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                result: Ok(ping::Success::Ping { .. }),
                ..
            })) => {
                failure_count = 0; // there may be an occasional success
            }
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event { result: Err(_), .. })) => {
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
    let mut swarm1 = Swarm::new_ephemeral(|_| keep_alive::Behaviour);
    let mut swarm2 = Swarm::new_ephemeral(|_| Behaviour::new(ping::Config::new()));

    let result = async_std::task::block_on(async {
        swarm1.listen().await;
        swarm2.connect(&mut swarm1).await;
        let swarm1_peer_id = *swarm1.local_peer_id();
        async_std::task::spawn(swarm1.loop_on_next());

        loop {
            match swarm2.next_swarm_event().await {
                SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                    result: Err(ping::Failure::Unsupported),
                    ..
                })) => {
                    swarm2.disconnect_peer_id(swarm1_peer_id).unwrap();
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

#[derive(NetworkBehaviour, Default)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Behaviour {
    keep_alive: keep_alive::Behaviour,
    ping: ping::Behaviour,
}

impl Behaviour {
    fn new(config: ping::Config) -> Self {
        Self {
            keep_alive: keep_alive::Behaviour,
            ping: ping::Behaviour::new(config),
        }
    }
}
