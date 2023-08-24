// Copyright 2023 Doug Anderson <douganderson444@peerpiper.io>
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

pub(crate) use libp2p_webrtc_utils::fingerprint::Fingerprint;
use libp2p_webrtc_utils::fingerprint::SHA256;
use webrtc::dtls_transport::dtls_fingerprint::RTCDtlsFingerprint;

/// Converts [`RTCDtlsFingerprint`] to [`Fingerprint`].
pub(crate) fn try_from_rtc_dtls(fp: &RTCDtlsFingerprint) -> Option<Fingerprint> {
    if fp.algorithm != SHA256 {
        return None;
    }

    let mut buf = [0; 32];
    hex::decode_to_slice(fp.value.replace(':', ""), &mut buf).ok()?;

    Some(Fingerprint::from(buf))
}
