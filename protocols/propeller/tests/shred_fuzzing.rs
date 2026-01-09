//! Shared fuzzing utilities for shred testing.
//!
//! This module contains common utilities for generating random shreds,
//! corrupting data, and performing encode/decode roundtrip tests.

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

/// Generate a random valid shred for testing
pub(crate) fn generate_random_shred(seed: u64) -> libp2p_propeller::Shred {
    let mut rng = ChaChaRng::seed_from_u64(seed);
    libp2p_propeller::Shred::random(&mut rng, 1 << 12)
}

#[test]
fn test_encode_decode_roundtrip_fuzzing() {
    const ITERATIONS: usize = 10_000;
    for seed in 0..ITERATIONS {
        if seed % 1_000 == 0 {
            println!("Progress: {}/{}", seed, ITERATIONS,);
        }
        let original_shred = generate_random_shred(seed as u64);
        let mut encoded_bytes = bytes::BytesMut::new();
        original_shred.encode(&mut encoded_bytes, 1 << 20);
        let mut decode_buffer = encoded_bytes.clone();
        let Some(decoded_shred) = libp2p_propeller::Shred::decode(&mut decode_buffer, 1 << 20)
        else {
            panic!(
                "Encode then decode failed! Seed: {}, Original: {:?}",
                seed, original_shred
            );
        };
        assert_eq!(decoded_shred, original_shred);
    }
}

#[test]
fn test_encode_decode_roundtrip_with_corruption_fuzzing() {
    const ITERATIONS: usize = 10_000;
    for seed in 0..ITERATIONS {
        if seed % 1_000 == 0 {
            println!("Progress: {}/{}", seed, ITERATIONS,);
        }
        let original_shred = generate_random_shred(seed as u64);
        let mut encoded_bytes = bytes::BytesMut::new();
        original_shred.encode(&mut encoded_bytes, 1 << 20);
        let mut our_bytes = encoded_bytes.to_vec();
        let (byte_pos, original_byte, new_byte) = corrupt_bytes(&mut our_bytes, seed as u64);
        let mut decode_buffer = bytes::BytesMut::from(our_bytes.as_slice());
        let Some(decoded_shred) = libp2p_propeller::Shred::decode(&mut decode_buffer, 1 << 20)
        else {
            continue;
        };
        assert_ne!(
            decoded_shred, original_shred,
            "Seed: {}, Byte pos: {}, Original byte: {:?}, New byte: {:?}.\nThis \
            test assumes that every byte in the encoding matters, if this is not \
            the case, why the useless bytes?",
            seed, byte_pos, original_byte, new_byte
        );
    }
}

#[test]
fn test_deterministic_hash_fuzzing() {
    const ITERATIONS: usize = 10_000;
    for seed in 0..ITERATIONS {
        if seed % 1_000 == 0 {
            println!("Progress: {}/{}", seed, ITERATIONS,);
        }
        let original_shred = generate_random_shred(seed as u64);
        let hash = original_shred.hash();

        let mut encoded_bytes = bytes::BytesMut::new();
        original_shred.encode(&mut encoded_bytes, 1 << 20);

        for _ in 0..10 {
            assert_eq!(hash, original_shred.hash());

            let mut our_bytes = encoded_bytes.to_vec();
            let (byte_pos, original_byte, new_byte) = corrupt_bytes(&mut our_bytes, seed as u64);
            let mut decode_buffer = bytes::BytesMut::from(our_bytes.as_slice());
            let Some(decoded_shred) = libp2p_propeller::Shred::decode(&mut decode_buffer, 1 << 20)
            else {
                continue;
            };
            assert_ne!(
                decoded_shred.hash(),
                hash,
                "Seed: {}, Byte pos: {}, Original byte: {:?}, New byte: {:?}",
                seed,
                byte_pos,
                original_byte,
                new_byte
            );
        }
    }
}
