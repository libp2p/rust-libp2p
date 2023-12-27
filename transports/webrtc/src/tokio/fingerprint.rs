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

use webrtc::dtls_transport::dtls_fingerprint::RTCDtlsFingerprint;

const SHA256: &str = "sha-256";

type Multihash = multihash::Multihash<64>;

/// A certificate fingerprint that is assumed to be created using the SHA256 hash algorithm.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct Fingerprint(libp2p_webrtc_utils::Fingerprint);

impl Fingerprint {
    #[cfg(test)]
    pub fn raw(bytes: [u8; 32]) -> Self {
        Self(libp2p_webrtc_utils::Fingerprint::raw(bytes))
    }

    /// Creates a fingerprint from a raw certificate.
    pub fn from_certificate(bytes: &[u8]) -> Self {
        Fingerprint(libp2p_webrtc_utils::Fingerprint::from_certificate(bytes))
    }

    /// Converts [`RTCDtlsFingerprint`] to [`Fingerprint`].
    pub fn try_from_rtc_dtls(fp: &RTCDtlsFingerprint) -> Option<Self> {
        if fp.algorithm != SHA256 {
            return None;
        }

        let mut buf = [0; 32];
        hex::decode_to_slice(fp.value.replace(':', ""), &mut buf).ok()?;

        Some(Self(libp2p_webrtc_utils::Fingerprint::raw(buf)))
    }

    /// Converts [`Multihash`](multihash::Multihash) to [`Fingerprint`].
    pub fn try_from_multihash(hash: Multihash) -> Option<Self> {
        Some(Self(libp2p_webrtc_utils::Fingerprint::try_from_multihash(
            hash,
        )?))
    }

    /// Converts this fingerprint to [`Multihash`](multihash::Multihash).
    pub fn to_multihash(self) -> Multihash {
        self.0.to_multihash()
    }

    /// Formats this fingerprint as uppercase hex, separated by colons (`:`).
    ///
    /// This is the format described in <https://www.rfc-editor.org/rfc/rfc4572#section-5>.
    pub fn to_sdp_format(self) -> String {
        self.0.to_sdp_format()
    }

    /// Returns the algorithm used (e.g. "sha-256").
    /// See <https://datatracker.ietf.org/doc/html/rfc8122#section-5>
    pub fn algorithm(&self) -> String {
        self.0.algorithm()
    }

    pub(crate) fn into_inner(self) -> libp2p_webrtc_utils::Fingerprint {
        self.0
    }
}
