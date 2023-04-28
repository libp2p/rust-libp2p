use futures::prelude::*;
use libp2p_core::multiaddr::Protocol;
use libp2p_identify as identify;
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use std::iter;

#[async_std::test]
async fn periodic_identify() {
    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string()),
        )
    });
    let swarm1_peer_id = *swarm1.local_peer_id();

    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("c".to_string(), identity.public())
                .with_agent_version("d".to_string()),
        )
    });
    let swarm2_peer_id = *swarm2.local_peer_id();

    let (swarm1_memory_listen, swarm1_tcp_listen_addr) = swarm1.listen().await;
    let (swarm2_memory_listen, swarm2_tcp_listen_addr) = swarm2.listen().await;
    swarm2.connect(&mut swarm1).await;

    // nb. Either swarm may receive the `Identified` event first, upon which
    // it will permit the connection to be closed, as defined by
    // `Handler::connection_keep_alive`. Hence the test succeeds if
    // either `Identified` event arrives correctly.
    loop {
        match future::select(swarm1.next_behaviour_event(), swarm2.next_behaviour_event())
            .await
            .factor_second()
            .0
        {
            future::Either::Left(identify::Event::Received { info, .. }) => {
                assert_eq!(info.public_key.to_peer_id(), swarm2_peer_id);
                assert_eq!(info.protocol_version, "c");
                assert_eq!(info.agent_version, "d");
                assert!(!info.protocols.is_empty());
                assert_eq!(
                    info.observed_addr,
                    swarm1_memory_listen.with(Protocol::P2p(swarm1_peer_id.into()))
                );
                assert!(info.listen_addrs.contains(&swarm2_tcp_listen_addr));
                assert!(info.listen_addrs.contains(&swarm2_memory_listen));
                return;
            }
            future::Either::Right(identify::Event::Received { info, .. }) => {
                assert_eq!(info.public_key.to_peer_id(), swarm1_peer_id);
                assert_eq!(info.protocol_version, "a");
                assert_eq!(info.agent_version, "b");
                assert!(!info.protocols.is_empty());
                assert_eq!(
                    info.observed_addr,
                    swarm2_memory_listen.with(Protocol::P2p(swarm2_peer_id.into()))
                );
                assert!(info.listen_addrs.contains(&swarm1_tcp_listen_addr));
                assert!(info.listen_addrs.contains(&swarm1_memory_listen));
                return;
            }
            _ => {}
        }
    }
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
