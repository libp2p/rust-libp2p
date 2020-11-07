use futures::{executor::LocalPool, future::FutureExt, task::Spawn};
use libp2p_core::{
    identity,
    multiaddr::{Multiaddr, Protocol},
    transport::MemoryTransport,
    transport::Transport,
    upgrade,
};
use libp2p_plaintext::PlainText2Config;
use libp2p_relay::{transport::RelayTransportWrapper, Relay};
use libp2p_swarm::{Swarm, SwarmEvent};

fn build_swarm() -> Swarm<Relay> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let plaintext = PlainText2Config {
        local_public_key: local_public_key.clone(),
    };

    let memory_transport = MemoryTransport::default();
    let (relay_wrapped_transport, (to_transport, from_transport)) =
        RelayTransportWrapper::new(memory_transport);

    let transport = relay_wrapped_transport
        .upgrade(upgrade::Version::V1)
        .authenticate(plaintext)
        .multiplex(libp2p_yamux::Config::default())
        .boxed();

    let relay_behaviour = Relay::new(to_transport, from_transport);

    let local_id = local_public_key.clone().into_peer_id();
    Swarm::new(transport, relay_behaviour, local_id)
}

#[test]
fn node_a_relay_via_relay_to_node_b() {
    env_logger::init();

    let mut pool = LocalPool::new();

    let mut node_a_swarm = build_swarm();
    let mut node_b_swarm = build_swarm();
    let mut relay_swarm = build_swarm();

    let node_a_peer_id = Swarm::local_peer_id(&node_a_swarm).clone();
    println!("NodeA: {:?}", node_a_peer_id);
    let node_b_peer_id = Swarm::local_peer_id(&node_b_swarm).clone();
    println!("NodeB: {:?}", node_b_peer_id);
    let node_b_peer_id_clone = node_b_peer_id.clone();
    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();
    println!("Relay: {:?}", relay_peer_id);
    let relay_peer_id_clone = relay_peer_id.clone();
    let relay_peer_id_clone_clone = relay_peer_id.clone();

    let relay_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(node_b_peer_id.into()));

    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            async move {
                loop {
                    // println!("Relay: {:?}",relay_swarm.next_event().await);
                    relay_swarm.next_event().await;
                }
            }
            .boxed()
            .into(),
        )
        .unwrap();

    Swarm::listen_on(&mut node_b_swarm, node_b_address.clone()).unwrap();
    let node_b_incoming_connection_established = async move {
        loop {
            match node_b_swarm.next_event().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == node_a_peer_id => {
                    println!("node b connected to node a");
                    return;
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    assert_eq!(peer_id, relay_peer_id_clone);
                    println!("NodeB->Relay");
                }
                SwarmEvent::Dialing(peer_id) => assert_eq!(peer_id, relay_peer_id_clone),
                SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                    assert_eq!(
                        send_back_addr,
                        Protocol::P2p(node_a_peer_id.clone().into()).into()
                    );
                    println!("NodeA->NodeB")
                }
                e => panic!("{:?}", e),
            }
        }
    };

    // Start node a dialing node b via relay.
    Swarm::dial_addr(&mut node_a_swarm, node_b_address).unwrap();
    let node_a_dial_connection_established = async move {
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. }
                    if peer_id == node_b_peer_id_clone =>
                {
                    println!("node a connected to node b");
                    return;
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    assert_eq!(peer_id, relay_peer_id_clone_clone);
                    println!("NodeA->Relay");
                }
                SwarmEvent::Dialing(peer_id) => assert_eq!(peer_id, relay_peer_id_clone_clone),
                e => panic!("{:?}", e),
            }
        }
    };

    pool.run_until(async move {
        futures::future::join(
            node_b_incoming_connection_established,
            node_a_dial_connection_established,
        )
        .await
    });
}
