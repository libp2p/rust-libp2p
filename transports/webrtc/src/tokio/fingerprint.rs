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

use sha2::Digest as _;
use std::fmt;
use webrtc::dtls_transport::dtls_fingerprint::RTCDtlsFingerprint;

const SHA256: &str = "sha-256";
const MULTIHASH_SHA256_CODE: u64 = 0x12;

type Multihash = multihash::Multihash<64>;

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

    /// Converts [`RTCDtlsFingerprint`] to [`Fingerprint`].
    pub fn try_from_rtc_dtls(fp: &RTCDtlsFingerprint) -> Option<Self> {
        if fp.algorithm != SHA256 {
            return None;
        }

        let mut buf = [0; 32];
        hex::decode_to_slice(fp.value.replace(':', ""), &mut buf).ok()?;

        Some(Self(buf))
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
