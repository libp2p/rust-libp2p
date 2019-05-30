/* ---------------------------------------------------------------- *
 * CONSTANTS                                                        *
 * ---------------------------------------------------------------- */

#![allow(non_snake_case, non_upper_case_globals)]
use hacl_star::{chacha20poly1305, curve25519};

pub const DHLEN: usize = curve25519::SECRET_LENGTH;
pub(crate) const HASHLEN: usize = 32;
pub(crate) const BLOCKLEN: usize = 64;
pub(crate) const EMPTY_HASH: [u8; DHLEN] = [0_u8; HASHLEN];
pub(crate) const EMPTY_KEY: [u8; DHLEN] = [0_u8; DHLEN];
pub const MAC_LENGTH: usize = chacha20poly1305::MAC_LENGTH;
pub(crate) const MAX_MESSAGE: usize = 0xFFFF;
pub(crate) const MAX_NONCE: u64 = u64::max_value();
pub(crate) const NONCE_LENGTH: usize = chacha20poly1305::NONCE_LENGTH;
pub(crate) const ZEROLEN: [u8; 0] = [0_u8; 0];
