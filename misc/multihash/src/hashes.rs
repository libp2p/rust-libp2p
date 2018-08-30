
/// List of types currently supported in the multihash spec.
///
/// Not all hash types are supported by this library.
#[derive(PartialEq, Eq, Clone, Debug, Copy, Hash)]
pub enum Hash {
    /// SHA-1 (20-byte hash size)
    SHA1,
    /// SHA-256 (32-byte hash size)
    SHA2256,
    /// SHA-512 (64-byte hash size)
    SHA2512,
    /// SHA3-512 (64-byte hash size)
    SHA3512,
    /// SHA3-384 (48-byte hash size)
    SHA3384,
    /// SHA3-256 (32-byte hash size)
    SHA3256,
    /// SHA3-224 (28-byte hash size)
    SHA3224,
    /// Keccak-224 (28-byte hash size)
    Keccak224,
    /// Keccak-256 (32-byte hash size)
    Keccak256,
    /// Keccak-384 (48-byte hash size)
    Keccak384,
    /// Keccak-512 (64-byte hash size)
    Keccak512,
    /// Encoding unsupported
    Blake2b,
    /// Encoding unsupported
    Blake2s,
}

impl Hash {
    /// Get the corresponding hash code.
    pub fn code(&self) -> u8 {
        match *self {
            Hash::SHA1 => 0x11,
            Hash::SHA2256 => 0x12,
            Hash::SHA2512 => 0x13,
            Hash::SHA3224 => 0x17,
            Hash::SHA3256 => 0x16,
            Hash::SHA3384 => 0x15,
            Hash::SHA3512 => 0x14,
            Hash::Keccak224 => 0x1A,
            Hash::Keccak256 => 0x1B,
            Hash::Keccak384 => 0x1C,
            Hash::Keccak512 => 0x1D,
            Hash::Blake2b => 0x40,
            Hash::Blake2s => 0x41,
        }
    }

    /// Get the hash length in bytes.
    pub fn size(&self) -> u8 {
        match *self {
            Hash::SHA1 => 20,
            Hash::SHA2256 => 32,
            Hash::SHA2512 => 64,
            Hash::SHA3224 => 28,
            Hash::SHA3256 => 32,
            Hash::SHA3384 => 48,
            Hash::SHA3512 => 64,
            Hash::Keccak224 => 28,
            Hash::Keccak256 => 32,
            Hash::Keccak384 => 48,
            Hash::Keccak512 => 64,
            Hash::Blake2b => 64,
            Hash::Blake2s => 32,
        }
    }

    /// Returns the algorithm corresponding to a code, or `None` if no algorith is matching.
    pub fn from_code(code: u8) -> Option<Hash> {
        Some(match code {
            0x11 => Hash::SHA1,
            0x12 => Hash::SHA2256,
            0x13 => Hash::SHA2512,
            0x14 => Hash::SHA3512,
            0x15 => Hash::SHA3384,
            0x16 => Hash::SHA3256,
            0x17 => Hash::SHA3224,
            0x1A => Hash::Keccak224,
            0x1B => Hash::Keccak256,
            0x1C => Hash::Keccak384,
            0x1D => Hash::Keccak512,
            0x40 => Hash::Blake2b,
            0x41 => Hash::Blake2s,
            _ => return None,
        })
    }
}
