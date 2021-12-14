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
use futures_timer::Delay;
use libp2p::{
    development_transport,
    identity::Keypair,
    swarm::{AddressScore, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_autonat::{Behaviour, Config, NatStatus};
use std::time::Duration;

const SERVER_COUNT: usize = 10;
const MAX_CONFIDENCE: usize = 3;
const TEST_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const TEST_REFRESH_INTERVAL: Duration = Duration::from_millis(100);

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

// Await a delay while still driving the swarm.
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
async fn test_auto_probe() {
    let run_test = async {
        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            throttle_server_period: Duration::ZERO,
            boot_delay: Duration::ZERO,
            ..Default::default()
        })
        .await;

        let (_handle, rx) = oneshot::channel();
        let (id, addr) = spawn_server(rx).await;
        client.behaviour_mut().add_server(id, Some(addr));

        // Initial status should be unknown.
        assert_eq!(client.behaviour().nat_status(), NatStatus::Unknown);
        assert!(client.behaviour().public_address().is_none());
        assert_eq!(client.behaviour().confidence(), 0);

        // Artificially add a faulty address.
        let unreachable_addr: Multiaddr = "/ip4/127.0.0.1/tcp/42".parse().unwrap();
        client.add_external_address(unreachable_addr.clone(), AddressScore::Infinite);

        let status = next_status(&mut client).await;
        assert_eq!(status, NatStatus::Private);
        assert_eq!(client.behaviour().confidence(), 0);
        assert_eq!(client.behaviour().nat_status(), NatStatus::Private);

        client
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();
        loop {
            match client.select_next_some().await {
                SwarmEvent::NewListenAddr { .. } => break,
                _ => {}
            }
        }

        // Expect inbound dial from server.
        loop {
            match client.select_next_some().await {
                SwarmEvent::OutgoingConnectionError { .. } => {}
                SwarmEvent::ConnectionEstablished { endpoint, .. } if endpoint.is_listener() => {
                    break
                }
                _ => {}
            }
        }

        let status = next_status(&mut client).await;
        assert!(status.is_public());
        assert!(client.behaviour().public_address().is_some());

        drop(_handle);
    };

    futures::select! {
        _ = run_test.fuse() => {},
        _ = Delay::new(Duration::from_secs(30)).fuse() => panic!("test timed out")
    }
}

#[async_std::test]
async fn test_throttle_server_period() {
    let run_test = async {
        let mut handles = Vec::new();

        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            // Throttle servers so they can not be re-used for dial request.
            throttle_server_period: Duration::from_secs(1000),
            boot_delay: Duration::ZERO,
            ..Default::default()
        })
        .await;

        for _ in 1..=MAX_CONFIDENCE {
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

        // Sleep double the time that would be needed to reach max confidence
        poll_and_sleep(&mut client, TEST_RETRY_INTERVAL * MAX_CONFIDENCE as u32 * 2).await;

        // Can only reach confidence n-1 with n available servers, as one probe
        // is required to flip the status the first time.
        assert_eq!(client.behaviour().confidence(), MAX_CONFIDENCE - 1);

        drop(handles)
    };

    futures::select! {
        _ = run_test.fuse() => {},
        _ = Delay::new(Duration::from_secs(30)).fuse() => panic!("test timed out")
    }
}

#[async_std::test]
async fn test_use_connected_as_server() {
    let run_test = async {
        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            throttle_server_period: Duration::ZERO,
            boot_delay: Duration::ZERO,
            ..Default::default()
        })
        .await;

        let (_handle, rx) = oneshot::channel();
        let (id, addr) = spawn_server(rx).await;

        client
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();

        client.dial(addr).unwrap();

        let nat_status = loop {
            match client.select_next_some().await {
                SwarmEvent::Behaviour(status) => break status,
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    // keep connection alive
                    client.dial(peer_id).unwrap();
                }
                _ => {}
            }
        };

        assert!(nat_status.is_public());
        assert_eq!(client.behaviour().confidence(), 0);

        let _ = client.disconnect_peer_id(id);

        poll_and_sleep(&mut client, TEST_RETRY_INTERVAL * 2).await;

        // No connected peers to send probes to; confidence can not increase.
        assert!(nat_status.is_public());
        assert_eq!(client.behaviour().confidence(), 0);

        drop(_handle);
    };

    futures::select! {
        _ = run_test.fuse() => {},
        _ = Delay::new(Duration::from_secs(30)).fuse() => panic!("test timed out")
    }
}

#[async_std::test]
async fn test_outbound_failure() {
    let run_test = async {
        let mut handles = Vec::new();

        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            throttle_server_period: Duration::ZERO,
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

        // Kill all servers apart from one.
        handles.drain(1..);

        // Expect inbound dial indicating that we sent a dial-request to the remote.
        loop {
            match client.select_next_some().await {
                SwarmEvent::OutgoingConnectionError { .. } => {}
                SwarmEvent::ConnectionEstablished { endpoint, .. } if endpoint.is_listener() => {
                    break
                }
                _ => {}
            }
        }

        // Delay so that the server has time to send dial-response
        poll_and_sleep(&mut client, Duration::from_millis(100)).await;

        assert!(client.behaviour().confidence() > 0);
    };

    futures::select! {
        _ = run_test.fuse() => {},
        _ = Delay::new(Duration::from_secs(30)).fuse() => panic!("test timed out")
    }
}
