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

use std::{num::NonZeroU32, time::Duration};

use libp2p_autonat::{
    Behaviour, Config, Event, InboundProbeError, InboundProbeEvent, ResponseError,
};
use libp2p_core::{multiaddr::Protocol, ConnectedPoint, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{DialError, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt as _;

#[tokio::test]
async fn test_dial_back() {
    let (mut server, server_id, server_addr) = new_server_swarm(None).await;
    let (mut client, client_id) = new_client_swarm(server_id, server_addr).await;
    let (_, client_addr) = client.listen().await;
    tokio::spawn(client.loop_on_next());

    let client_port = client_addr
        .into_iter()
        .find_map(|p| match p {
            Protocol::Tcp(port) => Some(port),
            _ => None,
        })
        .unwrap();
    let observed_client_ip = loop {
        match server.next_swarm_event().await {
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint:
                    ConnectedPoint::Listener {
                        mut send_back_addr, ..
                    },
                ..
            } => {
                assert_eq!(peer_id, client_id);
                let observed_client_ip = loop {
                    if let Protocol::Ip4(ip4_addr) = send_back_addr.pop().unwrap() {
                        break ip4_addr;
                    }
                };
                break observed_client_ip;
            }
            SwarmEvent::IncomingConnection { .. }
            | SwarmEvent::NewListenAddr { .. }
            | SwarmEvent::ExpiredListenAddr { .. } => {}
            other => panic!("Unexpected swarm event: {other:?}."),
        }
    };
    let expect_addr = Multiaddr::empty()
        .with(Protocol::Ip4(observed_client_ip))
        .with(Protocol::Tcp(client_port))
        .with(Protocol::P2p(client_id));
    let request_probe_id = match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Request {
            peer,
            addresses,
            probe_id,
        }) => {
            assert_eq!(peer, client_id);
            assert_eq!(addresses.len(), 1);
            assert_eq!(addresses[0], expect_addr);
            probe_id
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    };

    loop {
        match server.next_swarm_event().await {
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint:
                    ConnectedPoint::Dialer {
                        address,
                        role_override: Endpoint::Dialer,
                        ..
                    },
                num_established,
                concurrent_dial_errors,
                established_in: _,
                connection_id: _,
            } => {
                assert_eq!(peer_id, client_id);
                assert_eq!(num_established, NonZeroU32::new(2).unwrap());
                assert!(concurrent_dial_errors.unwrap().is_empty());
                assert_eq!(address, expect_addr);
                break;
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer),
                ..
            } => assert_eq!(peer, client_id),
            SwarmEvent::NewListenAddr { .. } | SwarmEvent::ExpiredListenAddr { .. } => {}
            other => panic!("Unexpected swarm event: {other:?}."),
        }
    }

    match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Response {
            probe_id,
            peer,
            address,
        }) => {
            assert_eq!(probe_id, request_probe_id);
            assert_eq!(peer, client_id);
            assert_eq!(address, expect_addr);
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }
}

#[tokio::test]
async fn test_dial_error() {
    let (mut server, server_id, server_addr) = new_server_swarm(None).await;
    let (mut client, client_id) = new_client_swarm(server_id, server_addr).await;
    client
        .behaviour_mut()
        .probe_address("/ip4/127.0.0.1/tcp/12345".parse().unwrap());
    tokio::spawn(client.loop_on_next());

    let request_probe_id = match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Request { peer, probe_id, .. }) => {
            assert_eq!(peer, client_id);
            probe_id
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    };

    loop {
        match server.next_swarm_event().await {
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                assert_eq!(peer_id.unwrap(), client_id);
                assert!(matches!(error, DialError::Transport(_)));
                break;
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer),
                ..
            } => assert_eq!(peer, client_id),
            SwarmEvent::NewListenAddr { .. } | SwarmEvent::ExpiredListenAddr { .. } => {}
            other => panic!("Unexpected swarm event: {other:?}."),
        }
    }

    match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Error {
            probe_id,
            peer,
            error,
        }) => {
            assert_eq!(probe_id, request_probe_id);
            assert_eq!(peer, client_id);
            assert!(matches!(
                error,
                InboundProbeError::Response(ResponseError::DialError)
            ));
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }
}

#[tokio::test]
async fn test_throttle_global_max() {
    let (mut server, server_id, server_addr) = new_server_swarm(Some(Config {
        throttle_clients_global_max: 1,
        throttle_clients_period: Duration::from_secs(60),
        only_global_ips: false,
        ..Default::default()
    }))
    .await;
    for _ in 0..2 {
        let (mut client, _) = new_client_swarm(server_id, server_addr.clone()).await;
        client.listen().await;
        tokio::spawn(client.loop_on_next());
    }

    let (first_probe_id, first_peer_id) = match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Request { peer, probe_id, .. }) => (probe_id, peer),
        other => panic!("Unexpected behaviour event: {other:?}."),
    };

    loop {
        match server.next_behaviour_event().await {
            Event::InboundProbe(InboundProbeEvent::Error {
                peer,
                probe_id,
                error: InboundProbeError::Response(ResponseError::DialRefused),
            }) => {
                assert_ne!(first_peer_id, peer);
                assert_ne!(first_probe_id, probe_id);
                break;
            }
            Event::InboundProbe(InboundProbeEvent::Response { peer, probe_id, .. }) => {
                assert_eq!(first_peer_id, peer);
                assert_eq!(first_probe_id, probe_id);
            }
            other => panic!("Unexpected behaviour event: {other:?}."),
        };
    }
}

#[tokio::test]
async fn test_throttle_peer_max() {
    let (mut server, server_id, server_addr) = new_server_swarm(Some(Config {
        throttle_clients_peer_max: 1,
        throttle_clients_period: Duration::from_secs(60),
        only_global_ips: false,
        ..Default::default()
    }))
    .await;

    let (mut client, client_id) = new_client_swarm(server_id, server_addr.clone()).await;
    client.listen().await;
    tokio::spawn(client.loop_on_next());

    let first_probe_id = match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Request { peer, probe_id, .. }) => {
            assert_eq!(client_id, peer);
            probe_id
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    };

    match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Response { peer, probe_id, .. }) => {
            assert_eq!(peer, client_id);
            assert_eq!(probe_id, first_probe_id);
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    }

    match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Error {
            peer,
            probe_id,
            error,
        }) => {
            assert_eq!(client_id, peer);
            assert_ne!(first_probe_id, probe_id);
            assert!(matches!(
                error,
                InboundProbeError::Response(ResponseError::DialRefused)
            ));
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    };
}

#[tokio::test]
async fn test_dial_multiple_addr() {
    let (mut server, server_id, server_addr) = new_server_swarm(Some(Config {
        throttle_clients_peer_max: 1,
        throttle_clients_period: Duration::from_secs(60),
        only_global_ips: false,
        ..Default::default()
    }))
    .await;

    let (mut client, client_id) = new_client_swarm(server_id, server_addr.clone()).await;
    client.listen().await;
    client
        .behaviour_mut()
        .probe_address("/ip4/127.0.0.1/tcp/12345".parse().unwrap());
    tokio::spawn(client.loop_on_next());

    let dial_addresses = match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Request {
            peer, addresses, ..
        }) => {
            assert_eq!(addresses.len(), 2);
            assert_eq!(client_id, peer);
            addresses
        }
        other => panic!("Unexpected behaviour event: {other:?}."),
    };

    loop {
        match server.next_swarm_event().await {
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint:
                    ConnectedPoint::Dialer {
                        address,
                        role_override: Endpoint::Dialer,
                        ..
                    },
                concurrent_dial_errors,
                ..
            } => {
                assert_eq!(peer_id, client_id);
                let dial_errors = concurrent_dial_errors.unwrap();

                // The concurrent dial might not be fast enough to produce a dial error.
                if let Some((addr, _)) = dial_errors.first() {
                    assert_eq!(addr, &dial_addresses[0]);
                }

                assert_eq!(address, dial_addresses[1]);
                break;
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer),
                ..
            } => assert_eq!(peer, client_id),
            SwarmEvent::NewListenAddr { .. } | SwarmEvent::ExpiredListenAddr { .. } => {}
            other => panic!("Unexpected swarm event: {other:?}."),
        }
    }
}

#[tokio::test]
async fn test_global_ips_config() {
    let (mut server, server_id, server_addr) = new_server_swarm(Some(Config {
        // Enforce that only clients outside of the local network are qualified for dial-backs.
        only_global_ips: true,
        ..Default::default()
    }))
    .await;

    let (mut client, _) = new_client_swarm(server_id, server_addr.clone()).await;
    client.listen().await;
    tokio::spawn(client.loop_on_next());

    // Expect the probe to be refused as both peers run
    // on the same machine and thus in the same local network.
    match server.next_behaviour_event().await {
        Event::InboundProbe(InboundProbeEvent::Error { error, .. }) => assert!(matches!(
            error,
            InboundProbeError::Response(ResponseError::DialRefused)
        )),
        other => panic!("Unexpected behaviour event: {other:?}."),
    };
}

async fn new_server_swarm(config: Option<Config>) -> (Swarm<Behaviour>, PeerId, Multiaddr) {
    let mut config = config.unwrap_or_else(|| Config {
        only_global_ips: false,
        ..Default::default()
    });
    // Don't do any outbound probes.
    config.boot_delay = Duration::from_secs(60);

    let mut server = Swarm::new_ephemeral(|key| Behaviour::new(key.public().to_peer_id(), config));
    let peer_id = *server.local_peer_id();
    let (_, addr) = server.listen().await;

    (server, peer_id, addr)
}

async fn new_client_swarm(server_id: PeerId, server_addr: Multiaddr) -> (Swarm<Behaviour>, PeerId) {
    let mut client = Swarm::new_ephemeral(|key| {
        Behaviour::new(
            key.public().to_peer_id(),
            Config {
                boot_delay: Duration::from_secs(1),
                retry_interval: Duration::from_secs(1),
                throttle_server_period: Duration::ZERO,
                only_global_ips: false,
                ..Default::default()
            },
        )
    });
    client
        .behaviour_mut()
        .add_server(server_id, Some(server_addr));
    let peer_id = *client.local_peer_id();

    (client, peer_id)
}
