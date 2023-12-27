use libp2p_identity::Keypair;

#[test]
fn calling_keypair_api() {
    let _ = Keypair::try_decode_protobuf(&[]);
}

#[allow(dead_code)]
fn using_keypair(kp: Keypair) {
    let _ = kp.encode_protobuf();
    let _ = kp.sign(&[]);
    let _ = kp.public();
    let _: Option<[u8; 32]> = kp.derive_secret(b"foobar");
}
