use std::{sync::Arc, time::Duration};

use libp2p_autonat::v2::{
    client::{self, Config},
    server,
};
use libp2p_core::{multiaddr::Protocol, transport::TransportError, Multiaddr};
use libp2p_swarm::{
    DialError, FromSwarm, NetworkBehaviour, NewExternalAddrCandidate, Swarm, SwarmEvent,
};
use libp2p_swarm_test::SwarmExt;
use rand_core::OsRng;
use tokio::sync::oneshot;
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn confirm_successful() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let (mut alice, mut bob) = start_and_connect().await;

    let cor_server_peer = *alice.local_peer_id();
    let cor_client_peer = *bob.local_peer_id();
    let bob_tcp_listeners = Arc::new(tcp_listeners(&bob));
    let alice_bob_tcp_listeners = bob_tcp_listeners.clone();

    let alice_task = async {
        let (dialed_peer_id, dialed_connection_id) = alice
            .wait(|event| match event {
                SwarmEvent::Dialing {
                    peer_id,
                    connection_id,
                    ..
                } => peer_id.map(|peer_id| (peer_id, connection_id)),
                _ => None,
            })
            .await;

        assert_eq!(dialed_peer_id, cor_client_peer);

        let _ = alice
            .wait(|event| match event {
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    connection_id,
                    ..
                } if peer_id == dialed_peer_id
                    && peer_id == cor_client_peer
                    && connection_id == dialed_connection_id =>
                {
                    Some(())
                }
                _ => None,
            })
            .await;

        let server::Event {
            all_addrs,
            tested_addr,
            client,
            data_amount,
            result,
        } = alice
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedServerEvent::Autonat(status_update)) => {
                    Some(status_update)
                }
                _ => None,
            })
            .await;

        assert_eq!(tested_addr, bob_tcp_listeners.first().cloned().unwrap());
        assert_eq!(data_amount, 0);
        assert_eq!(client, cor_client_peer);
        assert_eq!(&all_addrs[..], &bob_tcp_listeners[..]);
        assert!(result.is_ok(), "Result: {result:?}");
    };

    let bob_task = async {
        bob.wait(|event| match event {
            SwarmEvent::NewExternalAddrCandidate { address } => Some(address),
            _ => None,
        })
        .await;
        let incoming_conn_id = bob
            .wait(|event| match event {
                SwarmEvent::IncomingConnection { connection_id, .. } => Some(connection_id),
                _ => None,
            })
            .await;

        let _ = bob
            .wait(|event| match event {
                SwarmEvent::ConnectionEstablished {
                    connection_id,
                    peer_id,
                    ..
                } if incoming_conn_id == connection_id && peer_id == cor_server_peer => Some(()),
                _ => None,
            })
            .await;

        let client::Event {
            tested_addr,
            bytes_sent,
            server,
            result,
        } = bob
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedClientEvent::Autonat(status_update)) => {
                    Some(status_update)
                }
                _ => None,
            })
            .await;
        assert_eq!(
            tested_addr,
            alice_bob_tcp_listeners.first().cloned().unwrap()
        );
        assert_eq!(bytes_sent, 0);
        assert_eq!(server, cor_server_peer);
        assert!(result.is_ok(), "Result is {result:?}");
    };

    tokio::join!(alice_task, bob_task);
}

#[tokio::test]
async fn dial_back_to_unsupported_protocol() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let (mut alice, mut bob) = bootstrap().await;

    let alice_peer_id = *alice.local_peer_id();

    let test_addr: Multiaddr = "/ip4/127.0.0.1/udp/1234/quic/webtransport".parse().unwrap();
    let bob_test_addr = test_addr.clone();
    bob.behaviour_mut()
        .autonat
        .on_swarm_event(FromSwarm::NewExternalAddrCandidate(
            NewExternalAddrCandidate { addr: &test_addr },
        ));

    let (bob_done_tx, bob_done_rx) = oneshot::channel();

    let alice_task = async {
        let (alice_dialing_peer, alice_conn_id) = alice
            .wait(|event| match event {
                SwarmEvent::Dialing {
                    peer_id,
                    connection_id,
                } => peer_id.map(|e| (e, connection_id)),
                _ => None,
            })
            .await;
        let mut outgoing_conn_error = alice
            .wait(|event| match event {
                SwarmEvent::OutgoingConnectionError {
                    connection_id,
                    peer_id: Some(peer_id),
                    error: DialError::Transport(transport_errs),
                } if connection_id == alice_conn_id && alice_dialing_peer == peer_id => {
                    Some(transport_errs)
                }
                _ => None,
            })
            .await;
        if let Some((multiaddr, TransportError::MultiaddrNotSupported(not_supported_addr))) =
            outgoing_conn_error.pop()
        {
            assert_eq!(
                multiaddr,
                test_addr.clone().with_p2p(alice_dialing_peer).unwrap()
            );
            assert_eq!(not_supported_addr, multiaddr,);
        } else {
            panic!("Peers are empty");
        }
        assert_eq!(outgoing_conn_error.len(), 0);
        let data_amount = alice
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedServerEvent::Autonat(server::Event {
                    all_addrs,
                    tested_addr,
                    client,
                    data_amount,
                    result: Ok(()),
                })) if all_addrs == vec![test_addr.clone()]
                    && tested_addr == test_addr.clone()
                    && client == alice_dialing_peer =>
                {
                    Some(data_amount)
                }
                _ => None,
            })
            .await;

        let handler = tokio::spawn(async move {
            alice.loop_on_next().await;
        });
        let _ = bob_done_rx.await;
        handler.abort();
        data_amount
    };

    let bob_task = async {
        let data_amount = bob
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedClientEvent::Autonat(client::Event {
                    tested_addr,
                    bytes_sent,
                    server,
                    result: Err(_),
                })) if server == alice_peer_id && tested_addr == bob_test_addr => Some(bytes_sent),
                _ => None,
            })
            .await;
        bob_done_tx.send(()).unwrap();
        data_amount
    };
    let (alice_amount, bob_amount) = tokio::join!(alice_task, bob_task);
    assert_eq!(alice_amount, bob_amount);
}

#[tokio::test]
async fn dial_back_to_non_libp2p() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let (mut alice, mut bob) = bootstrap().await;
    let alice_peer_id = *alice.local_peer_id();

    let addr_str = "/ip6/::1/tcp/1000";
    let addr: Multiaddr = addr_str.parse().unwrap();
    let bob_addr = addr.clone();
    bob.behaviour_mut()
        .autonat
        .on_swarm_event(FromSwarm::NewExternalAddrCandidate(
            NewExternalAddrCandidate { addr: &addr },
        ));

    let alice_task = async {
        let (alice_dialing_peer, alice_conn_id) = alice
            .wait(|event| match event {
                SwarmEvent::Dialing {
                    peer_id,
                    connection_id,
                } => peer_id.map(|p| (p, connection_id)),
                _ => None,
            })
            .await;
        let mut outgoing_conn_error = alice
            .wait(|event| match event {
                SwarmEvent::OutgoingConnectionError {
                    connection_id,
                    peer_id: Some(peer_id),
                    error: DialError::Transport(peers),
                } if connection_id == alice_conn_id && peer_id == alice_dialing_peer => Some(peers),
                _ => None,
            })
            .await;

        if let Some((multiaddr, TransportError::Other(o))) = outgoing_conn_error.pop() {
            assert_eq!(
                multiaddr,
                addr.clone().with_p2p(alice_dialing_peer).unwrap()
            );
            let error_string = o.to_string();
            assert!(
                error_string.contains("Connection refused"),
                "Correct error string: {error_string} for {addr_str}"
            );
        } else {
            panic!("No outgoing connection errors");
        }

        alice
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedServerEvent::Autonat(server::Event {
                    all_addrs,
                    tested_addr,
                    client,
                    data_amount,
                    result: Ok(()),
                })) if all_addrs == vec![addr.clone()]
                    && tested_addr == addr
                    && alice_dialing_peer == client =>
                {
                    Some(data_amount)
                }
                _ => None,
            })
            .await
    };
    let bob_task = async {
        bob.wait(|event| match event {
            SwarmEvent::Behaviour(CombinedClientEvent::Autonat(client::Event {
                tested_addr,
                bytes_sent,
                server,
                result: Err(_),
            })) if tested_addr == bob_addr && server == alice_peer_id => Some(bytes_sent),
            _ => None,
        })
        .await
    };

    let (alice_bytes_sent, bob_bytes_sent) = tokio::join!(alice_task, bob_task);
    assert_eq!(alice_bytes_sent, bob_bytes_sent);
    bob.behaviour_mut().autonat.validate_addr(&addr);
}

#[tokio::test]
async fn dial_back_to_not_supporting() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (mut alice, mut bob) = bootstrap().await;
    let alice_peer_id = *alice.local_peer_id();

    let (bob_done_tx, bob_done_rx) = oneshot::channel();

    let hannes = new_dummy().await;
    let hannes_peer_id = *hannes.local_peer_id();
    let unreachable_address = hannes.external_addresses().next().unwrap().clone();
    let bob_unreachable_address = unreachable_address.clone();
    bob.behaviour_mut()
        .autonat
        .on_swarm_event(FromSwarm::NewExternalAddrCandidate(
            NewExternalAddrCandidate {
                addr: &unreachable_address,
            },
        ));

    let handler = tokio::spawn(async { hannes.loop_on_next().await });

    let alice_task = async {
        let (alice_dialing_peer, alice_conn_id) = alice
            .wait(|event| match event {
                SwarmEvent::Dialing {
                    peer_id,
                    connection_id,
                } => peer_id.map(|p| (p, connection_id)),
                _ => None,
            })
            .await;
        alice
            .wait(|event| match event {
                SwarmEvent::OutgoingConnectionError {
                    connection_id,
                    peer_id: Some(peer_id),
                    error: DialError::WrongPeerId { obtained, .. },
                } if connection_id == alice_conn_id
                    && peer_id == alice_dialing_peer
                    && obtained == hannes_peer_id =>
                {
                    Some(())
                }
                _ => None,
            })
            .await;

        let data_amount = alice
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedServerEvent::Autonat(server::Event {
                    all_addrs,
                    tested_addr,
                    client,
                    data_amount,
                    result: Ok(()),
                })) if all_addrs == vec![unreachable_address.clone()]
                    && tested_addr == unreachable_address
                    && alice_dialing_peer == client =>
                {
                    Some(data_amount)
                }
                _ => None,
            })
            .await;
        tokio::select! {
            _ = bob_done_rx => {
                data_amount
            }
            _ = alice.loop_on_next() => {
                unreachable!();
            }
        }
    };

    let bob_task = async {
        let bytes_sent = bob
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedClientEvent::Autonat(client::Event {
                    tested_addr,
                    bytes_sent,
                    server,
                    result: Err(_),
                })) if tested_addr == bob_unreachable_address && server == alice_peer_id => {
                    Some(bytes_sent)
                }
                _ => None,
            })
            .await;
        bob_done_tx.send(()).unwrap();
        bytes_sent
    };

    let (alice_bytes_sent, bob_bytes_sent) = tokio::join!(alice_task, bob_task);
    assert_eq!(alice_bytes_sent, bob_bytes_sent);
    handler.abort();
}

async fn new_server() -> Swarm<CombinedServer> {
    let mut node = Swarm::new_ephemeral(|identity| CombinedServer {
        autonat: libp2p_autonat::v2::server::Behaviour::default(),
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
        autonat: libp2p_autonat::v2::client::Behaviour::new(
            OsRng,
            Config::default().with_probe_interval(Duration::from_millis(100)),
        ),
        identify: libp2p_identify::Behaviour::new(libp2p_identify::Config::new(
            "/libp2p-test/1.0.0".into(),
            identity.public().clone(),
        )),
    });
    node.listen().await;
    node
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct CombinedServer {
    autonat: libp2p_autonat::v2::server::Behaviour,
    identify: libp2p_identify::Behaviour,
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct CombinedClient {
    autonat: libp2p_autonat::v2::client::Behaviour,
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

async fn start_and_connect() -> (Swarm<CombinedServer>, Swarm<CombinedClient>) {
    let mut alice = new_server().await;
    let mut bob = new_client().await;

    bob.connect(&mut alice).await;
    (alice, bob)
}

async fn bootstrap() -> (Swarm<CombinedServer>, Swarm<CombinedClient>) {
    let (mut alice, mut bob) = start_and_connect().await;

    let cor_server_peer = *alice.local_peer_id();
    let cor_client_peer = *bob.local_peer_id();

    let alice_task = async {
        let (dialed_peer_id, dialed_connection_id) = alice
            .wait(|event| match event {
                SwarmEvent::Dialing {
                    peer_id,
                    connection_id,
                    ..
                } => peer_id.map(|peer_id| (peer_id, connection_id)),
                _ => None,
            })
            .await;

        let _ = alice
            .wait(|event| match event {
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    connection_id,
                    ..
                } if peer_id == dialed_peer_id
                    && peer_id == cor_client_peer
                    && connection_id == dialed_connection_id =>
                {
                    Some(())
                }
                _ => None,
            })
            .await;

        alice
            .wait(|event| match event {
                SwarmEvent::Behaviour(CombinedServerEvent::Autonat(_)) => Some(()),
                _ => None,
            })
            .await;
    };

    let bob_task = async {
        bob.wait(|event| match event {
            SwarmEvent::NewExternalAddrCandidate { address } => Some(address),
            _ => None,
        })
        .await;
        let incoming_conn_id = bob
            .wait(|event| match event {
                SwarmEvent::IncomingConnection { connection_id, .. } => Some(connection_id),
                _ => None,
            })
            .await;

        let _ = bob
            .wait(|event| match event {
                SwarmEvent::ConnectionEstablished {
                    connection_id,
                    peer_id,
                    ..
                } if incoming_conn_id == connection_id && peer_id == cor_server_peer => Some(()),
                _ => None,
            })
            .await;

        bob.wait(|event| match event {
            SwarmEvent::Behaviour(CombinedClientEvent::Autonat(_)) => Some(()),
            _ => None,
        })
        .await;
    };

    tokio::join!(alice_task, bob_task);
    (alice, bob)
}

fn tcp_listeners<T: NetworkBehaviour>(swarm: &Swarm<T>) -> Vec<Multiaddr> {
    swarm
        .listeners()
        .filter(|addr| {
            addr.iter()
                .any(|protocol| matches!(protocol, Protocol::Tcp(_)))
        })
        .cloned()
        .collect::<Vec<_>>()
}
