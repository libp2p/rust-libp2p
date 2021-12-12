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

use async_std::task::sleep;
use futures::{channel::oneshot, select, FutureExt, StreamExt};
use libp2p::{
    development_transport,
    identity::Keypair,
    swarm::{AddressScore, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_autonat::{Behaviour, Config, NatStatus};
use std::time::Duration;

const SERVER_COUNT: usize = 5;
const MAX_CONFIDENCE: usize = 3;

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

async fn next_status(client: &mut Swarm<Behaviour>) -> NatStatus {
    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(nat_status) => {
                break nat_status;
            }
            _ => {}
        }
    }
}

async fn poll_and_sleep(client: &mut Swarm<Behaviour>, duration: Duration) {
    let poll = async {
        loop {
            let _ = client.next().await;
        }
    };
    select! {
        _ = poll.fuse()=> {},
        _ = sleep(duration).fuse() => {},
    }
}

#[async_std::test]
#[ignore]
// TODO: fix test
async fn test_auto_probe() {
    let test_retry_interval = Duration::from_millis(100);
    let mut client = init_swarm(Config {
        retry_interval: test_retry_interval,
        refresh_interval: test_retry_interval,
        throttle_peer_period: Duration::ZERO,
        boot_delay: Duration::ZERO,
        ..Default::default()
    })
    .await;

    let (_handle, rx) = oneshot::channel();
    let (id, addr) = spawn_server(rx).await;
    client.behaviour_mut().add_server(id, Some(addr));

    // Initial status should be unknown
    assert_eq!(client.behaviour().nat_status(), NatStatus::Unknown);
    assert!(client.behaviour().public_address().is_none());
    assert_eq!(client.behaviour().confidence(), 0);

    // Artificially add a faulty address.
    let unreachable_addr: Multiaddr = "/ip4/127.0.0.1/tcp/42".parse().unwrap();
    client.add_external_address(unreachable_addr.clone(), AddressScore::Infinite);

    // Auto-Probe should resolve to private since the server can not reach us.
    // Confidence should increase with each iteration.
    let mut expect_confidence = 0;
    let status = next_status(&mut client).await;
    assert_eq!(status, NatStatus::Private);
    assert_eq!(client.behaviour().confidence(), expect_confidence);
    assert_eq!(client.behaviour().nat_status(), NatStatus::Private);

    for _ in 0..MAX_CONFIDENCE + 1 {
        poll_and_sleep(&mut client, test_retry_interval).await;

        if expect_confidence < MAX_CONFIDENCE {
            expect_confidence += 1;
        }

        assert_eq!(client.behaviour().nat_status(), NatStatus::Private);
        assert!(client.behaviour().public_address().is_none());
        assert_eq!(client.behaviour().confidence(), expect_confidence);
    }
    assert_eq!(client.behaviour().confidence(), MAX_CONFIDENCE);

    client.remove_external_address(&unreachable_addr);

    // Confidence in private should decrease
    while expect_confidence > 0 {
        poll_and_sleep(&mut client, test_retry_interval).await;

        expect_confidence -= 1;

        assert_eq!(client.behaviour().nat_status(), NatStatus::Private);
        assert_eq!(client.behaviour().confidence(), expect_confidence);
    }

    // expect to flip to Unknown
    let status = next_status(&mut client).await;
    assert_eq!(status, NatStatus::Unknown);

    // Confidence should not increase.
    for _ in 0..MAX_CONFIDENCE {
        poll_and_sleep(&mut client, test_retry_interval).await;

        assert_eq!(client.behaviour().nat_status(), NatStatus::Unknown);
        assert_eq!(client.behaviour().confidence(), 0);
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

    let status = next_status(&mut client).await;
    assert!(status.is_public());
    let public_addr = client.behaviour().public_address().unwrap().clone();

    let mut expect_confidence = 0;
    for _ in 0..MAX_CONFIDENCE + 1 {
        poll_and_sleep(&mut client, test_retry_interval).await;

        if expect_confidence < MAX_CONFIDENCE {
            expect_confidence += 1;
        }

        assert!(client.behaviour().nat_status().is_public());
        assert_eq!(client.behaviour().public_address(), Some(&public_addr));
        assert_eq!(client.behaviour().confidence(), expect_confidence);
    }

    drop(_handle);
}

#[async_std::test]
#[ignore]
// TODO: fix test
async fn test_throttle_peer_period() {
    let mut handles = Vec::new();
    let test_retry_interval = Duration::from_millis(100);

    let mut client = init_swarm(Config {
        retry_interval: test_retry_interval,
        refresh_interval: test_retry_interval,
        throttle_peer_period: Duration::from_secs(100),
        boot_delay: Duration::from_millis(100),
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

    let nat_status = next_status(&mut client).await;
    assert!(nat_status.is_public());
    assert_eq!(client.behaviour().confidence(), 0);

    let mut expect_confidence = 0;
    for _ in 0..SERVER_COUNT - 1 {
        poll_and_sleep(&mut client, test_retry_interval).await;

        if expect_confidence < MAX_CONFIDENCE {
            expect_confidence += 1;
        }
        assert!(client.behaviour().nat_status().is_public());
        assert_eq!(client.behaviour().confidence(), expect_confidence);
    }

    assert!(nat_status.is_public());

    // All servers are throttled, probes will decrease confidence
    while expect_confidence > 0 {
        poll_and_sleep(&mut client, test_retry_interval).await;
        expect_confidence -= 1;
        assert_eq!(client.behaviour().confidence(), expect_confidence);
        assert!(client.behaviour().nat_status().is_public());
    }
    let nat_status = next_status(&mut client).await;
    assert_eq!(nat_status, NatStatus::Unknown);
    assert_eq!(client.behaviour().confidence(), 0);

    drop(handles)
}
