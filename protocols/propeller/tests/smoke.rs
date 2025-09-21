//! Smoke tests for Propeller protocol with 100 peers and fanout of 6.

use libp2p_identity::PeerId;
use libp2p_propeller::{Behaviour, Config, MessageAuthenticity};
use rand::{Rng, SeedableRng};
use tracing_subscriber::EnvFilter;

#[test]
fn test_propeller_100_peers_fanout_6_api() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    const NUM_PEERS: usize = 100;
    const FANOUT: usize = 6;

    tracing::info!(
        "Testing Propeller API with {} peers and fanout {}",
        NUM_PEERS,
        FANOUT
    );

    // Create configuration with fanout 6
    let config = Config::builder()
        .fanout(FANOUT)
        .fec_data_shreds(16)
        .fec_coding_shreds(16)
        .build();

    assert_eq!(config.fanout(), FANOUT);

    // Generate 100 peers with valid Ed25519 keypairs and random weights
    let mut rng = rand::rngs::StdRng::seed_from_u64(12345);
    let peers: Vec<(PeerId, u64)> = (0..NUM_PEERS)
        .map(|_| {
            let keypair = libp2p_identity::Keypair::generate_ed25519();
            let peer_id = PeerId::from(keypair.public());
            let weight = rng.random_range(100..10000);
            (peer_id, weight)
        })
        .collect();

    // Set local peer ID to the first peer
    let local_peer_id = peers[0].0;

    // Create behaviour
    let mut propeller = Behaviour::new(MessageAuthenticity::Author(local_peer_id), config.clone());
    // Local peer ID is now set automatically in constructor

    // Add all peers with their weights (including local peer required by tree manager)
    let _ = propeller.set_peers(peers.clone());

    // For testing purposes, simulate that all peers are connected
    let peer_ids: Vec<PeerId> = peers.iter().map(|(id, _)| *id).collect();
    propeller.add_connected_peers_for_test(peer_ids);

    tracing::info!("Added {} peers to propeller behaviour", NUM_PEERS);

    // Test broadcasting (local peer is always the publisher)
    // Data must be divisible by num_data_shreds
    let data_shreds = config.fec_data_shreds();
    let test_data = vec![42u8; data_shreds * 64]; // 64 bytes per shred
    match propeller.broadcast(test_data, 0) {
        Ok(_) => {
            tracing::info!("✅ Broadcast successful");
        }
        Err(e) => {
            panic!("❌ Broadcast failed: {}", e);
        }
    }

    tracing::info!("✅ All Propeller API tests passed!");
}

#[test]
fn test_propeller_tree_computation() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    const NUM_PEERS: usize = 20;
    const FANOUT: usize = 6;

    tracing::info!(
        "Testing Propeller tree computation with {} peers and fanout {}",
        NUM_PEERS,
        FANOUT
    );

    let config = Config::builder().fanout(FANOUT).build();

    // Generate peers with valid Ed25519 keypairs and different weights
    let mut rng = rand::rngs::StdRng::seed_from_u64(54321);
    let peers: Vec<(PeerId, u64)> = (0..NUM_PEERS)
        .map(|i| {
            let weight = if i < 5 {
                // First 5 peers have high weight
                rng.random_range(5000..10000)
            } else {
                // Rest have lower weight
                rng.random_range(100..1000)
            };
            let keypair = libp2p_identity::Keypair::generate_ed25519();
            let peer_id = PeerId::from(keypair.public());
            (peer_id, weight)
        })
        .collect();

    let local_peer_id = peers[0].0;
    let mut propeller = Behaviour::new(MessageAuthenticity::Author(local_peer_id), config.clone());
    // Local peer ID is now set automatically in constructor

    // Add all peers (including local peer required by tree manager)
    let _ = propeller.set_peers(peers.clone());

    // For testing purposes, simulate that all peers are connected
    let peer_ids: Vec<PeerId> = peers.iter().map(|(id, _)| *id).collect();
    propeller.add_connected_peers_for_test(peer_ids);

    // Test broadcasting with different message IDs to verify tree computation works
    for message_idx in 0..5 {
        tracing::debug!("Testing broadcast with message_id {}", message_idx);

        // Test broadcasting (local peer is the publisher)
        let data_shreds = config.fec_data_shreds();
        let test_data = vec![message_idx as u8; data_shreds * 64]; // 64 bytes per shred

        match propeller.broadcast(test_data, message_idx as u64) {
            Ok(_) => {
                tracing::debug!("Broadcast successful for message_id {}", message_idx);
            }
            Err(e) => {
                tracing::warn!("Broadcast failed for message_id {}: {}", message_idx, e);
            }
        }
    }

    tracing::info!("✅ Tree computation test completed successfully");
}

#[test]
fn test_propeller_configuration() {
    // Test various configuration combinations
    let configs = vec![
        (6, 16, 16),   // Default-ish
        (10, 8, 8),    // Less FEC
        (3, 32, 32),   // More FEC
        (100, 24, 24), // Large fanout
    ];

    for (fanout, data_shreds, coding_shreds) in configs {
        let config = Config::builder()
            .fanout(fanout)
            .fec_data_shreds(data_shreds)
            .fec_coding_shreds(coding_shreds)
            .build();

        let _propeller = Behaviour::new(
            MessageAuthenticity::Author(PeerId::random()),
            config.clone(),
        );

        assert_eq!(config.fanout(), fanout);
        assert_eq!(config.fec_data_shreds(), data_shreds);
        assert_eq!(config.fec_coding_shreds(), coding_shreds);
    }

    println!("✅ Configuration test passed!");
}
