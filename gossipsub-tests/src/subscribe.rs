use crate::node::{barrier_and_drive_swarm, ensure_all_peers_subscribed, gossipsub, transport};
use crate::Context;
use libp2p::core::ConnectedPoint;
use libp2p::futures::StreamExt;
use libp2p::gossipsub::IdentTopic;
use libp2p::swarm::SwarmEvent;
use libp2p::SwarmBuilder;
use tracing::{debug, info, warn};

/// Tests the gossipsub subscription mechanism across multiple nodes.
/// 
/// Verifies that nodes can successfully connect to each other, subscribe to a topic,
/// and form a proper mesh network for gossipsub message distribution.
pub(crate) async fn subscribe(mut context: Context) {
    let mut swarm_events = vec![];
    let is_primary_node = context.node_index == 0;
    info!("is_primary_node: {is_primary_node}");
    let keypair = libp2p::identity::Keypair::generate_ed25519();

    // ////////////////////////////////////////////////////////////////////////
    // Start libp2p
    // ////////////////////////////////////////////////////////////////////////
    info!("Starting libp2p swarm initialization");
    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_other_transport(|key| transport(key))
        .expect("infallible")
        .with_behaviour(|key| gossipsub(key.clone()).expect("Valid configuration"))
        .expect("infallible")
        .build();

    swarm
        .listen_on(context.local_addr.clone())
        .expect("Swarm starts listening");

    swarm_events.extend(
        barrier_and_drive_swarm(
            "Start libp2p",
            &mut swarm,
            &mut context.redis,
            context.node_count,
        )
        .await,
    );
    info!("libp2p swarm initialization completed");

    // ////////////////////////////////////////////////////////////////////////
    // Connect to nodes
    // ////////////////////////////////////////////////////////////////////////
    if is_primary_node {
        let nodes = context
            .participants
            .iter()
            .filter(|&p| p != &context.local_addr)
            .collect::<Vec<_>>();
        nodes.iter().for_each(|&p| {
            swarm.dial(p.clone()).unwrap();
        });

        // Ensure that a connection has been established.
        let mut connected_nodes = 0;
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::ConnectionEstablished {
                    peer_id: _,
                    endpoint,
                    ..
                } => match endpoint {
                    ConnectedPoint::Dialer { address, .. } => {
                        info!("Successfully connected to peer at {}", address);
                        connected_nodes += 1;
                    }
                    event => debug!("[Swarm event] {event:?}"),
                },
                event => debug!("[Swarm event] {event:?}"),
            }
            if connected_nodes == nodes.len() {
                info!(
                    "Primary node successfully connected to all {} peer nodes",
                    nodes.len()
                );
                break;
            }
        }
    }

    swarm_events.extend(
        barrier_and_drive_swarm(
            "Connect to nodes",
            &mut swarm,
            &mut context.redis,
            context.node_count,
        )
        .await,
    );

    let all_peers: Vec<_> = swarm.behaviour().all_peers().collect();
    let all_peers_count = all_peers.len();
    info!("All connected peers (total: {}):", all_peers_count);
    for (index, peer) in all_peers.iter().enumerate() {
        info!("  [{index}]: {peer:?}");
    }

    // ////////////////////////////////////////////////////////////////////////
    // Subscribe to a topic
    // ////////////////////////////////////////////////////////////////////////
    info!("Starting topic subscription process");
    let topic = IdentTopic::new("test_subscribe");
    if !swarm.behaviour_mut().subscribe(&topic).unwrap() {
        warn!(
            "Already subscribed to topic '{}', subscription request ignored",
            topic
        );
    }

    // Wait for all connected peers to be subscribed.
    ensure_all_peers_subscribed(&mut swarm, &swarm_events, all_peers_count).await;

    barrier_and_drive_swarm(
        "Subscribe to a topic",
        &mut swarm,
        &mut context.redis,
        context.node_count,
    )
    .await;
    info!("Topic subscription process completed");

    // ////////////////////////////////////////////////////////////////////////
    // Assertions
    // ////////////////////////////////////////////////////////////////////////
    let mesh_peers = swarm.behaviour().mesh_peers(&topic.hash());
    assert!(mesh_peers.count() > 0, "should have mesh peers.");

    barrier_and_drive_swarm(
        "End of test",
        &mut swarm,
        &mut context.redis,
        context.node_count,
    )
    .await;

    info!("Subscribe test completed successfully");
}
