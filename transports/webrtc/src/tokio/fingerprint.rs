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
