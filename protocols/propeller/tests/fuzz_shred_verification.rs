//! Fuzzing tests for shred verification robustness.
//!
//! This module contains deterministic pseudo-random fuzzing tests that corrupt
//! valid shreds in various ways to ensure the verification logic properly rejects
//! invalid data.

use libp2p_identity::{Keypair, PeerId};
use libp2p_propeller::{Behaviour, Config, MessageAuthenticity, ShredValidationError};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;

/// Apply random corruption to a byte array
fn corrupt_bytes(bytes: &mut [u8], seed: u64) -> (usize, u8, u8) {
    let mut rng = ChaChaRng::seed_from_u64(seed);
    let byte_pos = rng.random_range(0..bytes.len());
    let original_byte = bytes[byte_pos];
    bytes[byte_pos] ^= rng.random_range(1..=255u8);
    (byte_pos, original_byte, bytes[byte_pos])
}

/// Test configuration and setup data
struct FuzzTestSetup {
    leader_behaviour: Behaviour,
    follower_behaviour: Behaviour,
    follower_peer_id: PeerId,
    leader_peer_id: PeerId,
    valid_data: Vec<u8>,
}

/// Creates a complete test setup with leader and follower behaviours
fn create_fuzz_test_setup() -> FuzzTestSetup {
    let config = Config::builder()
        .fec_data_shreds(2)
        .fec_coding_shreds(2)
        .build();

    // Create leader keypair and behaviour
    let leader_keypair = Keypair::generate_ed25519();
    let leader_peer_id = PeerId::from(leader_keypair.public());
    let mut leader_behaviour = Behaviour::new(
        MessageAuthenticity::Signed(leader_keypair.clone()),
        config.clone(),
    );

    // Create follower behaviour
    let follower_keypair = Keypair::generate_ed25519();
    let follower_peer_id = PeerId::from(follower_keypair.public());
    let mut follower_behaviour = Behaviour::new(
        MessageAuthenticity::Signed(follower_keypair.clone()),
        config.clone(),
    );

    // Add peers to both behaviours
    leader_behaviour
        .set_peers(vec![(leader_peer_id, 2000), (follower_peer_id, 1000)])
        .unwrap();
    follower_behaviour
        .set_peers(vec![(leader_peer_id, 2000), (follower_peer_id, 1000)])
        .unwrap();

    // Create valid data for broadcasting
    let data_size: usize = config.fec_data_shreds() * 64; // 64 bytes per shred
    let valid_data = (0..data_size).map(|i| (i % 256) as u8).collect::<Vec<u8>>();

    FuzzTestSetup {
        leader_behaviour,
        follower_behaviour,
        follower_peer_id,
        leader_peer_id,
        valid_data,
    }
}

#[test]
fn test_deterministic_shred_corruption_fuzzing() {
    const FUZZ_ITERATIONS: usize = 10_000;

    let mut setup = create_fuzz_test_setup();

    // Create a valid shred and encode it
    let topic = 42;
    let shreds = setup
        .leader_behaviour
        .create_shreds_from_data(setup.valid_data, topic)
        .unwrap();
    for shred in shreds.iter() {
        setup
            .follower_behaviour
            .validate_shred(setup.leader_peer_id, shred)
            .unwrap();
    }

    let original_shreds_encoded = shreds
        .iter()
        .map(|shred| {
            let mut valid_shred_bytes = bytes::BytesMut::new();
            shred.encode(&mut valid_shred_bytes, 1 << 20);
            valid_shred_bytes.freeze().to_vec()
        })
        .collect::<Vec<_>>();
    let mut error_counter = vec![0; 7];

    for seed in 0..FUZZ_ITERATIONS {
        if seed % 1_000 == 0 {
            println!("Progress: {}/{}", seed, FUZZ_ITERATIONS);
        }

        let valid_shred_bytes =
            original_shreds_encoded[seed % original_shreds_encoded.len()].clone();

        // Create corrupted bytes
        let mut corrupted_bytes = valid_shred_bytes.clone();
        let (byte_pos, original_byte, new_byte) = corrupt_bytes(&mut corrupted_bytes, seed as u64);

        // Try to decode the corrupted shred
        let mut corrupted_bytes_mut = bytes::BytesMut::from(corrupted_bytes.as_slice());
        let Some(corrupted_shred) =
            libp2p_propeller::Shred::decode(&mut corrupted_bytes_mut, 1 << 20)
        else {
            continue;
        };

        // Validate the cLeaderReceivingShredorrupted shred - it should fail
        let sender = if seed % 50 == 0 {
            setup.follower_peer_id
        } else {
            setup.leader_peer_id
        };
        let behaviour = if seed % 51 == 0 {
            &mut setup.leader_behaviour
        } else {
            &mut setup.follower_behaviour
        };
        match behaviour.validate_shred(sender, &corrupted_shred) {
            Ok(_) => panic!(
                "CRITICAL: Corrupted shred passed validation! Seed: {}, Position: {}, Original: 0x{:02x}, New: 0x{:02x}",
                seed, byte_pos, original_byte, new_byte
            ),
            Err(error) => {
                error_counter[match error {
                    ShredValidationError::ReceivedPublishedShred => 2,
                    ShredValidationError::DuplicateShred => 3,
                    ShredValidationError::TreeError(_) => 4,
                    ShredValidationError::ParentVerificationFailed { .. } => 5,
                    ShredValidationError::SignatureVerificationFailed(_) => 6,
                }] += 1;
            }
        }
    }

    println!("Error counter: {:?}", error_counter);
}
