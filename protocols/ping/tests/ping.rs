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

use std::{num::NonZeroU8, time::Duration};

use libp2p_ping as ping;
use libp2p_swarm::{dummy, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use quickcheck::*;

#[tokio::test]
async fn ping_pong() {
    fn prop(count: NonZeroU8) {
        let cfg = ping::Config::new().with_interval(Duration::from_millis(10));

        let mut swarm1 = Swarm::new_ephemeral(|_| ping::Behaviour::new(cfg.clone()));
        let mut swarm2 = Swarm::new_ephemeral(|_| ping::Behaviour::new(cfg.clone()));

        tokio::spawn(async move {
            swarm1.listen().with_memory_addr_external().await;
            swarm2.connect(&mut swarm1).await;

            for _ in 0..count.get() {
                let ([e1], [e2]): ([ping::Event; 1], [ping::Event; 1]) =
                    libp2p_swarm_test::drive(&mut swarm1, &mut swarm2).await;

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
    let rtt = e.result.expect("a ping success");

    assert!(rtt < Duration::from_millis(50))
}

#[tokio::test]
async fn unsupported_doesnt_fail() {
    let mut swarm1 = Swarm::new_ephemeral(|_| dummy::Behaviour);
    let mut swarm2 = Swarm::new_ephemeral(|_| ping::Behaviour::new(ping::Config::new()));

    let result = {
        swarm1.listen().with_memory_addr_external().await;
        swarm2.connect(&mut swarm1).await;
        let swarm1_peer_id = *swarm1.local_peer_id();
        tokio::spawn(swarm1.loop_on_next());

        loop {
            match swarm2.next_swarm_event().await {
                SwarmEvent::Behaviour(ping::Event {
                    result: Err(ping::Failure::Unsupported),
                    ..
                }) => {
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
    };

    result.expect("node with ping should not fail connection due to unsupported protocol");
}
