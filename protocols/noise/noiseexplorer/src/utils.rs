/* ---------------------------------------------------------------- *
 * UTILITY FUNCTIONS                                                *
 * ---------------------------------------------------------------- */
use crate::consts::{EMPTY_HASH, HASHLEN, NONCE_LENGTH};

pub(crate) fn from_slice_hashlen(bytes: &[u8]) -> [u8; HASHLEN] {
	let mut array = EMPTY_HASH;
	let bytes = &bytes[..array.len()];
	array.copy_from_slice(bytes);
	array
}
pub(crate) fn prep_nonce(n: u64) -> [u8;12] {
	let mut nonce: [u8; NONCE_LENGTH] = [0_u8; NONCE_LENGTH];
	nonce[4..].copy_from_slice(&n.to_le_bytes());
	nonce
}
