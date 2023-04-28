use futures::pin_mut;
use futures::prelude::*;
use libp2p_core::{muxing::StreamMuxerBox, transport, upgrade, Transport};
use libp2p_identify as identify;
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use libp2p_mplex::MplexConfig;
use libp2p_noise as noise;
use libp2p_swarm::{Swarm, SwarmBuilder, SwarmEvent};
use libp2p_tcp as tcp;
use std::time::Duration;

fn transport() -> (
    identity::PublicKey,
    transport::Boxed<(PeerId, StreamMuxerBox)>,
) {
    let id_keys = identity::Keypair::generate_ed25519();
    let pubkey = id_keys.public();
    let transport = tcp::async_io::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(&id_keys).unwrap())
        .multiplex(MplexConfig::new())
        .boxed();
    (pubkey, transport)
}

#[test]
fn periodic_identify() {
    let (mut swarm1, pubkey1) = {
        let (pubkey, transport) = transport();
        let protocol = identify::Behaviour::new(
            identify::Config::new("a".to_string(), pubkey.clone())
                .with_agent_version("b".to_string()),
        );
        let swarm =
            SwarmBuilder::with_async_std_executor(transport, protocol, pubkey.to_peer_id()).build();
        (swarm, pubkey)
    };

    let (mut swarm2, pubkey2) = {
        let (pubkey, transport) = transport();
        let protocol = identify::Behaviour::new(
            identify::Config::new("c".to_string(), pubkey.clone())
                .with_agent_version("d".to_string()),
        );
        let swarm =
            SwarmBuilder::with_async_std_executor(transport, protocol, pubkey.to_peer_id()).build();
        (swarm, pubkey)
    };

    swarm1
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();

    let listen_addr = async_std::task::block_on(async {
        loop {
            let swarm1_fut = swarm1.select_next_some();
            pin_mut!(swarm1_fut);
            if let SwarmEvent::NewListenAddr { address, .. } = swarm1_fut.await {
                return address;
            }
        }
    });
    swarm2.dial(listen_addr).unwrap();

    // nb. Either swarm may receive the `Identified` event first, upon which
    // it will permit the connection to be closed, as defined by
    // `Handler::connection_keep_alive`. Hence the test succeeds if
    // either `Identified` event arrives correctly.
    async_std::task::block_on(async move {
        loop {
            let swarm1_fut = swarm1.select_next_some();
            pin_mut!(swarm1_fut);
            let swarm2_fut = swarm2.select_next_some();
            pin_mut!(swarm2_fut);

            match future::select(swarm1_fut, swarm2_fut)
                .await
                .factor_second()
                .0
            {
                future::Either::Left(SwarmEvent::Behaviour(identify::Event::Received {
                    info,
                    ..
                })) => {
                    assert_eq!(info.public_key, pubkey2);
                    assert_eq!(info.protocol_version, "c");
                    assert_eq!(info.agent_version, "d");
                    assert!(!info.protocols.is_empty());
                    assert!(info.listen_addrs.is_empty());
                    return;
                }
                future::Either::Right(SwarmEvent::Behaviour(identify::Event::Received {
                    info,
                    ..
                })) => {
                    assert_eq!(info.public_key, pubkey1);
                    assert_eq!(info.protocol_version, "a");
                    assert_eq!(info.agent_version, "b");
                    assert!(!info.protocols.is_empty());
                    assert_eq!(info.listen_addrs.len(), 1);
                    return;
                }
                _ => {}
            }
        }
    })
}

#[test]
fn identify_push() {
    let _ = env_logger::try_init();

    let (mut swarm1, pubkey1) = {
        let (pubkey, transport) = transport();
        let protocol =
            identify::Behaviour::new(identify::Config::new("a".to_string(), pubkey.clone()));
        let swarm =
            SwarmBuilder::with_async_std_executor(transport, protocol, pubkey.to_peer_id()).build();
        (swarm, pubkey)
    };

    let (mut swarm2, pubkey2) = {
        let (pubkey, transport) = transport();
        let protocol = identify::Behaviour::new(
            identify::Config::new("a".to_string(), pubkey.clone())
                .with_agent_version("b".to_string()),
        );
        let swarm =
            SwarmBuilder::with_async_std_executor(transport, protocol, pubkey.to_peer_id()).build();
        (swarm, pubkey)
    };

    Swarm::listen_on(&mut swarm1, "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let listen_addr = async_std::task::block_on(async {
        loop {
            let swarm1_fut = swarm1.select_next_some();
            pin_mut!(swarm1_fut);
            if let SwarmEvent::NewListenAddr { address, .. } = swarm1_fut.await {
                return address;
            }
        }
    });

    swarm2.dial(listen_addr).unwrap();

    async_std::task::block_on(async move {
        loop {
            let swarm1_fut = swarm1.select_next_some();
            let swarm2_fut = swarm2.select_next_some();

            {
                pin_mut!(swarm1_fut);
                pin_mut!(swarm2_fut);
                match future::select(swarm1_fut, swarm2_fut)
                    .await
                    .factor_second()
                    .0
                {
                    future::Either::Left(SwarmEvent::Behaviour(identify::Event::Received {
                        info,
                        ..
                    })) => {
                        assert_eq!(info.public_key, pubkey2);
                        assert_eq!(info.protocol_version, "a");
                        assert_eq!(info.agent_version, "b");
                        assert!(!info.protocols.is_empty());
                        assert!(info.listen_addrs.is_empty());
                        return;
                    }
                    future::Either::Right(SwarmEvent::ConnectionEstablished { .. }) => {
                        // Once a connection is established, we can initiate an
                        // active push below.
                    }
                    _ => continue,
                }
            }

            swarm2
                .behaviour_mut()
                .push(std::iter::once(pubkey1.to_peer_id()));
        }
    })
}

#[test]
fn discover_peer_after_disconnect() {
    let _ = env_logger::try_init();

    let mut swarm1 = {
        let (pubkey, transport) = transport();
        let protocol = identify::Behaviour::new(
            identify::Config::new("a".to_string(), pubkey.clone())
                // `swarm1` will set `KeepAlive::No` once it identified `swarm2` and thus
                // closes the connection. At this point in time `swarm2` might not yet have
                // identified `swarm1`. To give `swarm2` enough time, set an initial delay on
                // `swarm1`.
                .with_initial_delay(Duration::from_secs(10)),
        );

        SwarmBuilder::with_async_std_executor(transport, protocol, pubkey.to_peer_id()).build()
    };

    let mut swarm2 = {
        let (pubkey, transport) = transport();
        let protocol = identify::Behaviour::new(
            identify::Config::new("a".to_string(), pubkey.clone())
                .with_agent_version("b".to_string()),
        );

        SwarmBuilder::with_async_std_executor(transport, protocol, pubkey.to_peer_id()).build()
    };

    let swarm1_peer_id = *swarm1.local_peer_id();

    let listener = swarm1
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();

    let listen_addr = async_std::task::block_on(async {
        loop {
            match swarm1.select_next_some().await {
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } if listener_id == listener => return address,
                _ => {}
            }
        }
    });

    async_std::task::spawn(async move {
        loop {
            swarm1.next().await;
        }
    });

    swarm2.dial(listen_addr).unwrap();

    // Wait until we identified.
    async_std::task::block_on(async {
        loop {
            if let SwarmEvent::Behaviour(identify::Event::Received { .. }) =
                swarm2.select_next_some().await
            {
                break;
            }
        }
    });

    swarm2.disconnect_peer_id(swarm1_peer_id).unwrap();

    // Wait for connection to close.
    async_std::task::block_on(async {
        loop {
            if let SwarmEvent::ConnectionClosed { peer_id, .. } = swarm2.select_next_some().await {
                break peer_id;
            }
        }
    });

    // We should still be able to dial now!
    swarm2.dial(swarm1_peer_id).unwrap();

    let connected_peer = async_std::task::block_on(async {
        loop {
            if let SwarmEvent::ConnectionEstablished { peer_id, .. } =
                swarm2.select_next_some().await
            {
                break peer_id;
            }
        }
    });

    assert_eq!(connected_peer, swarm1_peer_id);
}
