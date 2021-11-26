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
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_autonat::{
    AutoProbe, Behaviour, Config, ProbeConfig, Reachability, ResponseError, ServerConfig,
};
use std::time::Duration;

const SERVER_COUNT: usize = 5;
const RETRY_CONFIG: AutoProbe = AutoProbe {
    interval: Duration::from_millis(100),
    config: ProbeConfig {
        max_peers: SERVER_COUNT,
        min_confidence: 3,
        servers: Vec::new(),
        extend_with_connected: false,
    },
};

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
            auto_probe: None,
            server: Some(ServerConfig {
                only_public: false,
                ..Default::default()
            }),
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
async fn test_public() {
    let mut handles = Vec::new();

    let mut client = init_swarm(Config {
        auto_probe: Some(RETRY_CONFIG),
        ..Default::default()
    })
    .await;

    for _ in 0..SERVER_COUNT {
        let (tx, rx) = oneshot::channel();
        let (id, addr) = spawn_server(rx).await;
        client.behaviour_mut().add_server(id, Some(addr));
        handles.push(tx);
    }

    // Auto-Probe should directly resolve to `Unknown` as the local peer has no listening addresses
    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(status) => {
                assert!(status.errors.is_empty());
                assert!(status.outbound_failures.is_empty());
                assert_eq!(status.reachability, Reachability::Unknown);
                assert_eq!(client.behaviour().reachability(), Reachability::Unknown);
                break;
            }
            _ => {}
        }
    }

    // Hack to create a dummy ListenerId
    let listener_id = init_swarm(Config::default())
        .await
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();
    // Artificially add a faulty address.
    let unreachable_addr = "/ip4/127.0.0.1/tcp/42".parse().unwrap();
    client
        .behaviour_mut()
        .inject_new_listen_addr(listener_id, &unreachable_addr);

    // Auto-Probe should resolve to private since no server can reach the client at this address.
    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(status) => {
                assert!(status.outbound_failures.is_empty());
                let errors: Vec<ResponseError> =
                    status.errors.into_iter().map(|(_, e)| e).collect();
                let expect_error_count = SERVER_COUNT - RETRY_CONFIG.config.min_confidence + 1;
                assert_eq!(errors, vec![ResponseError::DialError; expect_error_count]);
                assert_eq!(status.reachability, Reachability::Private);
                assert_eq!(client.behaviour().reachability(), Reachability::Private);
                break;
            }
            _ => {}
        }
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

    // Client should be reachable by all servers
    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(status) => {
                assert!(status.errors.is_empty());
                assert!(status.outbound_failures.is_empty());
                assert!(status.reachability.is_public());
                assert!(client.behaviour().reachability().is_public());
                break;
            }
            _ => {}
        }
    }

    // Drop enough severs so that min confidence can not be reached anymore.
    handles.truncate(RETRY_CONFIG.config.min_confidence - 1);

    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(status) => {
                assert!(status.errors.is_empty());
                assert_eq!(
                    status.outbound_failures.len(),
                    SERVER_COUNT - RETRY_CONFIG.config.min_confidence + 1
                );
                assert_eq!(status.reachability, Reachability::Unknown);
                assert_eq!(client.behaviour().reachability(), Reachability::Unknown);
                break;
            }
            _ => {}
        }
    }
}
