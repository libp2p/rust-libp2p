use libp2p_core::multiaddr::Protocol;
use libp2p_identify as identify;
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use std::iter;
use tracing_subscriber::EnvFilter;

#[async_std::test]
async fn periodic_identify() {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

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

    use identify::Event::Received;
    use identify::Event::Sent;

    match libp2p_swarm_test::drive(&mut swarm1, &mut swarm2).await {
        (
            [Received { info: s1_info, .. }, Sent { .. }],
            [Received { info: s2_info, .. }, Sent { .. }],
        )
        | (
            [Sent { .. }, Received { info: s1_info, .. }],
            [Sent { .. }, Received { info: s2_info, .. }],
        )
        | (
            [Received { info: s1_info, .. }, Sent { .. }],
            [Sent { .. }, Received { info: s2_info, .. }],
        )
        | (
            [Sent { .. }, Received { info: s1_info, .. }],
            [Received { info: s2_info, .. }, Sent { .. }],
        ) => {
            assert_eq!(s1_info.public_key.to_peer_id(), swarm2_peer_id);
            assert_eq!(s1_info.protocol_version, "c");
            assert_eq!(s1_info.agent_version, "d");
            assert!(!s1_info.protocols.is_empty());
            assert_eq!(
                s1_info.observed_addr,
                swarm1_memory_listen
                    .clone()
                    .with(Protocol::P2p(swarm1_peer_id))
            );
            assert!(s1_info.listen_addrs.contains(&swarm2_tcp_listen_addr));
            assert!(s1_info.listen_addrs.contains(&swarm2_memory_listen));

            assert_eq!(s2_info.public_key.to_peer_id(), swarm1_peer_id);
            assert_eq!(s2_info.protocol_version, "a");
            assert_eq!(s2_info.agent_version, "b");
            assert!(!s2_info.protocols.is_empty());

            // Cannot assert observed address of dialer because memory transport uses ephemeral, outgoing ports.
            // assert_eq!(
            //     s2_info.observed_addr,
            //     swarm2_memory_listen.with(Protocol::P2p(swarm2_peer_id.into()))
            // );
            assert!(s2_info.listen_addrs.contains(&swarm1_tcp_listen_addr));
            assert!(s2_info.listen_addrs.contains(&swarm1_memory_listen));
        }
        other => panic!("Unexpected events: {other:?}"),
    }
}

#[async_std::test]
async fn identify_push() {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

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

    // First, let the periodic identify do its thing.
    let ([e1, e2], [e3, e4]) = libp2p_swarm_test::drive(&mut swarm1, &mut swarm2).await;

    {
        use identify::Event::{Received, Sent};

        // These can be received in any order, hence assert them here.
        assert!(matches!(e1, Received { .. } | Sent { .. }));
        assert!(matches!(e2, Received { .. } | Sent { .. }));
        assert!(matches!(e3, Received { .. } | Sent { .. }));
        assert!(matches!(e4, Received { .. } | Sent { .. }));
    }

    // Second, actively push.
    swarm2
        .behaviour_mut()
        .push(iter::once(*swarm1.local_peer_id()));

    let swarm1_received_info = match libp2p_swarm_test::drive(&mut swarm1, &mut swarm2).await {
        ([identify::Event::Received { info, .. }], [identify::Event::Pushed { .. }]) => info,
        other => panic!("Unexpected events: {other:?}"),
    };

    assert_eq!(
        swarm1_received_info.public_key.to_peer_id(),
        *swarm2.local_peer_id()
    );
    assert_eq!(swarm1_received_info.protocol_version, "a");
    assert_eq!(swarm1_received_info.agent_version, "b");
    assert!(!swarm1_received_info.protocols.is_empty());
    assert!(swarm1_received_info.listen_addrs.is_empty());
}

#[async_std::test]
async fn discover_peer_after_disconnect() {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

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
