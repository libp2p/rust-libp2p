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
    multiaddr::Protocol,
    swarm::{AddressScore, Swarm, SwarmEvent},
    Multiaddr, PeerId,
};
use libp2p_autonat::{
    Behaviour, Config, Event, InboundProbeError, InboundProbeEvent, ResponseError,
};
use libp2p_core::{ConnectedPoint, Endpoint};
use libp2p_swarm::DialError;
use std::{num::NonZeroU32, time::Duration};

async fn init_swarm(config: Config) -> Swarm<Behaviour> {
    let keypair = Keypair::generate_ed25519();
    let local_id = PeerId::from_public_key(&keypair.public());
    let transport = development_transport(keypair).await.unwrap();
    let behaviour = Behaviour::new(local_id, config);
    Swarm::new(transport, behaviour, local_id)
}

async fn init_server(config: Option<Config>) -> (Swarm<Behaviour>, PeerId, Multiaddr) {
    let mut config = config.unwrap_or_else(|| Config {
        only_global_ips: false,
        ..Default::default()
    });
    // Don't do any outbound probes.
    config.boot_delay = Duration::from_secs(60);

    let mut server = init_swarm(config).await;
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
    (server, peer_id, addr)
}

async fn spawn_client(
    listen: bool,
    add_dummy_external_addr: bool,
    server_id: PeerId,
    server_addr: Multiaddr,
    kill: oneshot::Receiver<()>,
) -> (PeerId, Option<Multiaddr>) {
    let (tx, rx) = oneshot::channel();
    async_std::task::spawn(async move {
        let mut client = init_swarm(Config {
            boot_delay: Duration::from_secs(1),
            retry_interval: Duration::from_secs(1),
            throttle_server_period: Duration::ZERO,
            only_global_ips: false,
            ..Default::default()
        })
        .await;
        client
            .behaviour_mut()
            .add_server(server_id, Some(server_addr));
        let peer_id = *client.local_peer_id();
        let mut addr = None;
        if listen {
            client
                .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
                .unwrap();
            loop {
                match client.select_next_some().await {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        addr = Some(address);
                        break;
                    }
                    _ => {}
                };
            }
        }
        if add_dummy_external_addr {
            let dummy_addr: Multiaddr = "/ip4/127.0.0.1/tcp/12345".parse().unwrap();
            client.add_external_address(dummy_addr, AddressScore::Infinite);
        }
        tx.send((peer_id, addr)).unwrap();
        let mut kill = kill.fuse();
        loop {
            futures::select! {
                _ = client.select_next_some() => {},
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
async fn test_dial_back() {
    let test = async {
        let (mut server, server_id, server_addr) = init_server(None).await;
        let (_handle, rx) = oneshot::channel();
        let (client_id, client_addr) = spawn_client(true, false, server_id, server_addr, rx).await;
        let client_port = client_addr
            .unwrap()
            .into_iter()
            .find_map(|p| match p {
                Protocol::Tcp(port) => Some(port),
                _ => None,
            })
            .unwrap();
        let observed_client_ip = loop {
            match server.select_next_some().await {
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
                        match send_back_addr.pop().unwrap() {
                            Protocol::Ip4(ip4_addr) => break ip4_addr,
                            _ => {}
                        }
                    };
                    break observed_client_ip;
                }
                SwarmEvent::IncomingConnection { .. }
                | SwarmEvent::NewListenAddr { .. }
                | SwarmEvent::ExpiredListenAddr { .. } => {}
                other => panic!("Unexpected swarm event: {:?}.", other),
            }
        };
        let expect_addr = Multiaddr::empty()
            .with(Protocol::Ip4(observed_client_ip))
            .with(Protocol::Tcp(client_port))
            .with(Protocol::P2p(client_id.into()));
        let request_probe_id = match next_event(&mut server).await {
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
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };

        loop {
            match server.select_next_some().await {
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    endpoint:
                        ConnectedPoint::Dialer {
                            address,
                            role_override: Endpoint::Dialer,
                        },
                    num_established,
                    concurrent_dial_errors,
                } => {
                    assert_eq!(peer_id, client_id);
                    assert_eq!(num_established, NonZeroU32::new(2).unwrap());
                    assert!(concurrent_dial_errors.unwrap().is_empty());
                    assert_eq!(address, expect_addr);
                    break;
                }
                SwarmEvent::Dialing(peer) => assert_eq!(peer, client_id),
                SwarmEvent::NewListenAddr { .. } | SwarmEvent::ExpiredListenAddr { .. } => {}
                other => panic!("Unexpected swarm event: {:?}.", other),
            }
        }

        match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Response {
                probe_id,
                peer,
                address,
            }) => {
                assert_eq!(probe_id, request_probe_id);
                assert_eq!(peer, client_id);
                assert_eq!(address, expect_addr);
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_dial_error() {
    let test = async {
        let (mut server, server_id, server_addr) = init_server(None).await;
        let (_handle, rx) = oneshot::channel();
        let (client_id, _) = spawn_client(false, true, server_id, server_addr, rx).await;
        let request_probe_id = match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Request { peer, probe_id, .. }) => {
                assert_eq!(peer, client_id);
                probe_id
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };

        loop {
            match server.select_next_some().await {
                SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                    assert_eq!(peer_id.unwrap(), client_id);
                    assert!(matches!(error, DialError::Transport(_)));
                    break;
                }
                SwarmEvent::Dialing(peer) => assert_eq!(peer, client_id),
                SwarmEvent::NewListenAddr { .. } | SwarmEvent::ExpiredListenAddr { .. } => {}
                other => panic!("Unexpected swarm event: {:?}.", other),
            }
        }

        match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Error {
                probe_id,
                peer,
                error,
            }) => {
                assert_eq!(probe_id, request_probe_id);
                assert_eq!(peer, client_id);
                assert_eq!(error, InboundProbeError::Response(ResponseError::DialError));
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_throttle_global_max() {
    let test = async {
        let (mut server, server_id, server_addr) = init_server(Some(Config {
            throttle_clients_global_max: 1,
            throttle_clients_period: Duration::from_secs(60),
            only_global_ips: false,
            ..Default::default()
        }))
        .await;
        let mut _handles = Vec::new();
        for _ in 0..2 {
            let (_handle, rx) = oneshot::channel();
            spawn_client(true, false, server_id, server_addr.clone(), rx).await;
            _handles.push(_handle);
        }

        let (first_probe_id, first_peer_id) = match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Request { peer, probe_id, .. }) => {
                (probe_id, peer)
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };

        loop {
            match next_event(&mut server).await {
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
                other => panic!("Unexpected behaviour event: {:?}.", other),
            };
        }
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_throttle_peer_max() {
    let test = async {
        let (mut server, server_id, server_addr) = init_server(Some(Config {
            throttle_clients_peer_max: 1,
            throttle_clients_period: Duration::from_secs(60),
            only_global_ips: false,
            ..Default::default()
        }))
        .await;

        let (_handle, rx) = oneshot::channel();
        let (client_id, _) = spawn_client(true, false, server_id, server_addr.clone(), rx).await;

        let first_probe_id = match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Request { peer, probe_id, .. }) => {
                assert_eq!(client_id, peer);
                probe_id
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };

        match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Response { peer, probe_id, .. }) => {
                assert_eq!(peer, client_id);
                assert_eq!(probe_id, first_probe_id);
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        }

        match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Error {
                peer,
                probe_id,
                error,
            }) => {
                assert_eq!(client_id, peer);
                assert_ne!(first_probe_id, probe_id);
                assert_eq!(
                    error,
                    InboundProbeError::Response(ResponseError::DialRefused)
                )
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_dial_multiple_addr() {
    let test = async {
        let (mut server, server_id, server_addr) = init_server(Some(Config {
            throttle_clients_peer_max: 1,
            throttle_clients_period: Duration::from_secs(60),
            only_global_ips: false,
            ..Default::default()
        }))
        .await;

        let (_handle, rx) = oneshot::channel();
        let (client_id, _) = spawn_client(true, true, server_id, server_addr.clone(), rx).await;

        let dial_addresses = match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Request {
                peer, addresses, ..
            }) => {
                assert_eq!(addresses.len(), 2);
                assert_eq!(client_id, peer);
                addresses
            }
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };

        loop {
            match server.select_next_some().await {
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    endpoint:
                        ConnectedPoint::Dialer {
                            address,
                            role_override: Endpoint::Dialer,
                        },
                    concurrent_dial_errors,
                    ..
                } => {
                    assert_eq!(peer_id, client_id);
                    let dial_errors = concurrent_dial_errors.unwrap();
                    assert_eq!(dial_errors.len(), 1);
                    assert_eq!(dial_errors[0].0, dial_addresses[0]);
                    assert_eq!(address, dial_addresses[1]);
                    break;
                }
                SwarmEvent::Dialing(peer) => assert_eq!(peer, client_id),
                SwarmEvent::NewListenAddr { .. } | SwarmEvent::ExpiredListenAddr { .. } => {}
                other => panic!("Unexpected swarm event: {:?}.", other),
            }
        }
    };

    run_test_with_timeout(test).await;
}

#[async_std::test]
async fn test_global_ips_config() {
    let test = async {
        let (mut server, server_id, server_addr) = init_server(Some(Config {
            // Enforce that only clients outside of the local network are qualified for dial-backs.
            only_global_ips: true,
            ..Default::default()
        }))
        .await;

        let (_handle, rx) = oneshot::channel();
        spawn_client(true, false, server_id, server_addr.clone(), rx).await;

        // Expect the probe to be refused as both peers run on the same machine and thus in the same local network.
        match next_event(&mut server).await {
            Event::InboundProbe(InboundProbeEvent::Error { error, .. }) => assert!(matches!(
                error,
                InboundProbeError::Response(ResponseError::DialRefused)
            )),
            other => panic!("Unexpected behaviour event: {:?}.", other),
        };
    };

    run_test_with_timeout(test).await;
}
