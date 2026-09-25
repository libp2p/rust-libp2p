//! Helpers shared by the certificate handshake tests and the manual CPU profile.

use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    quic::{ClientConnection, Connection, ServerConnection, Version},
};

use super::*;

pub(super) fn certificate_for(
    identity: &identity::Keypair,
    algorithm: &'static rcgen::SignatureAlgorithm,
) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let key = rcgen::KeyPair::generate_for(algorithm).unwrap();
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .custom_extensions
        .push(make_libp2p_extension(identity, &key).unwrap());
    (
        params.self_signed(&key).unwrap().into(),
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
    )
}

pub(super) fn client(config: Arc<rustls::ClientConfig>) -> Connection {
    Connection::Client(
        ClientConnection::new(
            config,
            Version::V1,
            "libp2p".try_into().unwrap(),
            Vec::new(),
        )
        .unwrap(),
    )
}

pub(super) fn server(config: Arc<rustls::ServerConfig>) -> Connection {
    Connection::Server(ServerConnection::new(config, Version::V1, Vec::new()).unwrap())
}

pub(super) fn transfer(from: &mut Connection, to: &mut Connection) -> Result<(), rustls::Error> {
    let mut bytes = Vec::new();
    let _ = from.write_hs(&mut bytes);
    if bytes.is_empty() {
        Ok(())
    } else {
        to.read_hs(&bytes)
    }
}

pub(super) fn rsa_fixtures() -> [(&'static str, &'static [u8]); 6] {
    [
        (
            "rsa_pkcs1_sha256",
            include_bytes!("test_assets/rsa_fixture_pkcs1_sha256.der"),
        ),
        (
            "rsa_pkcs1_sha384",
            include_bytes!("test_assets/rsa_fixture_pkcs1_sha384.der"),
        ),
        (
            "rsa_pkcs1_sha512",
            include_bytes!("test_assets/rsa_fixture_pkcs1_sha512.der"),
        ),
        (
            "rsa_pss_sha256",
            include_bytes!("test_assets/rsa_fixture_pss_sha256.der"),
        ),
        (
            "rsa_pss_sha384",
            include_bytes!("test_assets/rsa_fixture_pss_sha384.der"),
        ),
        (
            "rsa_pss_sha512",
            include_bytes!("test_assets/rsa_fixture_pss_sha512.der"),
        ),
    ]
}
