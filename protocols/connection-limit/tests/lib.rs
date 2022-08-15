// TODO: Should imports go through libp2p crate?
use futures::executor::LocalPool;
use futures::future;
use futures::task::LocalSpawn;
use futures::task::LocalSpawnExt;
use futures::task::Spawn;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use libp2p::NetworkBehaviour;
use libp2p_connection_limit::ConnectionLimits;
use libp2p_core::connection::{ConnectionId, Endpoint};
use libp2p_core::identity;
use libp2p_core::identity::PublicKey;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::Boxed;
use libp2p_core::transport::TransportEvent;
use libp2p_core::transport::{MemoryTransport, Transport};
use libp2p_core::upgrade;
use libp2p_core::Multiaddr;
use libp2p_core::PeerId;
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::dial_opts::DialOpts;
use libp2p_swarm::DummyBehaviour;
use libp2p_swarm::KeepAlive;
use libp2p_swarm::Swarm;
use libp2p_swarm::SwarmBuilder;
use libp2p_swarm::SwarmEvent;
use libp2p_yamux::YamuxConfig;
use quickcheck::QuickCheck;

// TODO: Test that pending limit is decreased.

#[test]
fn enforces_pending_outbound_connection_limit() {
    fn prop(outbound: u8) {
        let limits = ConnectionLimits::default().with_max_pending_outgoing(Some(outbound.into()));

        let mut swarm = make_swarm(limits);

        let addr: Multiaddr = "/memory/1234".parse().unwrap();

        for _ in 0..outbound {
            swarm
                .dial(
                    DialOpts::peer_id(PeerId::random())
                        .addresses(vec![addr.clone()])
                        .build(),
                )
                .ok()
                .expect("Unexpected connection limit.");
        }

        assert!(swarm
            .dial(
                DialOpts::peer_id(PeerId::random())
                    .addresses(vec![addr.clone()])
                    .build(),
            )
            .is_err());
    }

    QuickCheck::new().quickcheck(prop as fn(_));
}

#[test]
fn enforces_pending_inbound_connection_limit() {
    fn prop(inbound: u8) {
        let mut pool = LocalPool::default();

        let limits = ConnectionLimits::default().with_max_pending_incoming(Some(inbound.into()));
        let mut swarm = make_swarm(limits);

        swarm.listen_on("/memory/0".parse().unwrap()).unwrap();
        let listen_addr = match pool.run_until(swarm.next()).unwrap() {
            SwarmEvent::NewListenAddr { address, .. } => address,
            e => panic!("Unexpected event {:?}", e),
        };

        pool.spawner().spawn_local(async move {
            let mut remote_transport = MemoryTransport::default();

            let dials = (0..inbound + 1)
                .map(|_| remote_transport.dial(listen_addr.clone()).unwrap())
                .collect::<Vec<_>>();

            future::join(
                future::try_join_all(dials),
                Transport::boxed(remote_transport).collect::<Vec<TransportEvent<_, _>>>(),
            )
            .await
            .0
            .unwrap();
        });

        for i in 0..inbound {
            match pool.run_until(swarm.next()).unwrap() {
                SwarmEvent::IncomingConnection { .. } => {}
                e => panic!("Unexpected event {:?}", e),
            }
        }

        match pool.run_until(swarm.next()).unwrap() {
            SwarmEvent::IncomingConnectionDenied => {}
            e => panic!("Unexpected event {:?}", e),
        }
    }

    QuickCheck::new().quickcheck(prop as fn(_));
}

#[test]
fn enforces_established_outbound_connection_limit() {
    fn prop(outbound: u8) {
        let mut pool = LocalPool::default();
        let limits =
            ConnectionLimits::default().with_max_established_outgoing(Some(outbound.into()));

        let mut local_swarm = make_swarm(limits);
        let mut remote_transport = make_transport(identity::Keypair::generate_ed25519().public());

        remote_transport
            .listen_on("/memory/0".parse().unwrap())
            .unwrap();
        let remote_addr = match pool.run_until(remote_transport.next()).unwrap() {
            TransportEvent::NewAddress { listen_addr, .. } => listen_addr,
            e => panic!("Unexpected event {:?}", e),
        };

        pool.spawner()
            .spawn_local({
                remote_transport
                    .flat_map(|e| match e {
                        TransportEvent::Incoming { upgrade, .. } => upgrade.into_stream(),
                        e => panic!("Unpexted event {:?}", e),
                    })
                    .try_collect::<Vec<(PeerId, StreamMuxerBox)>>()
                    .map(|r| r.map(|_| ()).unwrap())
            })
            .unwrap();

        for i in 0..outbound {
            println!("{:?}", i);
            local_swarm
                .dial(remote_addr.clone())
                .ok()
                .expect("Unexpected connection limit.");

            match pool.run_until(local_swarm.next()).unwrap() {
                SwarmEvent::ConnectionEstablished { .. } => {}
                e => panic!("Unexpected event {:?}", e),
            }
        }

        assert!(local_swarm.dial(remote_addr).is_err());
    }

    QuickCheck::new().quickcheck(prop as fn(_));
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    limit: libp2p_connection_limit::Behaviour,
    keep_alive: DummyBehaviour,
}

fn make_swarm(limits: ConnectionLimits) -> Swarm<Behaviour> {
    let behaviour = Behaviour {
        limit: libp2p_connection_limit::Behaviour::new(limits),
        keep_alive: DummyBehaviour::with_keep_alive(KeepAlive::Yes),
    };
    let local_public_key = identity::Keypair::generate_ed25519().public();
    let transport = make_transport(local_public_key.clone());
    SwarmBuilder::new(transport, behaviour, local_public_key.into()).build()
}

fn make_transport(local_public_key: PublicKey) -> Boxed<(PeerId, StreamMuxerBox)> {
    MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(PlainText2Config {
            local_public_key: local_public_key.clone(),
        })
        .multiplex(YamuxConfig::default())
        .boxed()
}
