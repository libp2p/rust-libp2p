//! Signature creation and validation for the Propeller protocol.
//!
//! This module handles cryptographic operations for signing and verifying shreds,
//! following similar patterns to gossipsub for consistency with the libp2p ecosystem.

use libp2p_identity::{PeerId, PublicKey};
use tracing::{debug, warn};

use crate::{
    behaviour::MessageAuthenticity,
    message::Shred,
    types::{ShredPublishError, ShredSignatureVerificationError},
};

/// Signing prefix used for Propeller protocol signatures.
/// This follows the same pattern as gossipsub's "libp2p-pubsub:" prefix.
pub(crate) const SIGNING_PREFIX: &[u8] = b"libp2p-propeller:";

/// Sign a shred using the provided keypair.
///
/// The signature is created over the bytes "libp2p-propeller:<shred-encoded-without-signature>"
/// where shred-encoded-without-signature is the result of `shred.encode_without_signature()`.
/// This follows the same pattern as gossipsub which signs the entire message content.
pub(crate) fn sign_shred(
    shred: &Shred,
    message_authenticity: &MessageAuthenticity,
) -> Result<Vec<u8>, ShredPublishError> {
    match message_authenticity {
        MessageAuthenticity::Signed(keypair) => {
            // Encode the shred without signature (following gossipsub pattern)
            let shred_bytes = shred.encode_without_signature();

            // Create signature bytes with prefix (following gossipsub pattern)
            let mut signature_bytes = SIGNING_PREFIX.to_vec();
            signature_bytes.extend_from_slice(&shred_bytes);

            // Sign the prefixed encoded shred
            match keypair.sign(&signature_bytes) {
                Ok(signature) => {
                    debug!(shred_id=?shred.id, "Successfully signed shred");
                    Ok(signature)
                }
                Err(e) => Err(ShredPublishError::SigningFailed(e.to_string())),
            }
        }
        MessageAuthenticity::Author(_) => {
            // No signing capability, return empty signature
            debug!(shred_id=?shred.id, "No signing capability, using empty signature");
            Ok(Vec::new())
        }
    }
}

/// Verify a shred signature using the provided public key.
///
/// This validates that the signature was created over
/// "libp2p-propeller:<shred-encoded-without-signature>" using the private key corresponding to the
/// provided public key. This follows the same pattern as gossipsub which verifies against the
/// entire message content.
pub(crate) fn verify_shred_signature(
    shred: &Shred,
    public_key: &PublicKey,
) -> Result<(), ShredSignatureVerificationError> {
    if shred.signature.is_empty() {
        return Err(ShredSignatureVerificationError::EmptySignature);
    }
    let shred_bytes = shred.encode_without_signature();
    let mut signature_bytes = SIGNING_PREFIX.to_vec();
    signature_bytes.extend_from_slice(&shred_bytes);
    let signature_valid = public_key.verify(&signature_bytes, &shred.signature);
    if signature_valid {
        Ok(())
    } else {
        Err(ShredSignatureVerificationError::VerificationFailed)
    }
}

/// Attempt to extract a public key from a PeerId.
///
/// This only works for small keys (≤42 bytes) like Ed25519 that are embedded
/// directly in the PeerId using an identity multihash.
///
/// Following gossipsub's approach, we validate that the extracted key
/// actually matches the PeerId to prevent spoofing attacks.
pub(crate) fn try_extract_public_key_from_peer_id(peer_id: &PeerId) -> Option<PublicKey> {
    // Get the multihash from the PeerId
    let multihash = peer_id.as_ref();

    // Check if this is an identity multihash (code 0x00)
    if multihash.code() == 0x00 {
        // For identity multihash, the digest contains the encoded public key
        let encoded_key = multihash.digest();

        // Try to decode the public key from protobuf
        match PublicKey::try_decode_protobuf(encoded_key) {
            Ok(public_key) => {
                // SECURITY: Verify that the extracted key actually matches this PeerId
                // This prevents attacks where someone provides a malicious PeerId
                let derived_peer_id = PeerId::from(&public_key);
                if derived_peer_id == *peer_id {
                    debug!(peer=%peer_id, "Successfully extracted and validated public key from PeerId");
                    Some(public_key)
                } else {
                    warn!(
                        peer=%peer_id,
                        derived_peer=%derived_peer_id,
                        "Security violation: extracted public key does not match PeerId - possible spoofing attempt"
                    );
                    None
                }
            }
            Err(e) => {
                debug!(peer=%peer_id, error=?e, "Failed to decode public key from PeerId");
                None
            }
        }
    } else {
        // This is a hashed PeerId (SHA-256), cannot extract the original key
        debug!(peer=%peer_id, multihash_code=%multihash.code(), "PeerId uses hashed multihash, cannot extract public key");
        None
    }
}

/// Validate that a public key matches the given PeerId.
///
/// This is a security check to prevent configuration errors and spoofing attacks.
pub(crate) fn validate_public_key_matches_peer_id(
    public_key: &PublicKey,
    peer_id: &PeerId,
) -> bool {
    let derived_peer_id = PeerId::from(public_key);
    derived_peer_id == *peer_id
}

#[cfg(test)]
mod tests {
    use libp2p_identity::Keypair;

    use super::*;
    use crate::message::{Shred, ShredId};

    #[test]
    fn test_sign_and_verify_shred() {
        let keypair = Keypair::generate_ed25519();
        let message_authenticity = MessageAuthenticity::Signed(keypair.clone());

        let shred = Shred {
            id: ShredId {
                message_id: 1,
                index: 0,
                publisher: PeerId::random(),
            },
            shard: vec![1, 2, 3, 4],
            signature: Vec::new(),
        };

        // Sign the shred
        let signature = sign_shred(&shred, &message_authenticity).unwrap();
        assert!(!signature.is_empty());

        // Create shred with signature
        let signed_shred = Shred { signature, ..shred };

        // Verify the signature
        let result = verify_shred_signature(&signed_shred, &keypair.public());
        assert!(result.is_ok());
    }

    #[test]
    fn test_key_extraction_and_validation() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = PeerId::from(keypair.public());

        // Test extraction
        let extracted_key = try_extract_public_key_from_peer_id(&peer_id);
        assert!(extracted_key.is_some());

        // Test validation
        let is_valid = validate_public_key_matches_peer_id(&keypair.public(), &peer_id);
        assert!(is_valid);

        // Test with mismatched key
        let other_keypair = Keypair::generate_ed25519();
        let is_invalid = validate_public_key_matches_peer_id(&other_keypair.public(), &peer_id);
        assert!(!is_invalid);
    }

    #[test]
    fn test_random_peer_id_extraction() {
        let random_peer = PeerId::random();
        let extracted_key = try_extract_public_key_from_peer_id(&random_peer);
        assert!(extracted_key.is_none()); // Should fail for random PeerIDs
    }
}
