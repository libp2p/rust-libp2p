use libp2p_identity::Keypair;

#[test]
fn calling_keypair_api() {
    let _ = Keypair::from_protobuf_encoding(&[]);
}

#[allow(dead_code)]
fn using_keypair(kp: Keypair) {
    let _ = kp.to_protobuf_encoding();
    let _ = kp.sign(&[]);
    let _ = kp.public();
}
