use futures::{executor::LocalPool, task::Spawn, FutureExt, StreamExt};
use libp2p_core::{
    identity, multiaddr::Protocol, transport::MemoryTransport, upgrade::Version, Multiaddr,
    Transport,
};
use libp2p_perf::{client, server, RunParams};
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
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let local_peer_id = local_public_key.to_peer_id();

        let transport = MemoryTransport::default()
            .boxed()
            .upgrade(Version::V1)
            .authenticate(PlainText2Config { local_public_key })
            .multiplex(YamuxConfig::default())
            .boxed();

        let mut server = Swarm::without_executor(
            transport,
            server::behaviour::Behaviour::new(),
            local_peer_id,
        );

        server.listen_on(server_address.clone()).unwrap();

        pool.spawner()
            .spawn_obj(server.collect::<Vec<_>>().map(|_| ()).boxed().into())
            .unwrap();
    }

    let mut client = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let local_peer_id = local_public_key.to_peer_id();

        let transport = MemoryTransport::default()
            .boxed()
            .upgrade(Version::V1)
            .authenticate(PlainText2Config { local_public_key })
            .multiplex(YamuxConfig::default())
            .boxed();

        let client = Swarm::without_executor(
            transport,
            client::behaviour::Behaviour::new(),
            local_peer_id,
        );

        client
    };

    client.behaviour_mut().perf(
        server_address,
        RunParams {
            to_send: 0,
            to_receive: 0,
        },
    );

    pool.run_until(async {
        loop {
            match client.select_next_some().await {
                SwarmEvent::IncomingConnection { .. } => panic!(),
                SwarmEvent::ConnectionEstablished { .. } => {}
                SwarmEvent::Dialing(_) => {}
                SwarmEvent::Behaviour(client::behaviour::Event::Finished { stats: _ }) => break,
                e => panic!("{e:?}"),
            }
        }
    });
}
