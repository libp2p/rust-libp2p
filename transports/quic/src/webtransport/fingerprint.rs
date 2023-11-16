use sha2::Digest as _;
use std::fmt;

const SHA256: &str = "sha-256";
const MULTIHASH_SHA256_CODE: u64 = 0x12;

type Multihash = libp2p_core::multihash::Multihash<64>;

/// A certificate fingerprint that is assumed to be created using the SHA256 hash algorithm.
#[derive(Eq, PartialEq, Copy, Clone)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub(crate) const FF: Fingerprint = Fingerprint([0xFF; 32]);

    #[cfg(test)]
    pub fn raw(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Creates a fingerprint from a raw certificate.
    pub fn from_certificate(bytes: &[u8]) -> Self {
        Fingerprint(sha2::Sha256::digest(bytes).into())
    }

    /// Converts [`Multihash`](multihash::Multihash) to [`Fingerprint`].
    pub fn try_from_multihash(hash: Multihash) -> Option<Self> {
        if hash.code() != MULTIHASH_SHA256_CODE {
            // Only support SHA256 for now.
            return None;
        }

        let bytes = hash.digest().try_into().ok()?;

        Some(Self(bytes))
    }

    /// Converts this fingerprint to [`Multihash`](multihash::Multihash).
    pub fn to_multihash(self) -> Multihash {
        Multihash::wrap(MULTIHASH_SHA256_CODE, &self.0).expect("fingerprint's len to be 32 bytes")
    }

    /// Formats this fingerprint as uppercase hex, separated by colons (`:`).
    ///
    /// This is the format described in <https://www.rfc-editor.org/rfc/rfc4572#section-5>.
    pub fn to_sdp_format(self) -> String {
        self.0.map(|byte| format!("{byte:02X}")).join(":")
    }

    /// Returns the algorithm used (e.g. "sha-256").
    /// See <https://datatracker.ietf.org/doc/html/rfc8122#section-5>
    pub fn algorithm(&self) -> String {
        SHA256.to_owned()
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdp_format() {
        let fp = Fingerprint::raw(hex_literal::hex!(
            "7DE3D83F81A680592A471E6B6ABB0747ABD35385A8093FDFE112C1EEBB6CC6AC"
        ));

        let sdp_format = fp.to_sdp_format();

        assert_eq!(sdp_format, "7D:E3:D8:3F:81:A6:80:59:2A:47:1E:6B:6A:BB:07:47:AB:D3:53:85:A8:09:3F:DF:E1:12:C1:EE:BB:6C:C6:AC")
    }
}