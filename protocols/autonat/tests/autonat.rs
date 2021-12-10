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

use futures::{channel::oneshot, FutureExt, StreamExt};
use libp2p::{
    development_transport,
    identity::Keypair,
    swarm::{AddressScore, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_autonat::{Behaviour, Config, Event, NatStatus};
use std::time::Duration;

const SERVER_COUNT: usize = 5;

async fn init_swarm(config: Config) -> Swarm<Behaviour> {
    let keypair = Keypair::generate_ed25519();
    let local_id = PeerId::from_public_key(&keypair.public());
    let transport = development_transport(keypair).await.unwrap();
    let behaviour = Behaviour::new(local_id, config);
    Swarm::new(transport, behaviour, local_id)
}

async fn spawn_server(kill: oneshot::Receiver<()>) -> (PeerId, Multiaddr) {
    let (tx, rx) = oneshot::channel();
    async_std::task::spawn(async move {
        let mut swarm = init_swarm(Config {
            boot_delay: Duration::from_secs(60),
            ..Default::default()
        })
        .await;
        let peer_id = *swarm.local_peer_id();
        swarm
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();
        let addr = loop {
            match swarm.select_next_some().await {
                SwarmEvent::NewListenAddr { address, .. } => break address,
                _ => {}
            };
        };
        tx.send((peer_id, addr)).unwrap();
        let mut kill = kill.fuse();
        loop {
            futures::select! {
                _ = swarm.select_next_some() => {},
                _ = kill => return,

            }
        }
    });
    rx.await.unwrap()
}

#[async_std::test]
async fn test_auto_probe() {
    let mut client = init_swarm(Config {
        retry_interval: Duration::from_millis(100),
        throttle_peer_period: Duration::from_millis(99),
        boot_delay: Duration::ZERO,
        ..Default::default()
    })
    .await;

    let (_handle, rx) = oneshot::channel();
    let (id, addr) = spawn_server(rx).await;
    client.behaviour_mut().add_server(id, Some(addr));

    // Probe should directly resolve to `Unknown` as the local peer has no listening addresses
    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(Event {
                nat_status,
                confidence,
                has_flipped,
                ..
            }) => {
                assert_eq!(nat_status, NatStatus::Unknown);
                assert_eq!(client.behaviour().nat_status(), nat_status);
                assert!(client.behaviour().public_address().is_none());
                assert_eq!(confidence, 0);
                assert_eq!(client.behaviour().confidence(), confidence);
                assert!(!has_flipped);
                break;
            }
            _ => {}
        }
    }

    // Artificially add a faulty address.
    let unreachable_addr: Multiaddr = "/ip4/127.0.0.1/tcp/42".parse().unwrap();
    client.add_external_address(unreachable_addr.clone(), AddressScore::Infinite);

    // Auto-Probe should resolve to private since the server can not reach us.
    // Confidence should increase with each iteration
    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(Event {
                nat_status,
                has_flipped,
                confidence,
                ..
            }) => {
                assert_eq!(nat_status, NatStatus::Private);
                assert_eq!(client.behaviour().nat_status(), nat_status);
                assert!(client.behaviour().public_address().is_none());
                assert_eq!(confidence, 0);
                assert_eq!(client.behaviour().confidence(), confidence);
                assert!(has_flipped);
                break;
            }
            _ => {}
        }
    }
    client.remove_external_address(&unreachable_addr);

    client
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();
    loop {
        match client.select_next_some().await {
            SwarmEvent::NewListenAddr { .. } => break,
            _ => {}
        }
    }

    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(Event {
                nat_status,
                confidence,
                has_flipped,
                ..
            }) => {
                assert!(matches!(nat_status, NatStatus::Public(_)));
                assert_eq!(client.behaviour().nat_status(), nat_status);
                assert_eq!(confidence, 0);
                assert_eq!(client.behaviour().confidence(), confidence);
                assert!(client.behaviour().public_address().is_some());
                assert!(has_flipped);
                break;
            }
            _ => {}
        }
    }

    drop(_handle);
}

#[async_std::test]
async fn test_manual_probe() {
    let mut handles = Vec::new();

    let mut client = init_swarm(Config {
        retry_interval: Duration::from_secs(60),
        throttle_peer_period: Duration::from_millis(100),
        ..Default::default()
    })
    .await;

    for _ in 0..SERVER_COUNT {
        let (tx, rx) = oneshot::channel();
        let (id, addr) = spawn_server(rx).await;
        client.behaviour_mut().add_server(id, Some(addr));
        handles.push(tx);
    }

    client
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();
    loop {
        match client.select_next_some().await {
            SwarmEvent::NewListenAddr { .. } => break,
            _ => {}
        }
    }

    let mut probes = Vec::new();

    for _ in 0..SERVER_COUNT {
        let probe_id = client.behaviour_mut().trigger_probe(None).unwrap();
        probes.push(probe_id);
    }

    // Expect None because of peer throttling
    assert!(client.behaviour_mut().trigger_probe(None).is_none());

    let mut curr_confidence = 0;
    let mut expect_flipped = true;
    let mut i = 0;
    while i < SERVER_COUNT {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(Event {
                nat_status,
                probe_id,
                confidence,
                has_flipped,
            }) => {
                assert_eq!(expect_flipped, has_flipped);
                expect_flipped = false;
                assert!(matches!(nat_status, NatStatus::Public(_)));
                assert_eq!(client.behaviour().nat_status(), nat_status);
                assert_eq!(client.behaviour().confidence(), confidence);
                assert_eq!(client.behaviour().confidence(), curr_confidence);
                if curr_confidence < 3 {
                    curr_confidence += 1;
                }
                assert!(client.behaviour().public_address().is_some());
                let index = probes.binary_search(&probe_id).unwrap();
                probes.remove(index);
                i += 1;
            }
            _ => {}
        }
    }
    drop(handles)
}
