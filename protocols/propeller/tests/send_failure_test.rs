//! Unit tests demonstrating ShredSendFailed events for disconnected peers.

use std::time::Duration;

use futures::FutureExt;
use libp2p_identity::{Keypair, PeerId};
use libp2p_propeller::{Behaviour, Config, Event, MessageAuthenticity};
use libp2p_swarm::Swarm;
use libp2p_swarm_test::SwarmExt as _;

// Helper functions
fn create_test_config() -> Config {
    Config::builder()
        .fanout(2)
        .fec_data_shreds(2)
        .fec_coding_shreds(2)
        .build()
}

fn create_keypair_and_peer() -> (Keypair, PeerId) {
    let keypair = Keypair::generate_ed25519();
    let peer_id = PeerId::from(keypair.public());
    (keypair, peer_id)
}

async fn create_swarm(peer_id: PeerId, config: Config) -> Swarm<Behaviour> {
    let mut swarm = Swarm::new_ephemeral_tokio(|_key| {
        Behaviour::new(MessageAuthenticity::Author(peer_id), config)
    });
    swarm.listen().with_memory_addr_external().await;
    swarm
}

async fn poll_for_events(
    swarm: &mut Swarm<Behaviour>,
    rounds: usize,
) -> (Vec<(Option<PeerId>, String)>, Vec<Event>) {
    let mut send_failed_events = Vec::new();
    let mut other_events = Vec::new();

    for _ in 0..rounds {
        while let Some(event) = swarm.next_behaviour_event().now_or_never() {
            match event {
                Event::ShredSendFailed { sent_to, error, .. } => {
                    send_failed_events.push((sent_to, error.to_string()));
                }
                other => other_events.push(other),
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;

        if !send_failed_events.is_empty() {
            break;
        }
    }

    (send_failed_events, other_events)
}

#[tokio::test]
async fn test_disconnected_peer_send_failure() {
    let config = create_test_config();
    let (_, leader_peer_id) = create_keypair_and_peer();
    let (_, connected_peer_id) = create_keypair_and_peer();
    let (_, disconnected_peer_id) = create_keypair_and_peer();

    let mut leader_swarm = create_swarm(leader_peer_id, config.clone()).await;
    let mut connected_swarm = create_swarm(connected_peer_id, config).await;

    connected_swarm.connect(&mut leader_swarm).await;

    let peers = vec![
        (leader_peer_id, 3000),
        (connected_peer_id, 2000),
        (disconnected_peer_id, 1000),
    ];

    leader_swarm
        .behaviour_mut()
        .set_peers(peers.clone())
        .unwrap();
    connected_swarm.behaviour_mut().set_peers(peers).unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    let test_data = vec![42u8; 64];
    leader_swarm
        .behaviour_mut()
        .broadcast(test_data, 123)
        .unwrap();

    let (send_failed_events, _) = poll_for_events(&mut leader_swarm, 50).await;

    // Test passes regardless - demonstrates graceful handling of disconnected peers
    if send_failed_events.is_empty() {
        panic!("No ShredSendFailed events - protocol should inform about disconnected peers");
    } else {
        // With 2 data shreds + 2 coding shreds, we expect 4 send failures per disconnected peer
        // The tree topology determines how many peers get the message, so we check for at least 4
        // failures
        assert!(
            send_failed_events.len() >= 4,
            "Expected at least 4 failures, got {}",
            send_failed_events.len()
        );
        for (_, error) in &send_failed_events {
            assert!(
                error.contains("failed")
                    || error.contains("error")
                    || error.contains("closed")
                    || error.contains("Not connected"),
                "Error should indicate connection failure: {}",
                error
            );
        }
    }
}

#[tokio::test]
async fn test_connection_drop_send_failure() {
    let config = create_test_config();
    let (_, leader_peer_id) = create_keypair_and_peer();
    let (_, follower_peer_id) = create_keypair_and_peer();

    let mut leader_swarm = create_swarm(leader_peer_id, config.clone()).await;
    let mut follower_swarm = create_swarm(follower_peer_id, config).await;

    follower_swarm.connect(&mut leader_swarm).await;

    let peers = vec![(leader_peer_id, 2000), (follower_peer_id, 1000)];
    leader_swarm
        .behaviour_mut()
        .set_peers(peers.clone())
        .unwrap();
    follower_swarm.behaviour_mut().set_peers(peers).unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    drop(follower_swarm);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let test_data = vec![42u8; 64];
    leader_swarm
        .behaviour_mut()
        .broadcast(test_data, 789)
        .unwrap();

    let (send_failed_events, _) = poll_for_events(&mut leader_swarm, 50).await;

    if send_failed_events.is_empty() {
        panic!("No send failures captured - timing or topology dependent");
    } else {
        // With 2 data shreds + 2 coding shreds, we expect 4 send failures when connection drops
        assert!(
            send_failed_events.len() >= 4,
            "Expected at least 4 failures, got {}",
            send_failed_events.len()
        );
        for (_, error) in &send_failed_events {
            assert!(
                error.contains("failed")
                    || error.contains("error")
                    || error.contains("closed")
                    || error.contains("connection")
                    || error.contains("Not connected"),
                "Error should indicate connection failure: {}",
                error
            );
        }
    }
}

#[tokio::test]
async fn test_handler_send_error_propagation() {
    let config = Config::builder()
        .fanout(1)
        .fec_data_shreds(1)
        .fec_coding_shreds(1)
        .build();

    let (_, peer_id) = create_keypair_and_peer();
    let (_, target_peer) = create_keypair_and_peer();

    let mut swarm = create_swarm(peer_id, config).await;

    swarm
        .behaviour_mut()
        .set_peers(vec![(peer_id, 2000), (target_peer, 1000)])
        .unwrap();

    let test_data = vec![42u8; 64];
    swarm.behaviour_mut().broadcast(test_data, 456).unwrap();

    let (send_failed_events, _) = poll_for_events(&mut swarm, 30).await;

    if send_failed_events.is_empty() {
        panic!("No send failures - connection handling may be async");
    } else {
        // With 1 data shred + 1 coding shred, we expect 2 send failures
        assert_eq!(send_failed_events.len(), 2);
        for (_, error) in &send_failed_events {
            assert!(
                error.contains("failed")
                    || error.contains("error")
                    || error.contains("Dial")
                    || error.contains("connection")
                    || error.contains("Not connected"),
                "Expected connection-related error: {}",
                error
            );
        }
    }
}
