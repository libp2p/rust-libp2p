use libp2p_swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn confirm_successful() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
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
}

#[tokio::test]
async fn dial_back_to_not_supporting() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
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

    println!("Standard succeed");

    let mut hannes = new_dummy().await;
    let unreachable_address = hannes.external_addresses().next().unwrap().clone();
    bob.behaviour_mut()
        .autonat
        .inject_test_addr(unreachable_address.clone());

    tokio::spawn(async move {
        loop {
            let hannes_event = hannes.next_swarm_event().await;
            println!("Hannes: {hannes_event:?}");
        }
    });

    tokio::spawn(async move {
        loop {
            let bob_event = bob.next_swarm_event().await;
            println!("Bob: {bob_event:?}");
        }
    });

    loop {
        let alice_event = alice.next_swarm_event().await;
        println!("Alice: {alice_event:?}");
    }
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
