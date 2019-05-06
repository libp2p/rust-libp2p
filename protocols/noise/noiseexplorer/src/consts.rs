/* ---------------------------------------------------------------- *
 * CONSTANTS                                                        *
 * ---------------------------------------------------------------- */

#![allow(non_snake_case, non_upper_case_globals)]
use hacl_star::chacha20poly1305;
use hacl_star::curve25519;

pub const DHLEN: usize = curve25519::SECRET_LENGTH;
pub const HASHLEN: usize = 32;
pub const BLOCKLEN: usize = 64;
pub const EMPTY_HASH: [u8; DHLEN] = [0u8; HASHLEN];
pub const EMPTY_KEY: [u8; DHLEN] = [0u8; DHLEN];
pub const MAC_LENGTH: usize = chacha20poly1305::MAC_LENGTH;
pub const MAX_MESSAGE: usize = 65535;
pub const MAX_NONCE: u64 = u64::max_value();
pub const NONCE_LENGTH: usize = chacha20poly1305::NONCE_LENGTH;
pub const ZEROLEN: [u8; 0] = [0u8; 0];
