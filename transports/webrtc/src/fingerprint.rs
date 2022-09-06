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

use multihash::Multihash;
use webrtc::dtls_transport::dtls_fingerprint::RTCDtlsFingerprint;

const SHA256: &str = "sha-256";

pub(crate) struct Fingerprint(RTCDtlsFingerprint);

impl Fingerprint {
    /// Creates new `Fingerprint` w/ "sha-256" hash function.
    pub fn new_sha256(value: String) -> Self {
        Self(RTCDtlsFingerprint {
            algorithm: SHA256.to_owned(),
            value,
        })
    }

    /// Transforms this fingerprint into a ufrag.
    pub fn to_ufrag(&self) -> String {
        self.0.value.replace(':', "").to_lowercase()
    }

    /// Returns the upper-hex value, each byte separated by ":".
    /// E.g. "7D:E3:D8:3F:81:A6:80:59:2A:47:1E:6B:6A:BB:07:47:AB:D3:53:85:A8:09:3F:DF:E1:12:C1:EE:BB:6C:C6:AC"
    pub fn value(&self) -> String {
        self.0.value.clone()
    }

    /// Returns the algorithm used (e.g. "sha-256").
    /// See https://datatracker.ietf.org/doc/html/rfc8122#section-5
    pub fn algorithm(&self) -> String {
        self.0.algorithm.clone()
    }
}

impl From<&[u8; 32]> for Fingerprint {
    fn from(t: &[u8; 32]) -> Self {
        let values: Vec<String> = t.iter().map(|x| format! {"{:02X}", x}).collect();
        Self::new_sha256(values.join(":"))
    }
}

impl From<Multihash> for Fingerprint {
    fn from(h: Multihash) -> Self {
        // Only support SHA-256 (0x12) for now.
        assert_eq!(h.code(), 0x12);
        let values: Vec<String> = h.digest().iter().map(|x| format! {"{:02X}", x}).collect();
        Self::new_sha256(values.join(":"))
    }
}

// TODO: derive when RTCDtlsFingerprint implements Eq.
impl PartialEq for Fingerprint {
    fn eq(&self, other: &Self) -> bool {
        self.0.algorithm == other.algorithm() && self.0.value == other.value()
    }
}
impl Eq for Fingerprint {}
