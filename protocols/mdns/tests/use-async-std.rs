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
// DEALINGS IN THE SOFTWARE.use futures::StreamExt;

use std::time::Duration;

use futures::future::Either;
use libp2p_mdns::{async_io::Behaviour, Config, Event};
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt as _;
use tracing_subscriber::EnvFilter;

#[async_std::test]
async fn test_discovery_async_std_ipv4() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    run_discovery_test(Config::default()).await
}

#[async_std::test]
async fn test_discovery_async_std_ipv6() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let config = Config {
        enable_ipv6: true,
        ..Default::default()
    };
    run_discovery_test(config).await
}

#[async_std::test]
async fn test_expired_async_std() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let config = Config {
        ttl: Duration::from_secs(1),
        query_interval: Duration::from_secs(10),
        ..Default::default()
    };

    let mut a = create_swarm(config.clone()).await;
    let a_peer_id = *a.local_peer_id();

    let mut b = create_swarm(config).await;
    let b_peer_id = *b.local_peer_id();

    loop {
        match futures::future::select(a.next_behaviour_event(), b.next_behaviour_event()).await {
            Either::Left((Event::Expired(peers), _)) => {
                if peers.into_iter().any(|(p, _)| p == b_peer_id) {
                    return;
                }
            }
            Either::Right((Event::Expired(peers), _)) => {
                if peers.into_iter().any(|(p, _)| p == a_peer_id) {
                    return;
                }
            }
            _ => {}
        }
    }
}

#[async_std::test]
async fn test_no_expiration_on_close_async_std() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let config = Config {
        ttl: Duration::from_secs(120),
        query_interval: Duration::from_secs(10),
        ..Default::default()
    };

    let mut a = create_swarm(config.clone()).await;

    let b = create_swarm(config).await;
    let b_peer_id = *b.local_peer_id();
    async_std::task::spawn(b.loop_on_next());

    // 1. Connect via address from mDNS event
    loop {
        if let Event::Discovered(peers) = a.next_behaviour_event().await {
            if let Some((_, addr)) = peers.into_iter().find(|(p, _)| p == &b_peer_id) {
                a.dial_and_wait(addr).await;
                break;
            }
        }
    }

    // 2. Close connection
    let _ = a.disconnect_peer_id(b_peer_id);
    a.wait(|event| {
        matches!(event, SwarmEvent::ConnectionClosed { peer_id, .. } if peer_id == b_peer_id)
            .then_some(())
    })
    .await;

    // 3. Ensure we can still dial via `PeerId`.
    a.dial(b_peer_id).unwrap();
    a.wait(|event| {
        matches!(event, SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == b_peer_id)
            .then_some(())
    })
    .await;
}

async fn run_discovery_test(config: Config) {
    let mut a = create_swarm(config.clone()).await;
    let a_peer_id = *a.local_peer_id();

    let mut b = create_swarm(config).await;
    let b_peer_id = *b.local_peer_id();

    let mut discovered_a = false;
    let mut discovered_b = false;

    while !discovered_a && !discovered_b {
        match futures::future::select(a.next_behaviour_event(), b.next_behaviour_event()).await {
            Either::Left((Event::Discovered(peers), _)) => {
                if peers.into_iter().any(|(p, _)| p == b_peer_id) {
                    discovered_b = true;
                }
            }
            Either::Right((Event::Discovered(peers), _)) => {
                if peers.into_iter().any(|(p, _)| p == a_peer_id) {
                    discovered_a = true;
                }
            }
            _ => {}
        }
    }
}

async fn create_swarm(config: Config) -> Swarm<Behaviour> {
    let mut swarm =
        Swarm::new_ephemeral(|key| Behaviour::new(config, key.public().to_peer_id()).unwrap());

    // Manually listen on all interfaces because mDNS only works for non-loopback addresses.
    let expected_listener_id = swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    swarm
        .wait(|e| match e {
            SwarmEvent::NewListenAddr { listener_id, .. } => {
                (listener_id == expected_listener_id).then_some(())
            }
            _ => None,
        })
        .await;

    swarm
}
