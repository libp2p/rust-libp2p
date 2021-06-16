use libp2p_core::SignedEnvelope;
use sha2::{Digest, Sha256};
use std::fmt;

const DOMAIN_TAG: &'static str = "libp2p-rendezvous-pow";

/// Run the Proof of Work algorithm for a registration at a rendezvous node.
///
/// Returns the calculated hash together with the computed nonce.
/// In the odd case that the nonce space is exhausted without meeting the difficulty requirement, the function fails with [`ExhaustedNonceSpace`].
pub fn run(
    challenge: &[u8],
    namespace: &str,
    envelope: SignedEnvelope,
    target_difficulty: Difficulty,
) -> Result<([u8; 32], i64), ExhaustedNonceSpace> {
    let envelope = envelope.into_protobuf_encoding();

    for i in i64::MIN..i64::MAX {
        let hash = make_hash(challenge, namespace, envelope.as_slice(), i);

        if difficulty_of(&hash) < target_difficulty {
            continue;
        }

        return Ok((hash, i));
    }

    Err(ExhaustedNonceSpace)
}

/// Verifies a PoW hash against the given parameters and requested difficulty.
pub fn verify(
    challenge: &[u8],
    namespace: &str,
    envelope: SignedEnvelope,
    requested_difficulty: Difficulty,
    hash: [u8; 32],
    nonce: i64,
) -> Result<(), VerifyError> {
    if difficulty_of(&hash) < requested_difficulty {
        return Err(VerifyError::DifficultyTooLow);
    }

    let actual_hash = make_hash(
        challenge,
        namespace,
        envelope.into_protobuf_encoding().as_slice(),
        nonce,
    );

    if actual_hash != hash {
        return Err(VerifyError::NotTheSameHash);
    }

    Ok(())
}

/// The difficulty of a PoW hash.
///
/// Measured as the number of leading `0`s at the front of the hash in big-endian representation.
#[derive(Debug, PartialEq, Clone, Copy, PartialOrd)]
pub struct Difficulty(u32);

impl fmt::Display for Difficulty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Difficulty {
    pub const ZERO: Difficulty = Difficulty(0);
    pub const MAX: Difficulty = Difficulty(32);

    pub fn from_u32(value: u32) -> Option<Self> {
        if value > 32 {
            return None;
        }

        Some(Difficulty(value))
    }

    pub fn to_u32(&self) -> u32 {
        self.0
    }
}

#[derive(Debug)]
pub struct ExhaustedNonceSpace;

impl fmt::Display for ExhaustedNonceSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Exhausted nonce space while computing proof of work")
    }
}

impl std::error::Error for ExhaustedNonceSpace {}

#[derive(Debug, PartialEq)]
pub enum VerifyError {
    DifficultyTooLow,
    NotTheSameHash,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::DifficultyTooLow => {
                write!(f, "The difficulty of the provided hash is too low")
            }
            VerifyError::NotTheSameHash => write!(f, "The hash differs unexpectedly"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Returns the difficulty of the provided hash.
pub fn difficulty_of(hash: &[u8; 32]) -> Difficulty {
    for i in 0..32 {
        if hash[i as usize] != 0 {
            return Difficulty(i);
        }
    }

    return Difficulty(32);
}

fn make_hash(challenge: &[u8], namespace: &str, envelope: &[u8], nonce: i64) -> [u8; 32] {
    let mut digest = Sha256::new();

    digest.update(DOMAIN_TAG.as_bytes());
    digest.update(challenge);
    digest.update(namespace.as_bytes());
    digest.update(envelope);
    digest.update(&nonce.to_le_bytes());

    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::{identity, PeerRecord};

    #[test]
    fn pow_verifies() {
        let challenge = b"foobar";
        let namespace = "baz";
        let envelope = PeerRecord::new(identity::Keypair::generate_ed25519(), vec![])
            .unwrap()
            .into_signed_envelope();

        let (hash, nonce) = run(challenge, namespace, envelope.clone(), Difficulty(1)).unwrap();
        let result = verify(challenge, namespace, envelope, Difficulty(1), hash, nonce);

        assert_eq!(result, Ok(()))
    }

    #[test]
    fn meets_difficulty_test_cases() {
        let test_cases = &[
            (
                [
                    0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                    1, 1, 1, 1, 1, 1,
                ],
                Difficulty(5),
            ),
            (
                [
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                    1, 1, 1, 1, 1, 1,
                ],
                Difficulty(0),
            ),
            (
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0,
                ],
                Difficulty(32),
            ),
        ];

        for (hash, expected_result) in test_cases {
            let actual = difficulty_of(&hash);

            assert_eq!(actual, *expected_result);
        }
    }
}
