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

use std::time::Duration;

use libp2p_autonat::{
    Behaviour, Config, Event, NatStatus, OutboundProbeError, OutboundProbeEvent, ResponseError,
};
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt as _;
use tokio::task::JoinHandle;

const MAX_CONFIDENCE: usize = 3;
const TEST_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const TEST_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[tokio::test]
async fn test_auto_probe() {
    let mut client = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                retry_interval: TEST_RETRY_INTERVAL,
                refresh_interval: TEST_REFRESH_INTERVAL,
                confidence_max: MAX_CONFIDENCE,
                only_global_ips: false,
                throttle_server_period: Duration::ZERO,
                boot_delay: Duration::ZERO,
                ..Default::default()
            },
        )
    });

    let (server_id, addr, _) = new_server_swarm().await;
    client.behaviour_mut().add_server(server_id, Some(addr));

    // Initial status should be unknown.
    assert_eq!(client.behaviour().nat_status(), NatStatus::Unknown);
    assert!(client.behaviour().public_address().is_none());
    assert_eq!(client.behaviour().confidence(), 0);

    // Test no listening addresses
    match client.next_behaviour_event().await {
        Event::OutboundProbe(OutboundProbeEvent::Error { peer, error, .. }) => {
            assert!(peer.is_none());
            assert!(matches!(error, OutboundProbeError::NoAddresses));
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }

    assert_eq!(client.behaviour().nat_status(), NatStatus::Unknown);
    assert!(client.behaviour().public_address().is_none());
    assert_eq!(client.behaviour().confidence(), 0);

    // Test new public listening address
    client.listen().await;

    let id = match client.next_behaviour_event().await {
        Event::OutboundProbe(OutboundProbeEvent::Request { probe_id, peer }) => {
            assert_eq!(peer, server_id);
            probe_id
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    };

    let mut had_connection_event = false;
    loop {
        match client.next_swarm_event().await {
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
                assert_eq!(old, NatStatus::Unknown);
                assert!(matches!(new, NatStatus::Public(_)));
                assert!(new.is_public());
                break;
            }
            SwarmEvent::IncomingConnection { .. }
            | SwarmEvent::ConnectionEstablished { .. }
            | SwarmEvent::Dialing { .. }
            | SwarmEvent::NewListenAddr { .. }
            | SwarmEvent::ExpiredListenAddr { .. } => {}
            other => panic!("Unexpected swarm event: {other:?}."),
        }
    }

    // It can happen that the server observed the established connection and
    // returned a response before the inbound established connection was reported at the client.
    // In this (rare) case the `ConnectionEstablished` event
    // occurs after the `OutboundProbeEvent::Response`.
    if !had_connection_event {
        match client.next_swarm_event().await {
            SwarmEvent::ConnectionEstablished {
                endpoint, peer_id, ..
            } if endpoint.is_listener() => {
                assert_eq!(peer_id, server_id);
            }
            other => panic!("Unexpected swarm event: {other:?}."),
        }
    }

    assert_eq!(client.behaviour().confidence(), 0);
    assert!(client.behaviour().nat_status().is_public());
    assert!(client.behaviour().public_address().is_some());
}

#[tokio::test]
async fn test_confidence() {
    let mut client = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                retry_interval: TEST_RETRY_INTERVAL,
                refresh_interval: TEST_REFRESH_INTERVAL,
                confidence_max: MAX_CONFIDENCE,
                only_global_ips: false,
                throttle_server_period: Duration::ZERO,
                boot_delay: Duration::from_millis(100),
                ..Default::default()
            },
        )
    });
    let (server_id, addr, _) = new_server_swarm().await;
    client.behaviour_mut().add_server(server_id, Some(addr));

    // Randomly test either for public or for private status the confidence.
    let test_public = rand::random::<bool>();
    if test_public {
        client.listen().with_memory_addr_external().await;
    } else {
        let unreachable_addr = "/ip4/127.0.0.1/tcp/42".parse().unwrap();
        client.behaviour_mut().probe_address(unreachable_addr);
    }

    for i in 0..MAX_CONFIDENCE + 1 {
        let id = match client.next_behaviour_event().await {
            Event::OutboundProbe(OutboundProbeEvent::Request { probe_id, peer }) => {
                assert_eq!(peer, server_id);
                probe_id
            }
            other => panic!("Unexpected behaviour event: {other:?}."),
        };

        match client.next_behaviour_event().await {
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
                        assert!(matches!(
                            error,
                            OutboundProbeError::Response(ResponseError::DialError)
                        ));
                        (peer.unwrap(), probe_id)
                    }
                    other => panic!("Unexpected Outbound Event: {other:?}"),
                };
                assert_eq!(peer, server_id);
                assert_eq!(probe_id, id);
            }
            other => panic!("Unexpected behaviour event: {other:?}."),
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
            match client.next_behaviour_event().await {
                Event::StatusChanged { old, new } => {
                    assert_eq!(old, NatStatus::Unknown);
                    assert_eq!(new.is_public(), test_public);
                }
                other => panic!("Unexpected behaviour event: {other:?}."),
            }
        }
    }
}

#[tokio::test]
async fn test_throttle_server_period() {
    let mut client = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                retry_interval: TEST_RETRY_INTERVAL,
                refresh_interval: TEST_REFRESH_INTERVAL,
                confidence_max: MAX_CONFIDENCE,
                only_global_ips: false,
                // Throttle servers so they can not be re-used for dial request.
                throttle_server_period: Duration::from_secs(1000),
                boot_delay: Duration::from_millis(100),
                ..Default::default()
            },
        )
    });

    client.listen().await;

    let (server_id, addr, _) = new_server_swarm().await;
    client.behaviour_mut().add_server(server_id, Some(addr));

    // First probe should be successful and flip status to public.
    loop {
        match client.next_behaviour_event().await {
            Event::StatusChanged { old, new } => {
                assert_eq!(old, NatStatus::Unknown);
                assert!(new.is_public());
                break;
            }
            Event::OutboundProbe(OutboundProbeEvent::Request { .. }) => {}
            Event::OutboundProbe(OutboundProbeEvent::Response { .. }) => {}
            other => panic!("Unexpected behaviour event: {other:?}."),
        }
    }

    assert_eq!(client.behaviour().confidence(), 0);

    // Expect following probe to fail because server is throttled

    match client.next_behaviour_event().await {
        Event::OutboundProbe(OutboundProbeEvent::Error { peer, error, .. }) => {
            assert!(peer.is_none());
            assert!(matches!(error, OutboundProbeError::NoServer));
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }
    assert_eq!(client.behaviour().confidence(), 0);
}

#[tokio::test]
async fn test_use_connected_as_server() {
    let mut client = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                retry_interval: TEST_RETRY_INTERVAL,
                refresh_interval: TEST_REFRESH_INTERVAL,
                confidence_max: MAX_CONFIDENCE,
                only_global_ips: false,
                throttle_server_period: Duration::ZERO,
                boot_delay: Duration::from_millis(100),
                ..Default::default()
            },
        )
    });

    let (server_id, addr, _) = new_server_swarm().await;

    client.listen().await;
    let connected = client.dial_and_wait(addr).await;
    assert_eq!(connected, server_id);

    match client.next_behaviour_event().await {
        Event::OutboundProbe(OutboundProbeEvent::Request { peer, .. }) => {
            assert_eq!(peer, server_id);
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }

    match client.next_behaviour_event().await {
        Event::OutboundProbe(OutboundProbeEvent::Response { peer, .. }) => {
            assert_eq!(peer, server_id);
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }
}

#[tokio::test]
async fn test_outbound_failure() {
    let mut client = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                retry_interval: TEST_RETRY_INTERVAL,
                refresh_interval: TEST_REFRESH_INTERVAL,
                confidence_max: MAX_CONFIDENCE,
                only_global_ips: false,
                throttle_server_period: Duration::ZERO,
                boot_delay: Duration::from_millis(100),
                ..Default::default()
            },
        )
    });

    let mut servers = Vec::new();

    for _ in 0..5 {
        let (id, addr, handle) = new_server_swarm().await;
        client.behaviour_mut().add_server(id, Some(addr));
        servers.push((id, handle));
    }

    client.listen().await;

    // First probe should be successful and flip status to public.
    loop {
        match client.next_behaviour_event().await {
            Event::StatusChanged { old, new } => {
                assert_eq!(old, NatStatus::Unknown);
                assert!(new.is_public());
                break;
            }
            Event::OutboundProbe(OutboundProbeEvent::Request { .. }) => {}
            Event::OutboundProbe(OutboundProbeEvent::Response { .. }) => {}
            other => panic!("Unexpected behaviour event: {other:?}."),
        }
    }

    // kill a server
    let mut inactive_servers = Vec::new();

    for (id, handle) in servers.split_off(1) {
        handle.abort();
        inactive_servers.push(id);
    }

    // Expect to retry on outbound failure
    loop {
        match client.next_behaviour_event().await {
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
                assert!(inactive_servers.contains(&peer));
            }
            other => panic!("Unexpected behaviour event: {other:?}."),
        }
    }
}

#[tokio::test]
async fn test_global_ips_config() {
    let mut client = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                retry_interval: TEST_RETRY_INTERVAL,
                refresh_interval: TEST_REFRESH_INTERVAL,
                confidence_max: MAX_CONFIDENCE,
                // Enforce that only peers outside of the local network are used as servers.
                only_global_ips: true,
                boot_delay: Duration::from_millis(100),
                ..Default::default()
            },
        )
    });
    client.listen().await;

    let (server_id, addr, _) = new_server_swarm().await;

    // Dial server instead of adding it via `Behaviour::add_server` because the
    // `only_global_ips` restriction does not apply for manually added servers.
    let connected = client.dial_and_wait(addr).await;
    assert_eq!(connected, server_id);

    // Expect that the server is not qualified for dial-back because it is observed
    // at a local IP.
    match client.next_behaviour_event().await {
        Event::OutboundProbe(OutboundProbeEvent::Error { error, .. }) => {
            assert!(matches!(error, OutboundProbeError::NoServer))
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }
}

async fn new_server_swarm() -> (PeerId, Multiaddr, JoinHandle<()>) {
    let mut swarm = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                boot_delay: Duration::from_secs(60),
                throttle_clients_peer_max: usize::MAX,
                only_global_ips: false,
                ..Default::default()
            },
        )
    });

    let (_, multiaddr) = swarm.listen().await;
    let peer_id = *swarm.local_peer_id();

    let task = tokio::spawn(swarm.loop_on_next());

    (peer_id, multiaddr, task)
}
