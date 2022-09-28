// Copyright 2021 Protocol Labs.
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

use futures::{channel::oneshot, Future, FutureExt, StreamExt};
use futures_timer::Delay;
use libp2p::{
    development_transport,
    identity::Keypair,
    swarm::{AddressScore, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_autonat::{
    Behaviour, Config, Event, NatStatus, OutboundProbeError, OutboundProbeEvent, ResponseError,
};
use std::time::Duration;

const MAX_CONFIDENCE: usize = 3;
const TEST_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const TEST_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

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
        let mut server = init_swarm(Config {
            boot_delay: Duration::from_secs(60),
            throttle_clients_peer_max: usize::MAX,
            only_global_ips: false,
            ..Default::default()
        })
        .await;
        let peer_id = *server.local_peer_id();
        server
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();
        let addr = loop {
            match server.select_next_some().await {
                SwarmEvent::NewListenAddr { address, .. } => break address,
                _ => {}
            };
        };
        tx.send((peer_id, addr)).unwrap();
        let mut kill = kill.fuse();
        loop {
            futures::select! {
                _ = server.select_next_some() => {},
                _ = kill => return,

            }
        }
    });
    rx.await.unwrap()
}

async fn next_event(swarm: &mut Swarm<Behaviour>) -> Event {
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(event) => {
                break event;
            }
            _ => {}
        }
    }
}

async fn run_test_with_timeout(test: impl Future) {
    futures::select! {
        _ = test.fuse() => {},
        _ = Delay::new(Duration::from_secs(60)).fuse() => panic!("test timed out")
    }
}

#[async_std::test]
async fn test_auto_probe() {
    let test = async {
        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            only_global_ips: false,
            throttle_server_period: Duration::ZERO,
            boot_delay: Duration::ZERO,
            ..Default::default()
        })
        .await;

        let (_handle, rx) = oneshot::channel();
        let (server_id, addr) = spawn_server(rx).await;
        client.behaviour_mut().add_server(server_id, Some(addr));

        // Initial status should be unknown.
        assert_eq!(client.behaviour().nat_status(), NatStatus::Unknown);
        assert!(client.behaviour().public_address().is_none());
        assert_eq!(client.behaviour().confidence(), 0);

        // Test no listening addresses
        match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Error { peer, error, .. }) => {
                assert!(peer.is_none());
                assert_eq!(error, OutboundProbeError::NoAddresses);
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }

        assert_eq!(client.behaviour().nat_status(), NatStatus::Unknown);
        assert!(client.behaviour().public_address().is_none());
        assert_eq!(client.behaviour().confidence(), 0);

        // Test Private NAT Status

        // Artificially add a faulty address.
        let unreachable_addr: Multiaddr = "/ip4/127.0.0.1/tcp/42".parse().unwrap();
        client.add_external_address(unreachable_addr.clone(), AddressScore::Infinite);

        let id = match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Request { probe_id, peer }) => {
                assert_eq!(peer, server_id);
                probe_id
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };

        match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Error {
                probe_id,
                peer,
                error,
            }) => {
                assert_eq!(peer.unwrap(), server_id);
                assert_eq!(probe_id, id);
                assert_eq!(
                    error,
                    OutboundProbeError::Response(ResponseError::DialError)
                );
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }

        match next_event(&mut client).await {
            Event::StatusChanged { old, new } => {
                assert_eq!(old, NatStatus::Unknown);
                assert_eq!(new, NatStatus::Private);
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }

        assert_eq!(client.behaviour().confidence(), 0);
        assert_eq!(client.behaviour().nat_status(), NatStatus::Private);
        assert!(client.behaviour().public_address().is_none());

        // Test new public listening address
        client
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();
        loop {
            match client.select_next_some().await {
                SwarmEvent::NewListenAddr { .. } => break,
                _ => {}
            }
        }

        let id = match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Request { probe_id, peer }) => {
                assert_eq!(peer, server_id);
                probe_id
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };

        let mut had_connection_event = false;
        loop {
            match client.select_next_some().await {
                SwarmEvent::ConnectionEstablished {
                    endpoint, peer_id, ..
                } if endpoint.is_listener() => {
                    assert_eq!(peer_id, server_id);
                    had_connection_event = true;
                }
                SwarmEvent::Behaviour(Event::OutboundProbe(OutboundProbeEvent::Response {
                    probe_id,
                    peer,
                    ..
                })) => {
                    assert_eq!(peer, server_id);
                    assert_eq!(probe_id, id);
                }
                SwarmEvent::Behaviour(Event::StatusChanged { old, new }) => {
                    // Expect to flip status to public
                    assert_eq!(old, NatStatus::Private);
                    assert!(matches!(new, NatStatus::Public(_)));
                    assert!(new.is_public());
                    break;
                }
                SwarmEvent::IncomingConnection { .. }
                | SwarmEvent::NewListenAddr { .. }
                | SwarmEvent::ExpiredListenAddr { .. } => {}
                other => panic!("Unexpected swarm event: {:?}.", other),
            }
        }

        // It can happen that the server observed the established connection and
        // returned a response before the inbound established connection was reported at the client.
        // In this (rare) case the `ConnectionEstablished` event occurs after the `OutboundProbeEvent::Response`.
        if !had_connection_event {
            match client.select_next_some().await {
                SwarmEvent::ConnectionEstablished {
                    endpoint, peer_id, ..
                } if endpoint.is_listener() => {
                    assert_eq!(peer_id, server_id);
                }
                other => panic!("Unexpected swarm event: {:?}.", other),
            }
        }

        assert_eq!(client.behaviour().confidence(), 0);
        assert!(client.behaviour().nat_status().is_public());
        assert!(client.behaviour().public_address().is_some());
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_confidence() {
    let test = async {
        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            only_global_ips: false,
            throttle_server_period: Duration::ZERO,
            boot_delay: Duration::from_millis(100),
            ..Default::default()
        })
        .await;

        let (_handle, rx) = oneshot::channel();
        let (server_id, addr) = spawn_server(rx).await;
        client.behaviour_mut().add_server(server_id, Some(addr));

        // Randomly test either for public or for private status the confidence.
        let test_public = rand::random::<bool>();
        if test_public {
            client
                .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
                .unwrap();
            loop {
                match client.select_next_some().await {
                    SwarmEvent::NewListenAddr { .. } => break,
                    _ => {}
                }
            }
        } else {
            let unreachable_addr: Multiaddr = "/ip4/127.0.0.1/tcp/42".parse().unwrap();
            client.add_external_address(unreachable_addr, AddressScore::Infinite);
        }

        for i in 0..MAX_CONFIDENCE + 1 {
            let id = match next_event(&mut client).await {
                Event::OutboundProbe(OutboundProbeEvent::Request { probe_id, peer }) => {
                    assert_eq!(peer, server_id);
                    probe_id
                }
                other => panic!("Unexpected behaviour event: {:?}.", other),
            };

            match next_event(&mut client).await {
                Event::OutboundProbe(event) => {
                    let (peer, probe_id) = match event {
                        OutboundProbeEvent::Response { probe_id, peer, .. } if test_public => {
                            (peer, probe_id)
                        }
                        OutboundProbeEvent::Error {
                            probe_id,
                            peer,
                            error,
                        } if !test_public => {
                            assert_eq!(
                                error,
                                OutboundProbeError::Response(ResponseError::DialError)
                            );
                            (peer.unwrap(), probe_id)
                        }
                        other => panic!("Unexpected Outbound Event: {:?}", other),
                    };
                    assert_eq!(peer, server_id);
                    assert_eq!(probe_id, id);
                }
                other => panic!("Unexpected behaviour event: {:?}.", other),
            }

            // Confidence should increase each iteration up to MAX_CONFIDENCE
            let expect_confidence = if i <= MAX_CONFIDENCE {
                i
            } else {
                MAX_CONFIDENCE
            };
            assert_eq!(client.behaviour().confidence(), expect_confidence);
            assert_eq!(client.behaviour().nat_status().is_public(), test_public);

            // Expect status to flip after first probe
            if i == 0 {
                match next_event(&mut client).await {
                    Event::StatusChanged { old, new } => {
                        assert_eq!(old, NatStatus::Unknown);
                        assert_eq!(new.is_public(), test_public);
                    }
                    other => panic!("Unexpected behaviour event: {:?}.", other),
                }
            }
        }
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_throttle_server_period() {
    let test = async {
        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            only_global_ips: false,
            // Throttle servers so they can not be re-used for dial request.
            throttle_server_period: Duration::from_secs(1000),
            boot_delay: Duration::from_millis(100),
            ..Default::default()
        })
        .await;

        client
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();
        loop {
            match client.select_next_some().await {
                SwarmEvent::NewListenAddr { .. } => break,
                _ => {}
            }
        }

        let (_handle, rx) = oneshot::channel();
        let (id, addr) = spawn_server(rx).await;
        client.behaviour_mut().add_server(id, Some(addr));

        // First probe should be successful and flip status to public.
        loop {
            match next_event(&mut client).await {
                Event::StatusChanged { old, new } => {
                    assert_eq!(old, NatStatus::Unknown);
                    assert!(new.is_public());
                    break;
                }
                Event::OutboundProbe(OutboundProbeEvent::Request { .. }) => {}
                Event::OutboundProbe(OutboundProbeEvent::Response { .. }) => {}
                other => panic!("Unexpected behaviour event: {:?}.", other),
            }
        }

        assert_eq!(client.behaviour().confidence(), 0);

        // Expect following probe to fail because server is throttled

        match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Error { peer, error, .. }) => {
                assert!(peer.is_none());
                assert_eq!(error, OutboundProbeError::NoServer);
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }
        assert_eq!(client.behaviour().confidence(), 0);
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_use_connected_as_server() {
    let test = async {
        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            only_global_ips: false,
            throttle_server_period: Duration::ZERO,
            boot_delay: Duration::from_millis(100),
            ..Default::default()
        })
        .await;

        let (_handle, rx) = oneshot::channel();
        let (server_id, addr) = spawn_server(rx).await;

        client
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();

        client.dial(addr).unwrap();

        // await connection
        loop {
            if let SwarmEvent::ConnectionEstablished { peer_id, .. } =
                client.select_next_some().await
            {
                assert_eq!(peer_id, server_id);
                break;
            }
        }

        match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Request { peer, .. }) => {
                assert_eq!(peer, server_id);
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }

        match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Response { peer, .. }) => {
                assert_eq!(peer, server_id);
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_outbound_failure() {
    let test = async {
        let mut servers = Vec::new();

        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            only_global_ips: false,
            throttle_server_period: Duration::ZERO,
            boot_delay: Duration::from_millis(100),
            ..Default::default()
        })
        .await;

        for _ in 0..5 {
            let (tx, rx) = oneshot::channel();
            let (id, addr) = spawn_server(rx).await;
            client.behaviour_mut().add_server(id, Some(addr));
            servers.push((id, tx));
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
        // First probe should be successful and flip status to public.
        loop {
            match next_event(&mut client).await {
                Event::StatusChanged { old, new } => {
                    assert_eq!(old, NatStatus::Unknown);
                    assert!(new.is_public());
                    break;
                }
                Event::OutboundProbe(OutboundProbeEvent::Request { .. }) => {}
                Event::OutboundProbe(OutboundProbeEvent::Response { .. }) => {}
                other => panic!("Unexpected behaviour event: {:?}.", other),
            }
        }

        let inactive = servers.split_off(1);
        // Drop the handles of the inactive servers to kill them.
        let inactive_ids: Vec<_> = inactive.into_iter().map(|(id, _handle)| id).collect();

        // Expect to retry on outbound failure
        loop {
            match next_event(&mut client).await {
                Event::OutboundProbe(OutboundProbeEvent::Request { .. }) => {}
                Event::OutboundProbe(OutboundProbeEvent::Response { peer, .. }) => {
                    assert_eq!(peer, servers[0].0);
                    break;
                }
                Event::OutboundProbe(OutboundProbeEvent::Error {
                    peer: Some(peer),
                    error: OutboundProbeError::OutboundRequest(_),
                    ..
                }) => {
                    assert!(inactive_ids.contains(&peer));
                }
                other => panic!("Unexpected behaviour event: {:?}.", other),
            }
        }
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_global_ips_config() {
    let test = async {
        let mut client = init_swarm(Config {
            retry_interval: TEST_RETRY_INTERVAL,
            refresh_interval: TEST_REFRESH_INTERVAL,
            confidence_max: MAX_CONFIDENCE,
            // Enforce that only peers outside of the local network are used as servers.
            only_global_ips: true,
            boot_delay: Duration::from_millis(100),
            ..Default::default()
        })
        .await;

        client
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();
        loop {
            match client.select_next_some().await {
                SwarmEvent::NewListenAddr { .. } => break,
                _ => {}
            }
        }

        let (_handle, rx) = oneshot::channel();
        let (server_id, addr) = spawn_server(rx).await;

        // Dial server instead of adding it via `Behaviour::add_server` because the
        // `only_global_ips` restriction does not apply for manually added servers.
        client.dial(addr).unwrap();
        loop {
            if let SwarmEvent::ConnectionEstablished { peer_id, .. } =
                client.select_next_some().await
            {
                assert_eq!(peer_id, server_id);
                break;
            }
        }

        // Expect that the server is not qualified for dial-back because it is observed
        // at a local IP.
        match next_event(&mut client).await {
            Event::OutboundProbe(OutboundProbeEvent::Error { error, .. }) => {
                assert!(matches!(error, OutboundProbeError::NoServer))
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }
    };

    run_test_with_timeout(test).await;
}
