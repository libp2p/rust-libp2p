use errors::Error;

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
    /// Get the corresponding hash code
    pub fn code(&self) -> u8 {
        use Hash::*;

        match *self {
            SHA1 => 0x11,
            SHA2256 => 0x12,
            SHA2512 => 0x13,
            SHA3224 => 0x17,
            SHA3256 => 0x16,
            SHA3384 => 0x15,
            SHA3512 => 0x14,
            Keccak224 => 0x1A,
            Keccak256 => 0x1B,
            Keccak384 => 0x1C,
            Keccak512 => 0x1D,
            Blake2b => 0x40,
            Blake2s => 0x41,
        }
    }

    /// Get the hash length in bytes
    pub fn size(&self) -> u8 {
        use Hash::*;

        match *self {
            SHA1 => 20,
            SHA2256 => 32,
            SHA2512 => 64,
            SHA3224 => 28,
            SHA3256 => 32,
            SHA3384 => 48,
            SHA3512 => 64,
            Keccak224 => 28,
            Keccak256 => 32,
            Keccak384 => 48,
            Keccak512 => 64,
            Blake2b => 64,
            Blake2s => 32,

        }
    }

    /// Get the human readable name
    pub fn name(&self) -> &str {
        use Hash::*;

        match *self {
            SHA1 => "SHA1",
            SHA2256 => "SHA2-256",
            SHA2512 => "SHA2-512",
            SHA3512 => "SHA3-512",
            SHA3384 => "SHA3-384",
            SHA3256 => "SHA3-256",
            SHA3224 => "SHA3-224",
            Keccak224 => "Keccak-224",
            Keccak256 => "Keccak-256",
            Keccak384 => "Keccak-384",
            Keccak512 => "Keccak-512",
            Blake2b => "Blake-2b",
            Blake2s => "Blake-2s",
        }
    }

    pub fn from_code(code: u8) -> Result<Hash, Error> {
        use Hash::*;

        Ok(match code {
            0x11 => SHA1,
            0x12 => SHA2256,
            0x13 => SHA2512,
            0x14 => SHA3512,
            0x15 => SHA3384,
            0x16 => SHA3256,
            0x17 => SHA3224,
            0x1A => Keccak224,
            0x1B => Keccak256,
            0x1C => Keccak384,
            0x1D => Keccak512,
            0x40 => Blake2b,
            0x41 => Blake2s,
            _ => return Err(Error::UnknownCode),
        })
    }
}
