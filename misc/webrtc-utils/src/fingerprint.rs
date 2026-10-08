// Copyright 2023 Doug Anderson.
// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use std::fmt;

use libp2p_core::multihash;
use sha2::Digest as _;

pub const SHA256: &str = "sha-256";
const MULTIHASH_SHA256_CODE: u64 = 0x12;

type Multihash = multihash::Multihash<64>;

/// A certificate fingerprint that is assumed to be created using the SHA256 hash algorithm.
#[derive(Eq, PartialEq, Copy, Clone)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub const FF: Fingerprint = Fingerprint([0xFF; 32]);

    pub const fn raw(digest: [u8; 32]) -> Self {
        Fingerprint(digest)
    }

    /// Creates a new [Fingerprint] from a raw certificate by hashing the given bytes with SHA256.
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

    /// Parses a fingerprint from the format described in
    /// <https://www.rfc-editor.org/rfc/rfc4572#section-5>: 32 hex-encoded bytes, optionally
    /// separated by colons (`:`). Case-insensitive.
    ///
    /// This is the inverse of [`Fingerprint::to_sdp_format`]. Returns `None` if the input is
    /// not a well-formed SHA-256 fingerprint.
    pub fn from_sdp_format(sdp: &str) -> Option<Self> {
        let mut digest = [0u8; 32];
        hex::decode_to_slice(sdp.replace(':', ""), &mut digest).ok()?;

        Some(Self(digest))
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

    const SDP_FORMAT: &str = "7D:E3:D8:3F:81:A6:80:59:2A:47:1E:6B:6A:BB:07:47:AB:D3:53:85:A8:09:3F:DF:E1:12:C1:EE:BB:6C:C6:AC";
    const REGULAR_FORMAT: [u8; 32] =
        hex_literal::hex!("7DE3D83F81A680592A471E6B6ABB0747ABD35385A8093FDFE112C1EEBB6CC6AC");

    #[test]
    fn sdp_format() {
        let fp = Fingerprint::raw(REGULAR_FORMAT);

        let formatted = fp.to_sdp_format();

        assert_eq!(formatted, SDP_FORMAT)
    }

    #[test]
    fn from_sdp() {
        let fp = Fingerprint::from_sdp_format(SDP_FORMAT).unwrap();

        assert_eq!(fp, Fingerprint::raw(REGULAR_FORMAT));
    }

    #[test]
    fn sdp_format_round_trips() {
        let fp = Fingerprint::raw(REGULAR_FORMAT);

        assert_eq!(Fingerprint::from_sdp_format(&fp.to_sdp_format()), Some(fp));
    }

    /// Colons are separators, and hex is case-insensitive; neither carries meaning.
    #[test]
    fn from_sdp_accepts_lowercase_and_missing_colons() {
        let expected = Fingerprint::raw(REGULAR_FORMAT);

        for accepted in [
            SDP_FORMAT.to_lowercase(),
            SDP_FORMAT.replace(':', ""),
            SDP_FORMAT.replace(':', "").to_lowercase(),
        ] {
            assert_eq!(Fingerprint::from_sdp_format(&accepted), Some(expected));
        }
    }

    /// A truncated or malformed fingerprint must not yield a partial one: a wrong fingerprint
    /// silently fails the DTLS handshake much later, which is far harder to diagnose.
    #[test]
    fn from_sdp_rejects_malformed_input() {
        for rejected in [
            "",
            "not hex at all",
            // One byte short.
            &SDP_FORMAT[..SDP_FORMAT.len() - 3],
            // One byte too many.
            &format!("{SDP_FORMAT}:AB"),
            // Odd number of nibbles.
            &SDP_FORMAT[..SDP_FORMAT.len() - 1],
        ] {
            assert_eq!(
                Fingerprint::from_sdp_format(rejected),
                None,
                "should have rejected {rejected:?}"
            );
        }
    }
}
