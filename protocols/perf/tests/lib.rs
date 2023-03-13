use futures::{executor::LocalPool, task::Spawn, FutureExt, StreamExt};
use libp2p_core::{
    multiaddr::Protocol, transport::MemoryTransport, upgrade::Version, Multiaddr, Transport,
};
use libp2p_perf::{
    client::{self, RunParams},
    server,
};
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_yamux::YamuxConfig;

#[test]
fn perf() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let server_address = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));

    // Spawn server
    {
        let local_key = libp2p_identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let local_peer_id = local_public_key.to_peer_id();

        let transport = MemoryTransport::default()
            .boxed()
            .upgrade(Version::V1)
            .authenticate(PlainText2Config { local_public_key })
            .multiplex(YamuxConfig::default())
            .boxed();

        let mut server =
            Swarm::without_executor(transport, server::Behaviour::new(), local_peer_id);

        server.listen_on(server_address.clone()).unwrap();

        pool.spawner()
            .spawn_obj(server.collect::<Vec<_>>().map(|_| ()).boxed().into())
            .unwrap();
    }

    let mut client = {
        let local_key = libp2p_identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let local_peer_id = local_public_key.to_peer_id();

        let transport = MemoryTransport::default()
            .boxed()
            .upgrade(Version::V1)
            .authenticate(PlainText2Config { local_public_key })
            .multiplex(YamuxConfig::default())
            .boxed();

        Swarm::without_executor(transport, client::Behaviour::new(), local_peer_id)
    };

    client.dial(server_address).unwrap();
    let server_peer_id = pool.run_until(async {
        loop {
            match client.next().await.unwrap() {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => return peer_id,
                SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                    panic!("Outgoing connection error to {peer_id:?}: {error:?}");
                }
                e => panic!("{e:?}"),
            }
        }
    });

    client
        .behaviour_mut()
        .perf(
            server_peer_id,
            RunParams {
                to_send: 0,
                to_receive: 0,
            },
        )
        .unwrap();

    pool.run_until(async {
        loop {
            match client.select_next_some().await {
                SwarmEvent::IncomingConnection { .. } => panic!(),
                SwarmEvent::ConnectionEstablished { .. } => {}
                SwarmEvent::Dialing(_) => {}
                SwarmEvent::Behaviour(client::Event { result: Ok(_), .. }) => break,
                e => panic!("{e:?}"),
            }
        }
    });
}
