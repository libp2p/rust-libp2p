use futures::pin_mut;
use futures::prelude::*;
use libp2p_core::{muxing::StreamMuxerBox, transport, upgrade, Transport};
use libp2p_identify as identify;
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use libp2p_mplex::MplexConfig;
use libp2p_noise as noise;
use libp2p_swarm::{Swarm, SwarmBuilder, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use libp2p_tcp as tcp;
use std::iter;

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

#[async_std::test]
async fn identify_push() {
    let _ = env_logger::try_init();

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(identify::Config::new("a".to_string(), identity.public()))
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string()),
        )
    });

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    swarm2
        .behaviour_mut()
        .push(iter::once(*swarm1.local_peer_id()));

    match libp2p_swarm_test::drive(&mut swarm1, &mut swarm2).await {
        ([identify::Event::Received { info, .. }], [identify::Event::Pushed { .. }]) => {
            assert_eq!(info.public_key.to_peer_id(), *swarm2.local_peer_id());
            assert_eq!(info.protocol_version, "a");
            assert_eq!(info.agent_version, "b");
            assert!(!info.protocols.is_empty());
            assert!(info.listen_addrs.is_empty());
        }
        other => panic!("Unexpected events: {other:?}"),
    }
}

#[async_std::test]
async fn discover_peer_after_disconnect() {
    let _ = env_logger::try_init();

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(identify::Config::new("a".to_string(), identity.public()))
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string()),
        )
    });

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    let swarm1_peer_id = *swarm1.local_peer_id();
    async_std::task::spawn(swarm1.loop_on_next());

    // Wait until we identified.
    swarm2
        .wait(|event| {
            matches!(
                event,
                SwarmEvent::Behaviour(identify::Event::Received { .. })
            )
            .then_some(())
        })
        .await;

    swarm2.disconnect_peer_id(swarm1_peer_id).unwrap();

    // Wait for connection to close.
    swarm2
        .wait(|event| matches!(event, SwarmEvent::ConnectionClosed { .. }).then_some(()))
        .await;

    // We should still be able to dial now!
    swarm2.dial(swarm1_peer_id).unwrap();

    let connected_peer = swarm2
        .wait(|event| match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. } => Some(peer_id),
            _ => None,
        })
        .await;

    assert_eq!(connected_peer, swarm1_peer_id);
}
