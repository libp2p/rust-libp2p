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
use futures::{executor::LocalPool, future::FutureExt, task::Spawn};

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
fn connect_to_relay() {
    let mut pool = LocalPool::new();

    let mut relay_swarm = build_swarm();
    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();
    let relay_address: Multiaddr = Protocol::Memory(random::<u64>()).into();
    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner().spawn_obj(async move {
        loop {
            relay_swarm.next().await;
        }
    }.boxed().into());

    let mut node_a_swarm = build_swarm();
    let node_a_peer_id = Swarm::local_peer_id(&node_a_swarm).clone();
    let node_a_address = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit);
    Swarm::listen_on(&mut node_a_swarm, node_a_address.clone()).unwrap();

    pool.run_until(async move {
        loop {
            node_a_swarm.next().await;
        }
    });
}
