use std::{
    collections::HashSet,
    iter,
    time::{Duration, Instant},
};

use futures::StreamExt;
use libp2p_identify as identify;
use libp2p_identity::Keypair;
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use tracing_subscriber::EnvFilter;

#[async_std::test]
async fn periodic_identify() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

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

    let (swarm1_memory_listen, swarm1_tcp_listen_addr) =
        swarm1.listen().with_memory_addr_external().await;
    let (swarm2_memory_listen, swarm2_tcp_listen_addr) = swarm2.listen().await;
    swarm2.connect(&mut swarm1).await;

    use identify::Event::{Received, Sent};

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
            assert_eq!(s1_info.observed_addr, swarm1_memory_listen);
            assert!(s1_info.listen_addrs.contains(&swarm2_tcp_listen_addr));
            assert!(s1_info.listen_addrs.contains(&swarm2_memory_listen));

            assert_eq!(s2_info.public_key.to_peer_id(), swarm1_peer_id);
            assert_eq!(s2_info.protocol_version, "a");
            assert_eq!(s2_info.agent_version, "b");
            assert!(!s2_info.protocols.is_empty());

            // Cannot assert observed address of dialer because memory transport uses ephemeral,
            // outgoing ports.
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
async fn only_emits_address_candidate_once_per_connection() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string())
                .with_interval(Duration::from_secs(1)),
        )
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("c".to_string(), identity.public())
                .with_agent_version("d".to_string()),
        )
    });

    swarm2.listen().with_memory_addr_external().await;
    swarm1.connect(&mut swarm2).await;

    async_std::task::spawn(swarm2.loop_on_next());

    let swarm_events = futures::stream::poll_fn(|cx| swarm1.poll_next_unpin(cx))
        .take(8)
        .collect::<Vec<_>>()
        .await;

    let infos = swarm_events
        .iter()
        .filter_map(|e| match e {
            SwarmEvent::Behaviour(identify::Event::Received { info, .. }) => Some(info.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        infos.len() > 1,
        "should exchange identify payload more than once"
    );

    let varying_observed_addresses = infos
        .iter()
        .map(|i| i.observed_addr.clone())
        .collect::<HashSet<_>>();
    assert_eq!(
        varying_observed_addresses.len(),
        1,
        "Observed address should not vary on persistent connection"
    );

    let external_address_candidates = swarm_events
        .iter()
        .filter_map(|e| match e {
            SwarmEvent::NewExternalAddrCandidate { address } => Some(address.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        external_address_candidates.len(),
        1,
        "To only have one external address candidate"
    );
    assert_eq!(
        &external_address_candidates[0],
        varying_observed_addresses.iter().next().unwrap()
    );
}

#[async_std::test]
async fn emits_unique_listen_addresses() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string())
                .with_interval(Duration::from_secs(1))
                .with_cache_size(10),
        )
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("c".to_string(), identity.public())
                .with_agent_version("d".to_string()),
        )
    });

    let (swarm2_mem_listen_addr, swarm2_tcp_listen_addr) =
        swarm2.listen().with_memory_addr_external().await;
    let swarm2_peer_id = *swarm2.local_peer_id();
    swarm1.connect(&mut swarm2).await;

    async_std::task::spawn(swarm2.loop_on_next());

    let swarm_events = futures::stream::poll_fn(|cx| swarm1.poll_next_unpin(cx))
        .take(8)
        .collect::<Vec<_>>()
        .await;

    let infos = swarm_events
        .iter()
        .filter_map(|e| match e {
            SwarmEvent::Behaviour(identify::Event::Received { info, .. }) => Some(info.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        infos.len() > 1,
        "should exchange identify payload more than once"
    );

    let listen_addrs = infos
        .iter()
        .map(|i| i.listen_addrs.clone())
        .collect::<Vec<_>>();

    for addrs in listen_addrs {
        assert_eq!(addrs.len(), 2);
        assert!(addrs.contains(&swarm2_mem_listen_addr));
        assert!(addrs.contains(&swarm2_tcp_listen_addr));
    }

    let reported_addrs = swarm_events
        .iter()
        .filter_map(|e| match e {
            SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                Some((*peer_id, address.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(reported_addrs.len(), 2, "To have two addresses of remote");
    assert!(reported_addrs.contains(&(swarm2_peer_id, swarm2_mem_listen_addr)));
    assert!(reported_addrs.contains(&(swarm2_peer_id, swarm2_tcp_listen_addr)));
}

#[async_std::test]
async fn hides_listen_addresses() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string())
                .with_interval(Duration::from_secs(1))
                .with_cache_size(10),
        )
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("c".to_string(), identity.public())
                .with_agent_version("d".to_string())
                .with_hide_listen_addrs(true),
        )
    });

    let (_swarm2_mem_listen_addr, swarm2_tcp_listen_addr) =
        swarm2.listen().with_tcp_addr_external().await;
    let swarm2_peer_id = *swarm2.local_peer_id();
    swarm1.connect(&mut swarm2).await;

    async_std::task::spawn(swarm2.loop_on_next());

    let swarm_events = futures::stream::poll_fn(|cx| swarm1.poll_next_unpin(cx))
        .take(8)
        .collect::<Vec<_>>()
        .await;

    let infos = swarm_events
        .iter()
        .filter_map(|e| match e {
            SwarmEvent::Behaviour(identify::Event::Received { info, .. }) => Some(info.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        infos.len() > 1,
        "should exchange identify payload more than once"
    );

    let listen_addrs = infos
        .iter()
        .map(|i| i.listen_addrs.clone())
        .collect::<Vec<_>>();

    for addrs in listen_addrs {
        assert_eq!(addrs.len(), 1);
        assert!(addrs.contains(&swarm2_tcp_listen_addr));
    }

    let reported_addrs = swarm_events
        .iter()
        .filter_map(|e| match e {
            SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                Some((*peer_id, address.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(reported_addrs.len(), 1, "To have one TCP address of remote");
    assert!(reported_addrs.contains(&(swarm2_peer_id, swarm2_tcp_listen_addr)));
}

#[async_std::test]
async fn identify_push() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(identify::Config::new("a".to_string(), identity.public()))
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string()),
        )
    });

    swarm1.listen().with_memory_addr_external().await;
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
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(identify::Config::new("a".to_string(), identity.public()))
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string()),
        )
    });

    swarm1.listen().with_memory_addr_external().await;
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

#[async_std::test]
async fn configured_interval_starts_after_first_identify() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let identify_interval = Duration::from_secs(5);

    let mut swarm1 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_interval(identify_interval),
        )
    });
    let mut swarm2 = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_agent_version("b".to_string()),
        )
    });

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    async_std::task::spawn(swarm2.loop_on_next());

    let start = Instant::now();

    // Wait until we identified.
    swarm1
        .wait(|event| {
            matches!(event, SwarmEvent::Behaviour(identify::Event::Sent { .. })).then_some(())
        })
        .await;

    let time_to_first_identify = Instant::now().duration_since(start);

    assert!(time_to_first_identify < identify_interval)
}

#[async_std::test]
async fn reject_mismatched_public_key() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut honest_swarm = Swarm::new_ephemeral(|identity| {
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), identity.public())
                .with_interval(Duration::from_secs(1)),
        )
    });
    let mut spoofing_swarm = Swarm::new_ephemeral(|_unused_identity| {
        let arbitrary_public_key = Keypair::generate_ed25519().public();
        identify::Behaviour::new(
            identify::Config::new("a".to_string(), arbitrary_public_key)
                .with_interval(Duration::from_secs(1)),
        )
    });

    honest_swarm.listen().with_memory_addr_external().await;
    spoofing_swarm.connect(&mut honest_swarm).await;

    spoofing_swarm
        .wait(|event| {
            matches!(event, SwarmEvent::Behaviour(identify::Event::Sent { .. })).then_some(())
        })
        .await;

    let honest_swarm_events = futures::stream::poll_fn(|cx| honest_swarm.poll_next_unpin(cx))
        .take(4)
        .collect::<Vec<_>>()
        .await;

    assert!(
        !honest_swarm_events
            .iter()
            .any(|e| matches!(e, SwarmEvent::Behaviour(identify::Event::Received { .. }))),
        "should emit no received events as received public key won't match remote peer",
    );
}
