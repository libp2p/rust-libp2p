//! Run with `cargo test --release -p libp2p-tls --lib profile_handshakes -- --ignored --nocapture`.
//! Timings exclude certificate generation and network I/O. No timing assertions are made.

use std::{
    hint::black_box,
    sync::{Arc, Barrier},
    time::Instant,
};

use rustls::{
    HandshakeKind,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    quic::{ClientConnection, Connection, ServerConnection, Version},
};

use super::*;

const MESSAGE: &[u8] = b"a TLS 1.3 CertificateVerify transcript for profiling";
const SAMPLES: usize = 7;

fn measure(label: &str, iterations: u32, mut operation: impl FnMut()) {
    (0..10).for_each(|_| operation());
    let mut samples = (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            (0..iterations).for_each(|_| operation());
            start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations)
        })
        .collect::<Vec<_>>();
    samples.sort_by(f64::total_cmp);
    println!(
        "PROFILE,{label},{iterations},{:.3},{:.3},{:.3}",
        samples[0],
        samples[SAMPLES / 2],
        samples[SAMPLES - 1]
    );
}

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

fn profile_certificate(label: &str, der: &CertificateDer<'_>, key: &PrivateKeyDer<'_>) {
    let cert = parse(der).unwrap();
    let scheme = cert.signature_scheme().unwrap();
    let signer = rustls::crypto::aws_lc_rs::sign::any_supported_type(key)
        .unwrap()
        .choose_scheme(&[scheme])
        .unwrap();
    let transcript_signature = signer.sign(MESSAGE).unwrap();
    let extension_message = [
        P2P_SIGNING_PREFIX.as_slice(),
        cert.certificate.public_key().raw,
    ]
    .concat();

    measure(&format!("{label}/parse_only"), 200, || {
        black_box(parse_unverified(black_box(der.as_ref())).unwrap());
    });
    measure(&format!("{label}/certificate_signature"), 200, || {
        cert.verify_signature(
            scheme,
            black_box(cert.certificate.tbs_certificate.as_ref()),
            black_box(cert.certificate.signature_value.as_ref()),
        )
        .unwrap();
    });
    measure(&format!("{label}/extension_signature"), 200, || {
        assert!(cert.extension.public_key.verify(
            black_box(&extension_message),
            black_box(&cert.extension.signature),
        ));
    });
    measure(&format!("{label}/transcript_signature"), 200, || {
        cert.verify_signature(scheme, black_box(MESSAGE), black_box(&transcript_signature))
            .unwrap();
    });
    measure(&format!("{label}/verified_parse"), 200, || {
        black_box(parse(black_box(der)).unwrap());
    });
    measure(
        &format!("{label}/original_transcript_callback"),
        200,
        || {
            parse(black_box(der))
                .unwrap()
                .verify_signature(scheme, MESSAGE, &transcript_signature)
                .unwrap();
        },
    );
    measure(
        &format!("{label}/candidate_transcript_callback"),
        200,
        || {
            parse_unverified(black_box(der.as_ref()))
                .unwrap()
                .verify_signature(scheme, MESSAGE, &transcript_signature)
                .unwrap();
        },
    );
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

fn transfer(from: &mut Connection, to: &mut Connection) -> Result<(), rustls::Error> {
    let mut bytes = Vec::new();
    let _ = from.write_hs(&mut bytes);
    if bytes.is_empty() {
        Ok(())
    } else {
        to.read_hs(&bytes)
    }
}

fn handshake(
    client_config: Arc<rustls::ClientConfig>,
    server_config: Arc<rustls::ServerConfig>,
    kind: HandshakeKind,
) {
    let mut client = client(client_config);
    let mut server = server(server_config);
    // Fixed upper bound includes post-handshake tickets and key transitions.
    (0..8).for_each(|_| {
        transfer(&mut client, &mut server).unwrap();
        transfer(&mut server, &mut client).unwrap();
    });
    assert!(!client.is_handshaking());
    assert!(!server.is_handshaking());
    assert_eq!(client.handshake_kind(), Some(kind));
    assert_eq!(server.handshake_kind(), Some(kind));
    // Match the additional verified parse used to extract each established QUIC peer's identity.
    black_box(
        parse(&client.peer_certificates().unwrap()[0])
            .unwrap()
            .peer_id(),
    );
    black_box(
        parse(&server.peer_certificates().unwrap()[0])
            .unwrap()
            .peer_id(),
    );
}

fn profile_quic_tls() {
    let client_key = identity::Keypair::generate_ed25519();
    let server_key = identity::Keypair::generate_ed25519();
    let mut full_client =
        crate::make_client_config(&client_key, Some(server_key.public().to_peer_id())).unwrap();
    full_client.resumption = rustls::client::Resumption::disabled();
    let full_client = Arc::new(full_client);
    let server_config = Arc::new(crate::make_server_config(&server_key).unwrap());

    measure("quic_tls/client_first_flight", 100, || {
        let mut connection = client(full_client.clone());
        let mut bytes = Vec::new();
        let _ = connection.write_hs(&mut bytes);
        black_box(bytes);
    });
    let mut initial_client = client(full_client.clone());
    let mut client_hello = Vec::new();
    let _ = initial_client.write_hs(&mut client_hello);
    measure("quic_tls/server_first_flight", 100, || {
        let mut connection = server(server_config.clone());
        connection.read_hs(black_box(&client_hello)).unwrap();
        (0..3).for_each(|_| {
            let mut bytes = Vec::new();
            let _ = connection.write_hs(&mut bytes);
            black_box(bytes);
        });
        assert!(connection.is_handshaking());
    });
    measure("quic_tls/full_pair_with_peer_ids", 100, || {
        handshake(
            full_client.clone(),
            server_config.clone(),
            HandshakeKind::Full,
        );
    });

    let resumed_client = Arc::new(
        crate::make_client_config(&client_key, Some(server_key.public().to_peer_id())).unwrap(),
    );
    handshake(
        resumed_client.clone(),
        server_config.clone(),
        HandshakeKind::Full,
    );
    measure("quic_tls/resumed_pair_with_peer_ids", 100, || {
        handshake(
            resumed_client.clone(),
            server_config.clone(),
            HandshakeKind::Resumed,
        );
    });

    [1, 4, 8].into_iter().for_each(|workers| {
        measure(
            &format!("quic_tls/full_batch/{workers}_workers_20_each"),
            1,
            || {
                let barrier = Arc::new(Barrier::new(workers));
                std::thread::scope(|scope| {
                    let handles = (0..workers)
                        .map(|_| {
                            let client_config = full_client.clone();
                            let server_config = server_config.clone();
                            let barrier = barrier.clone();
                            scope.spawn(move || {
                                barrier.wait();
                                (0..20).for_each(|_| {
                                    handshake(
                                        client_config.clone(),
                                        server_config.clone(),
                                        HandshakeKind::Full,
                                    );
                                });
                            })
                        })
                        .collect::<Vec<_>>();
                    handles
                        .into_iter()
                        .for_each(|handle| handle.join().unwrap());
                });
            },
        );
    });
}

pub(super) fn rsa_fixtures() -> [(&'static str, &'static [u8]); 6] {
    [
        (
            "rsa_pkcs1_sha256",
            include_bytes!("test_assets/profile_rsa_pkcs1_sha256.der"),
        ),
        (
            "rsa_pkcs1_sha384",
            include_bytes!("test_assets/profile_rsa_pkcs1_sha384.der"),
        ),
        (
            "rsa_pkcs1_sha512",
            include_bytes!("test_assets/profile_rsa_pkcs1_sha512.der"),
        ),
        (
            "rsa_pss_sha256",
            include_bytes!("test_assets/profile_rsa_pss_sha256.der"),
        ),
        (
            "rsa_pss_sha384",
            include_bytes!("test_assets/profile_rsa_pss_sha384.der"),
        ),
        (
            "rsa_pss_sha512",
            include_bytes!("test_assets/profile_rsa_pss_sha512.der"),
        ),
    ]
}

#[test]
#[ignore = "manual release-mode CPU profile"]
fn profile_rsa_signatures() {
    assert!(
        !black_box(cfg!(debug_assertions)),
        "run this profile with --release"
    );
    println!("PROFILE,label,iterations,min_us,median_us,max_us");
    let key = PrivatePkcs8KeyDer::from(include_bytes!("test_assets/rsa-2048.pk8").to_vec()).into();
    rsa_fixtures().into_iter().for_each(|(name, bytes)| {
        profile_certificate(
            &format!("ed25519/{name}"),
            &CertificateDer::from(bytes),
            &key,
        );
    });
}

#[test]
#[ignore = "manual release-mode CPU profile"]
fn profile_handshakes() {
    assert!(
        !black_box(cfg!(debug_assertions)),
        "run this profile with --release"
    );
    println!("PROFILE,label,iterations,min_us,median_us,max_us");
    let rsa =
        identity::Keypair::rsa_from_pkcs8(&mut include_bytes!("test_assets/rsa-2048.pk8").to_vec())
            .unwrap();
    [
        ("ed25519", identity::Keypair::generate_ed25519()),
        ("ecdsa", identity::Keypair::generate_ecdsa()),
        ("secp256k1", identity::Keypair::generate_secp256k1()),
        ("rsa2048", rsa),
    ]
    .into_iter()
    .for_each(|(identity_name, identity)| {
        [
            ("p256", &rcgen::PKCS_ECDSA_P256_SHA256),
            ("p384", &rcgen::PKCS_ECDSA_P384_SHA384),
            ("ed25519", &rcgen::PKCS_ED25519),
        ]
        .into_iter()
        .for_each(|(certificate_name, algorithm)| {
            let (certificate, key) = certificate_for(&identity, algorithm);
            profile_certificate(
                &format!("{identity_name}/{certificate_name}"),
                &certificate,
                &key,
            );
        });
    });
    profile_quic_tls();
}
