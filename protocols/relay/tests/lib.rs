use futures::{executor::LocalPool, future::FutureExt, task::Spawn};
use libp2p_core::{
    identity,
    multiaddr::{Multiaddr, Protocol},
    transport::MemoryTransport,
    transport::{ListenerEvent, Transport},
    upgrade,
};
use libp2p_plaintext::PlainText2Config;
use libp2p_relay::{transport::RelayTransportWrapper, Relay};
use libp2p_swarm::Swarm;
use rand::random;
use std::iter::FromIterator;

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
    let mut swarm = Swarm::new(transport, relay_behaviour, local_id);

    swarm
}

#[test]
fn node_a_relay_via_relay_to_node_b() {
    env_logger::init();

    let mut pool = LocalPool::new();

    // Start relay.
    let mut relay_swarm = build_swarm();
    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();
    let relay_address: Multiaddr = Protocol::Memory(random::<u64>()).into();
    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            async move {
                loop {
                    relay_swarm.next_event().await;
                }
            }
            .boxed()
            .into(),
        )
        .unwrap();

    // Start node b listening via relay.
    let mut node_b_swarm = build_swarm();
    let node_b_peer_id = Swarm::local_peer_id(&node_b_swarm).clone();
    let node_b_address = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(node_b_peer_id.into()));
    Swarm::listen_on(&mut node_b_swarm, node_b_address.clone()).unwrap();
    pool.spawner().spawn_obj(async move {
        loop {
            node_b_swarm.next_event().await;
        }
    }.boxed().into());

    // Start node a dialing node b via relay.
    let mut node_a_swarm = build_swarm();
    Swarm::dial_addr(&mut node_a_swarm, node_b_address);
    pool.run_until(async move {
        loop {
            node_a_swarm.next_event().await;
        }
    });
}
