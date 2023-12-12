use futures::StreamExt;
use libp2p_autonatv2::client::{Report, StatusUpdate};
use libp2p_core::{multiaddr::Protocol, Multiaddr};
use libp2p_swarm::{DialError, NetworkBehaviour, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn confirm_successful() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let (_alice, _bob) = bootstrap().await;
}

#[tokio::test]
async fn dial_back_to_unsupported_protocol() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let (mut alice, mut bob) = bootstrap().await;

    let test_addr: Multiaddr = "/ip4/127.0.0.1/udp/1234/quic/webtransport".parse().unwrap();

    bob.behaviour_mut()
        .autonat
        .inject_test_addr(test_addr.clone());

    match libp2p_swarm_test::drive(&mut alice, &mut bob).await {
        (
            [SwarmEvent::Dialing { .. }, SwarmEvent::OutgoingConnectionError {
                error: DialError::Transport(_),
                ..
            }],
            [SwarmEvent::Behaviour(CombinedClientEvent::Autonat(Report {
                update:
                    StatusUpdate::GotDialDataReq {
                        addr: addr_0,
                        num_bytes: req_num,
                    },
                ..
            })), SwarmEvent::Behaviour(CombinedClientEvent::Autonat(Report {
                update:
                    StatusUpdate::CompletedDialData {
                        addr: addr_1,
                        num_bytes: done_num,
                    },
                ..
            })), SwarmEvent::ExternalAddrExpired { .. }],
        ) => {
            assert_eq!(addr_0, test_addr);
            assert_eq!(addr_1, test_addr);
            assert_eq!(req_num, done_num);
        }
        e => panic!("unknown events {e:#?}"),
    }
}

#[tokio::test]
async fn dial_back_to_non_libp2p() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let (mut alice, mut bob) = bootstrap().await;

    for addr_str in [
        "/dns4/umgefahren.xyz/tcp/809",
        "/ip4/169.150.247.38/tcp/32",
        "/ip6/2400:52e0:1e02::1187:1/tcp/1000",
    ] {
        let addr: Multiaddr = addr_str.parse().unwrap();
        bob.behaviour_mut().autonat.inject_test_addr(addr.clone());

        match libp2p_swarm_test::drive(&mut alice, &mut bob).await {
            (
                [SwarmEvent::Dialing { .. }, SwarmEvent::OutgoingConnectionError {
                    error: DialError::Transport(mut peers),
                    ..
                }],
                [SwarmEvent::Behaviour(CombinedClientEvent::Autonat(Report {
                    update:
                        StatusUpdate::GotDialDataReq {
                            addr: addr_0,
                            num_bytes: req_num,
                        },
                    ..
                })), SwarmEvent::Behaviour(CombinedClientEvent::Autonat(Report {
                    update:
                        StatusUpdate::CompletedDialData {
                            addr: addr_1,
                            num_bytes: done_num,
                        },
                    ..
                })), SwarmEvent::ExternalAddrExpired { .. }],
            ) => {
                let (peer_addr, _) = peers.pop().unwrap();
                let cleaned_addr: Multiaddr = peer_addr
                    .iter()
                    .filter(|p| !matches!(p, Protocol::P2p(_)))
                    .collect();
                assert_eq!(addr, cleaned_addr);
                assert_eq!(addr_0, cleaned_addr);
                assert_eq!(addr_1, cleaned_addr);
                assert_eq!(req_num, done_num);
            }
            e => panic!("unknown events {e:#?}"),
        }
    }
}

#[tokio::test]
async fn dial_back_to_not_supporting() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (mut alice, mut bob) = bootstrap().await;

    let mut hannes = new_dummy().await;
    let unreachable_address = hannes.external_addresses().next().unwrap().clone();
    bob.behaviour_mut()
        .autonat
        .inject_test_addr(unreachable_address.clone());

    let handler = tokio::spawn(async move {
        loop {
            hannes.next_swarm_event().await;
        }
    });

    match libp2p_swarm_test::drive(&mut alice, &mut bob).await {
        (
            [SwarmEvent::Dialing { .. }, SwarmEvent::OutgoingConnectionError { .. }],
            [SwarmEvent::Behaviour(CombinedClientEvent::Autonat(Report {
                update:
                    StatusUpdate::GotDialDataReq {
                        addr: addr_0,
                        num_bytes: req_num,
                    },
                ..
            })), SwarmEvent::Behaviour(CombinedClientEvent::Autonat(Report {
                update:
                    StatusUpdate::CompletedDialData {
                        addr: addr_1,
                        num_bytes: done_num,
                    },
                ..
            })), SwarmEvent::ExternalAddrExpired { .. }],
        ) => {
            assert_eq!(addr_0, unreachable_address);
            assert_eq!(addr_1, unreachable_address);
            assert_eq!(req_num, done_num);
        }
        e => panic!("unknown events {e:#?}"),
    }
    handler.abort();
}

async fn new_server() -> Swarm<CombinedServer> {
    let mut node = Swarm::new_ephemeral(|identity| CombinedServer {
        autonat: libp2p_autonatv2::server::Behaviour::default(),
        identify: libp2p_identify::Behaviour::new(libp2p_identify::Config::new(
            "/libp2p-test/1.0.0".into(),
            identity.public().clone(),
        )),
    });
    node.listen().with_tcp_addr_external().await;

    node
}

async fn new_client() -> Swarm<CombinedClient> {
    let mut node = Swarm::new_ephemeral(|identity| CombinedClient {
        autonat: libp2p_autonatv2::client::Behaviour::default(),
        identify: libp2p_identify::Behaviour::new(libp2p_identify::Config::new(
            "/libp2p-test/1.0.0".into(),
            identity.public().clone(),
        )),
    });
    node.listen().with_tcp_addr_external().await;
    node
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct CombinedServer {
    autonat: libp2p_autonatv2::server::Behaviour,
    identify: libp2p_identify::Behaviour,
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct CombinedClient {
    autonat: libp2p_autonatv2::client::Behaviour,
    identify: libp2p_identify::Behaviour,
}

async fn new_dummy() -> Swarm<libp2p_identify::Behaviour> {
    let mut node = Swarm::new_ephemeral(|identity| {
        libp2p_identify::Behaviour::new(libp2p_identify::Config::new(
            "/libp2p-test/1.0.0".into(),
            identity.public().clone(),
        ))
    });
    node.listen().with_tcp_addr_external().await;
    node
}

async fn bootstrap() -> (Swarm<CombinedServer>, Swarm<CombinedClient>) {
    let mut alice = new_server().await;
    let cor_server_peer = alice.local_peer_id().clone();
    let mut bob = new_client().await;
    let cor_client_peer = bob.local_peer_id().clone();
    bob.connect(&mut alice).await;

    match libp2p_swarm_test::drive(&mut alice, &mut bob).await {
        (
            [SwarmEvent::Behaviour(CombinedServerEvent::Identify(libp2p_identify::Event::Sent {
                peer_id: client_peer_sent,
            })), SwarmEvent::Behaviour(CombinedServerEvent::Identify(
                libp2p_identify::Event::Received {
                    peer_id: client_peer_recv,
                    ..
                },
            )), SwarmEvent::NewExternalAddrCandidate { .. }],
            [SwarmEvent::Behaviour(CombinedClientEvent::Identify(libp2p_identify::Event::Sent {
                peer_id: server_peer_sent,
            })), SwarmEvent::Behaviour(CombinedClientEvent::Identify(
                libp2p_identify::Event::Received {
                    peer_id: server_peer_recv,
                    ..
                },
            ))],
        ) => {
            assert_eq!(server_peer_sent, cor_server_peer);
            assert_eq!(client_peer_sent, cor_client_peer);
            assert_eq!(server_peer_recv, cor_server_peer);
            assert_eq!(client_peer_recv, cor_client_peer);
        }
        e => panic!("unexpected events: {e:#?}"),
    }

    match libp2p_swarm_test::drive(&mut alice, &mut bob).await {
        (
            [SwarmEvent::Dialing { .. }, SwarmEvent::ConnectionEstablished { .. }, SwarmEvent::Behaviour(CombinedServerEvent::Identify(_)), SwarmEvent::Behaviour(CombinedServerEvent::Identify(_)), SwarmEvent::NewExternalAddrCandidate { .. }],
            [SwarmEvent::NewExternalAddrCandidate { address: addr_new }, SwarmEvent::IncomingConnection { .. }, SwarmEvent::ConnectionEstablished { .. }, SwarmEvent::Behaviour(CombinedClientEvent::Identify(_)), SwarmEvent::Behaviour(CombinedClientEvent::Identify(_)), SwarmEvent::NewExternalAddrCandidate { address: addr_snd }, SwarmEvent::ExternalAddrConfirmed { address: addr_ok }],
        ) => {
            assert_eq!(addr_new, addr_snd);
            assert_eq!(addr_snd, addr_ok);
        }
        e => panic!("unknown events {e:#?}"),
    }
    (alice, bob)
}
