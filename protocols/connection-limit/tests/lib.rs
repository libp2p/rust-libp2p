// TODO: Should imports go through libp2p crate?
use futures::executor::LocalPool;
use futures::task::LocalSpawn;
use futures::task::LocalSpawnExt;
use futures::task::Spawn;
use futures::FutureExt;
use futures::StreamExt;
use libp2p_connection_limit::{Behaviour, ConnectionLimits};
use libp2p_core::connection::{ConnectionId, Endpoint};
use libp2p_core::identity;
use libp2p_core::transport::TransportEvent;
use libp2p_core::transport::{MemoryTransport, Transport};
use libp2p_core::upgrade;
use libp2p_core::Multiaddr;
use libp2p_core::PeerId;
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::behaviour::NetworkBehaviour;
use libp2p_swarm::dial_opts::DialOpts;
use libp2p_swarm::SwarmBuilder;
use libp2p_swarm::SwarmEvent;
use libp2p_yamux::YamuxConfig;
use quickcheck::QuickCheck;

#[test]
fn enforces_pending_outbound_connection_limit() {
    fn prop(outbound: u8) {
        let limit = ConnectionLimits::default().with_max_pending_outgoing(Some(outbound.into()));

        let behaviour = Behaviour::new(limit);
        let id_keys = identity::Keypair::generate_ed25519();
        let local_public_key = id_keys.public();
        let transport = MemoryTransport::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(PlainText2Config {
                local_public_key: local_public_key.clone(),
            })
            .multiplex(YamuxConfig::default())
            .boxed();
        let mut swarm = SwarmBuilder::new(transport, behaviour, local_public_key.into()).build();

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

        let mut swarm = {
            let local_public_key = identity::Keypair::generate_ed25519().public();
            // TODO: Abstract into method
            let transport = MemoryTransport::default()
                .upgrade(upgrade::Version::V1)
                .authenticate(PlainText2Config {
                    local_public_key: local_public_key.clone(),
                })
                .multiplex(YamuxConfig::default())
                .boxed();
            let limit = ConnectionLimits::default().with_max_pending_incoming(Some(inbound.into()));
            SwarmBuilder::new(transport, Behaviour::new(limit), local_public_key.into()).build()
        };

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

            // TODO: What if any of them fails?
            futures::future::join(
                futures::future::try_join_all(dials),
                Transport::boxed(remote_transport).collect::<Vec<TransportEvent<_, _>>>(),
            )
            .await;
        });

        for i in 0..inbound {
            println!("{:?}", i);
            match pool.run_until(swarm.next()).unwrap() {
                SwarmEvent::IncomingConnection { .. } => {}
                e => panic!("Unexpected event {:?}", e),
            }
        }

        match pool.run_until(swarm.next()).unwrap() {
            SwarmEvent::IncomingConnectionError { .. } => {}
            e => panic!("Unexpected event {:?}", e),
        }
    }

    QuickCheck::new().quickcheck(prop as fn(_));
}

// TODO: Bring back
// #[test]
// fn max_outgoing() {
//     use rand::Rng;
//
//     let outgoing_limit = rand::thread_rng().gen_range(1, 10);
//
//     let limits = ConnectionLimits::default().with_max_pending_outgoing(Some(outgoing_limit));
//     let mut network = new_test_swarm::<_, ()>(DummyConnectionHandler {
//         keep_alive: KeepAlive::Yes,
//     })
//     .connection_limits(limits)
//     .build();
//
//     let addr: Multiaddr = "/memory/1234".parse().unwrap();
//
//     let target = PeerId::random();
//     for _ in 0..outgoing_limit {
//         network
//             .dial(
//                 DialOpts::peer_id(target)
//                     .addresses(vec![addr.clone()])
//                     .build(),
//             )
//             .ok()
//             .expect("Unexpected connection limit.");
//     }
//
//     match network
//         .dial(
//             DialOpts::peer_id(target)
//                 .addresses(vec![addr.clone()])
//                 .build(),
//         )
//         .expect_err("Unexpected dialing success.")
//     {
//         DialError::ConnectionLimit(limit) => {
//             assert_eq!(limit.current, outgoing_limit);
//             assert_eq!(limit.limit, outgoing_limit);
//         }
//         e => panic!("Unexpected error: {:?}", e),
//     }
//
//     let info = network.network_info();
//     assert_eq!(info.num_peers(), 0);
//     assert_eq!(
//         info.connection_counters().num_pending_outgoing(),
//         outgoing_limit
//     );
// }
//
// #[test]
// fn max_established_incoming() {
//     use rand::Rng;
//
//     #[derive(Debug, Clone)]
//     struct Limit(u32);
//
//     impl Arbitrary for Limit {
//         fn arbitrary<G: Gen>(g: &mut G) -> Self {
//             Self(g.gen_range(1, 10))
//         }
//     }
//
//     fn limits(limit: u32) -> ConnectionLimits {
//         ConnectionLimits::default().with_max_established_incoming(Some(limit))
//     }
//
//     fn prop(limit: Limit) {
//         let limit = limit.0;
//
//         let mut network1 = new_test_swarm::<_, ()>(DummyConnectionHandler {
//             keep_alive: KeepAlive::Yes,
//         })
//         .connection_limits(limits(limit))
//         .build();
//         let mut network2 = new_test_swarm::<_, ()>(DummyConnectionHandler {
//             keep_alive: KeepAlive::Yes,
//         })
//         .connection_limits(limits(limit))
//         .build();
//
//         let _ = network1.listen_on(multiaddr![Memory(0u64)]).unwrap();
//         let listen_addr = async_std::task::block_on(poll_fn(|cx| {
//             match ready!(network1.poll_next_unpin(cx)).unwrap() {
//                 SwarmEvent::NewListenAddr { address, .. } => Poll::Ready(address),
//                 e => panic!("Unexpected network event: {:?}", e),
//             }
//         }));
//
//         // Spawn and block on the dialer.
//         async_std::task::block_on({
//             let mut n = 0;
//             let _ = network2.dial(listen_addr.clone()).unwrap();
//
//             let mut expected_closed = false;
//             let mut network_1_established = false;
//             let mut network_2_established = false;
//             let mut network_1_limit_reached = false;
//             let mut network_2_limit_reached = false;
//             poll_fn(move |cx| {
//                 loop {
//                     let mut network_1_pending = false;
//                     let mut network_2_pending = false;
//
//                     match network1.poll_next_unpin(cx) {
//                         Poll::Ready(Some(SwarmEvent::IncomingConnection { .. })) => {}
//                         Poll::Ready(Some(SwarmEvent::ConnectionEstablished { .. })) => {
//                             network_1_established = true;
//                         }
//                         Poll::Ready(Some(SwarmEvent::IncomingConnectionError {
//                             error: PendingConnectionError::ConnectionLimit(err),
//                             ..
//                         })) => {
//                             assert_eq!(err.limit, limit);
//                             assert_eq!(err.limit, err.current);
//                             let info = network1.network_info();
//                             let counters = info.connection_counters();
//                             assert_eq!(counters.num_established_incoming(), limit);
//                             assert_eq!(counters.num_established(), limit);
//                             network_1_limit_reached = true;
//                         }
//                         Poll::Pending => {
//                             network_1_pending = true;
//                         }
//                         e => panic!("Unexpected network event: {:?}", e),
//                     }
//
//                     match network2.poll_next_unpin(cx) {
//                         Poll::Ready(Some(SwarmEvent::ConnectionEstablished { .. })) => {
//                             network_2_established = true;
//                         }
//                         Poll::Ready(Some(SwarmEvent::ConnectionClosed { .. })) => {
//                             assert!(expected_closed);
//                             let info = network2.network_info();
//                             let counters = info.connection_counters();
//                             assert_eq!(counters.num_established_outgoing(), limit);
//                             assert_eq!(counters.num_established(), limit);
//                             network_2_limit_reached = true;
//                         }
//                         Poll::Pending => {
//                             network_2_pending = true;
//                         }
//                         e => panic!("Unexpected network event: {:?}", e),
//                     }
//
//                     if network_1_pending && network_2_pending {
//                         return Poll::Pending;
//                     }
//
//                     if network_1_established && network_2_established {
//                         network_1_established = false;
//                         network_2_established = false;
//
//                         if n <= limit {
//                             // Dial again until the limit is exceeded.
//                             n += 1;
//                             network2.dial(listen_addr.clone()).unwrap();
//
//                             if n == limit {
//                                 // The the next dialing attempt exceeds the limit, this
//                                 // is the connection we expected to get closed.
//                                 expected_closed = true;
//                             }
//                         } else {
//                             panic!("Expect networks not to establish connections beyond the limit.")
//                         }
//                     }
//
//                     if network_1_limit_reached && network_2_limit_reached {
//                         return Poll::Ready(());
//                     }
//                 }
//             })
//         });
//     }
//
//     quickcheck(prop as fn(_));
// }
