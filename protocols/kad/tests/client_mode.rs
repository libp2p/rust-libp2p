use libp2p_identify as identify;
use libp2p_identity as identity;
use libp2p_kad::store::MemoryStore;
use libp2p_kad::{Behaviour, Config, Event, Mode};
use libp2p_swarm::Swarm;
use libp2p_swarm_test::SwarmExt;

#[async_std::test]
async fn server_gets_added_to_routing_table_by_client() {
    let _ = env_logger::try_init();

    let mut client = Swarm::new_ephemeral(MyBehaviour::new);
    let mut server = Swarm::new_ephemeral(MyBehaviour::new);

    server.listen().await;
    client.connect(&mut server).await;

    let server_peer_id = *server.local_peer_id();

    match libp2p_swarm_test::drive(&mut client, &mut server).await {
        (
            [MyBehaviourEvent::Identify(_), MyBehaviourEvent::Identify(_), MyBehaviourEvent::Kad(Event::RoutingUpdated { peer, .. })],
            [MyBehaviourEvent::Identify(_), MyBehaviourEvent::Identify(_)],
        ) => {
            assert_eq!(peer, server_peer_id)
        }
        other => panic!("Unexpected events: {other:?}"),
    }
}

#[async_std::test]
async fn two_servers_add_each_other_to_routing_table() {
    let _ = env_logger::try_init();

    let mut server1 = Swarm::new_ephemeral(MyBehaviour::new);
    let mut server2 = Swarm::new_ephemeral(MyBehaviour::new);

    server2.listen().await;
    server1.connect(&mut server2).await;

    let server1_peer_id = *server1.local_peer_id();
    let server2_peer_id = *server2.local_peer_id();

    use Event::*;
    use MyBehaviourEvent::*;

    match libp2p_swarm_test::drive(&mut server1, &mut server2).await {
        (
            [Identify(_), Identify(_), Kad(RoutingUpdated { peer: peer1, .. })],
            [Identify(_), Identify(_)],
        ) => {
            assert_eq!(peer1, server2_peer_id);
        }
        other => panic!("Unexpected events: {other:?}"),
    }

    server1.listen().await;
    server2.connect(&mut server1).await;

    match libp2p_swarm_test::drive(&mut server2, &mut server1).await {
        (
            [Identify(_), Kad(RoutingUpdated { peer: peer2, .. }), Identify(_)],
            [Identify(_), Identify(_)],
        )
        | (
            [Identify(_), Identify(_), Kad(RoutingUpdated { peer: peer2, .. })],
            [Identify(_), Identify(_)],
        ) => {
            assert_eq!(peer2, server1_peer_id);
        }
        other => panic!("Unexpected events: {other:?}"),
    }
}

#[async_std::test]
async fn adding_an_external_addresses_activates_server_mode_on_existing_connections() {
    let _ = env_logger::try_init();

    let mut client = Swarm::new_ephemeral(MyBehaviour::new);
    let mut server = Swarm::new_ephemeral(MyBehaviour::new);
    let server_peer_id = *server.local_peer_id();

    let (memory_addr, _) = server.listen().await;

    // Remove memory address to simulate a server that doesn't know its external address.
    server.remove_external_address(&memory_addr);
    client.dial(memory_addr.clone()).unwrap();

    use MyBehaviourEvent::*;

    // Do the usual identify send/receive dance. This triggers a mode change to Mode::Client.
    match libp2p_swarm_test::drive(&mut client, &mut server).await {
        ([Identify(_), Identify(_)], [Kad(ModeChanged { new_mode }), Identify(_), Identify(_)]) => {
            assert_eq!(new_mode, Mode::Client);
        }
        other => panic!("Unexpected events: {other:?}"),
    }

    use Event::*;

    // Server learns its external address (this could be through AutoNAT or some other mechanism).
    server.add_external_address(memory_addr);

    // The server reconfigured its connection to the client to be in server mode, pushes that information to client which as a result updates its routing table and triggers a mode change to Mode::Server.
    match libp2p_swarm_test::drive(&mut client, &mut server).await {
        (
            [Identify(identify::Event::Received { .. }), Kad(RoutingUpdated { peer: peer1, .. })],
            [Kad(ModeChanged { new_mode }), Identify(identify::Event::Pushed { .. })],
        ) => {
            assert_eq!(new_mode, Mode::Server);
            assert_eq!(peer1, server_peer_id);
        }
        other => panic!("Unexpected events: {other:?}"),
    }
}

#[async_std::test]
async fn set_client_to_server_mode() {
    let _ = env_logger::try_init();

    let mut client = Swarm::new_ephemeral(MyBehaviour::new);
    client.behaviour_mut().kad.set_mode(Some(Mode::Client));

    let mut server = Swarm::new_ephemeral(MyBehaviour::new);

    server.listen().await;
    client.connect(&mut server).await;

    let server_peer_id = *server.local_peer_id();

    match libp2p_swarm_test::drive(&mut client, &mut server).await {
        (
            [MyBehaviourEvent::Identify(_), MyBehaviourEvent::Identify(_), MyBehaviourEvent::Kad(Event::RoutingUpdated { peer, .. })],
            [MyBehaviourEvent::Identify(_), MyBehaviourEvent::Identify(identify::Event::Received { info, .. })],
        ) => {
            assert_eq!(peer, server_peer_id);
            assert!(info
                .protocols
                .iter()
                .all(|proto| libp2p_kad::PROTOCOL_NAME.ne(proto)))
        }
        other => panic!("Unexpected events: {other:?}"),
    }

    client.behaviour_mut().kad.set_mode(Some(Mode::Server));

    match libp2p_swarm_test::drive(&mut client, &mut server).await {
        (
            [MyBehaviourEvent::Identify(_)],
            [MyBehaviourEvent::Identify(identify::Event::Received { info, .. }), MyBehaviourEvent::Kad(_)],
        ) => {
            assert!(info
                .protocols
                .iter()
                .any(|proto| libp2p_kad::PROTOCOL_NAME.eq(proto)))
        }
        other => panic!("Unexpected events: {other:?}"),
    }
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct MyBehaviour {
    identify: identify::Behaviour,
    kad: Behaviour<MemoryStore>,
}

impl MyBehaviour {
    fn new(k: identity::Keypair) -> Self {
        let local_peer_id = k.public().to_peer_id();

        Self {
            identify: identify::Behaviour::new(identify::Config::new(
                "/test/1.0.0".to_owned(),
                k.public(),
            )),
            kad: Behaviour::with_config(
                local_peer_id,
                MemoryStore::new(local_peer_id),
                Config::default(),
            ),
        }
    }
}
