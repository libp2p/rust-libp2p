// These tests verify interoperability with Go and JS libp2p implementations
// using the standard peer-record format.
#[cfg(test)]
mod tests {
    use libp2p_core::{PeerRecord, SignedEnvelope};

    const JS_FIXTURE: &[u8] = include_bytes!("fixtures/js/js-signed-peer-record.bin");
    const GO_FIXTURE: &[u8] = include_bytes!("fixtures/go/go-signed-peer-record.bin");

    #[test]
    fn verify_js_signed_peer_record() {
        let envelope = SignedEnvelope::from_protobuf_encoding(JS_FIXTURE)
            .expect("Failed to decode envelope from JS");

        let peer_record =
            PeerRecord::from_signed_envelope_interop(envelope).expect("Failed to verify JS peer record");

        assert!(!peer_record.addresses().is_empty());
    }

    #[test]
    fn verify_go_signed_peer_record() {
        let envelope = SignedEnvelope::from_protobuf_encoding(GO_FIXTURE)
            .expect("Failed to decode envelope from Go");

        let peer_record =
            PeerRecord::from_signed_envelope_interop(envelope).expect("Failed to verify Go peer record");

        assert!(!peer_record.addresses().is_empty());
    }
}
