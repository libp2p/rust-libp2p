use rustls::{CertificateError, pki_types::PrivatePkcs8KeyDer, quic::Connection};

use super::{
    test_support::{certificate_for, client, rsa_fixtures, server},
    *,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sender {
    Client,
    Server,
}

#[derive(Clone, Copy, Debug)]
enum InvalidProof {
    Certificate,
    Extension,
    Transcript,
}

fn invalidate_transcript(bytes: &mut [u8]) -> usize {
    if bytes.is_empty() {
        0
    } else {
        assert!(bytes.len() >= 4);
        let length =
            usize::try_from(u32::from_be_bytes([0, bytes[1], bytes[2], bytes[3]])).unwrap();
        let (message, rest) = bytes.split_at_mut(4 + length);
        if message[0] == 15 {
            *message.last_mut().unwrap() ^= 1;
            1
        } else {
            invalidate_transcript(rest)
        }
    }
}

fn deliver(
    from: &mut Connection,
    to: &mut Connection,
    mutate: impl FnOnce(&mut [u8]),
) -> Result<(), rustls::Error> {
    let mut bytes = Vec::new();
    let _ = from.write_hs(&mut bytes);
    mutate(&mut bytes);
    if bytes.is_empty() {
        Ok(())
    } else {
        to.read_hs(&bytes).inspect_err(|_| {
            assert!(
                to.is_handshaking(),
                "invalid proof must not authenticate its sender"
            );
        })
    }
}

fn drive(
    client: &mut Connection,
    server: &mut Connection,
    mut mutate: impl FnMut(Sender, &mut [u8]),
) -> Result<(), rustls::Error> {
    (0..8).try_for_each(|_| {
        deliver(client, server, |bytes| mutate(Sender::Client, bytes))?;
        deliver(server, client, |bytes| mutate(Sender::Server, bytes))
    })
}

fn webpki_cause(error: &rustls::Error) -> Option<&webpki::Error> {
    if let rustls::Error::InvalidCertificate(CertificateError::Other(other)) = error {
        other.0.downcast_ref::<webpki::Error>()
    } else {
        None
    }
}

fn resolver(
    cert: rustls::pki_types::CertificateDer<'static>,
    key: &rustls::pki_types::PrivateKeyDer<'_>,
) -> Arc<AlwaysResolvesCert> {
    Arc::new(AlwaysResolvesCert(Arc::new(
        rustls::sign::CertifiedKey::new(
            vec![cert],
            rustls::crypto::aws_lc_rs::sign::any_supported_type(key).unwrap(),
        ),
    )))
}

fn invalid_certificate(
    identity: &identity::Keypair,
    algorithm: &'static rcgen::SignatureAlgorithm,
    proof: InvalidProof,
) -> (
    rustls::pki_types::CertificateDer<'static>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    match proof {
        InvalidProof::Certificate => {
            let (cert, key) = certificate_for(identity, algorithm);
            let mut bytes = cert.as_ref().to_vec();
            *bytes.last_mut().unwrap() ^= 1;
            (bytes.into(), key)
        }
        InvalidProof::Extension => {
            let key = rcgen::KeyPair::generate_for(algorithm).unwrap();
            let message = [
                P2P_SIGNING_PREFIX.as_slice(),
                key.public_key_der().as_slice(),
            ]
            .concat();
            let mut signature = identity.sign(&message).unwrap();
            signature[0] ^= 1;
            let mut params = rcgen::CertificateParams::default();
            params
                .custom_extensions
                .push(rcgen::CustomExtension::from_oid_content(
                    &P2P_EXT_OID,
                    yasna::encode_der(&(identity.public().encode_protobuf(), signature)),
                ));
            let cert: rustls::pki_types::CertificateDer<'static> =
                params.self_signed(&key).unwrap().into();
            // The certificate signature is valid independently of the invalid identity proof.
            let parsed = parse_unverified(cert.as_ref()).unwrap();
            parsed
                .verify_signature(
                    parsed.signature_scheme().unwrap(),
                    parsed.certificate.tbs_certificate.as_ref(),
                    parsed.certificate.signature_value.as_ref(),
                )
                .unwrap();
            (cert, PrivatePkcs8KeyDer::from(key.serialize_der()).into())
        }
        InvalidProof::Transcript => certificate_for(identity, algorithm),
    }
}

#[test]
fn independently_invalid_proofs_fail_in_both_directions() {
    [
        &rcgen::PKCS_ECDSA_P256_SHA256,
        &rcgen::PKCS_ECDSA_P384_SHA384,
        &rcgen::PKCS_ED25519,
    ]
    .into_iter()
    .flat_map(|algorithm| [Sender::Client, Sender::Server].map(|sender| (algorithm, sender)))
    .flat_map(|(algorithm, sender)| {
        [
            InvalidProof::Certificate,
            InvalidProof::Extension,
            InvalidProof::Transcript,
        ]
        .map(|proof| (algorithm, sender, proof))
    })
    .for_each(|(algorithm, sender, proof)| {
        let client_key = identity::Keypair::generate_ed25519();
        let server_key = identity::Keypair::generate_ed25519();
        let mut client_config =
            crate::make_client_config(&client_key, Some(server_key.public().to_peer_id())).unwrap();
        let mut server_config = crate::make_server_config(&server_key).unwrap();
        let identity = match sender {
            Sender::Client => &client_key,
            Sender::Server => &server_key,
        };
        let (cert, key) = invalid_certificate(identity, algorithm, proof);
        if let InvalidProof::Transcript = proof {
            assert!(parse(&cert).is_ok());
        } else {
            assert!(
                parse(&cert).is_err(),
                "the public parser must still verify certificates"
            );
        }
        match sender {
            Sender::Client => client_config.client_auth_cert_resolver = resolver(cert, &key),
            Sender::Server => server_config.cert_resolver = resolver(cert, &key),
        }
        let mut client = client(Arc::new(client_config));
        let mut server = server(Arc::new(server_config));
        let mut corrupted = 0;
        let error = drive(&mut client, &mut server, |source, bytes| {
            if source == sender && matches!(proof, InvalidProof::Transcript) {
                corrupted += invalidate_transcript(bytes);
            }
        })
        .expect_err("invalid authentication proof must abort the handshake");
        match proof {
            InvalidProof::Certificate => assert_eq!(
                webpki_cause(&error),
                // Pre-existing label: `P2pCertificate::verify` maps every self-signature
                // failure to `SignatureAlgorithmMismatch`.
                Some(&webpki::Error::SignatureAlgorithmMismatch),
                "{error:?}"
            ),
            InvalidProof::Extension => assert_eq!(
                webpki_cause(&error),
                Some(&webpki::Error::UnknownIssuer),
                "{error:?}"
            ),
            InvalidProof::Transcript => {
                assert_eq!(corrupted, 1);
                assert_eq!(
                    error,
                    rustls::Error::InvalidCertificate(CertificateError::BadSignature)
                );
            }
        }
    });
}

#[test]
fn expected_peer_id_is_still_enforced() {
    let client_key = identity::Keypair::generate_ed25519();
    let server_key = identity::Keypair::generate_ed25519();
    let unexpected_peer = identity::Keypair::generate_ed25519().public().to_peer_id();
    let mut client = client(Arc::new(
        crate::make_client_config(&client_key, Some(unexpected_peer)).unwrap(),
    ));
    let mut server = server(Arc::new(crate::make_server_config(&server_key).unwrap()));
    assert_eq!(
        drive(&mut client, &mut server, |_, _| {}).unwrap_err(),
        rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure),
    );
}

#[test]
fn rsa_signature_schemes_retain_verification_and_scheme_checks() {
    let key = rustls::pki_types::PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        include_bytes!("test_assets/rsa-2048.pk8").to_vec(),
    ));
    let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&key).unwrap();
    rsa_fixtures().into_iter().for_each(|(name, bytes)| {
        let der = rustls::pki_types::CertificateDer::from(bytes);
        let scheme = parse(&der).unwrap().signature_scheme().unwrap();
        let signer = signing_key.choose_scheme(&[scheme]).unwrap();
        let message = b"independent transcript signature";
        let mut signature = signer.sign(message).unwrap();
        verify_tls13_signature(&der, scheme, message, &signature).unwrap();
        signature[0] ^= 1;
        assert_eq!(
            verify_tls13_signature(&der, scheme, message, &signature).unwrap_err(),
            rustls::Error::InvalidCertificate(CertificateError::BadSignature),
            "{name}"
        );
        assert!(
            verify_tls13_signature(&der, rustls::SignatureScheme::ED25519, message, &signature)
                .is_err()
        );
    });
}

#[test]
fn valid_certificate_schemes_establish_authenticated_connections() {
    let identity = identity::Keypair::generate_ed25519();
    let generated = [
        &rcgen::PKCS_ECDSA_P256_SHA256,
        &rcgen::PKCS_ECDSA_P384_SHA384,
        &rcgen::PKCS_ED25519,
    ]
    .into_iter()
    .map(|algorithm| certificate_for(&identity, algorithm));
    let rsa = rsa_fixtures()
        .into_iter()
        // RSA negotiation prefers SHA-512. The existing verifier requires the
        // transcript scheme to match the certificate's self-signature scheme.
        .filter(|(name, _)| *name == "rsa_pss_sha512")
        .map(|(_, bytes)| {
            (
                rustls::pki_types::CertificateDer::from(bytes),
                PrivatePkcs8KeyDer::from(include_bytes!("test_assets/rsa-2048.pk8").to_vec())
                    .into(),
            )
        });
    generated
        .chain(rsa)
        .flat_map(|(certificate, key)| {
            [Sender::Client, Sender::Server]
                .map(|sender| (certificate.clone(), key.clone_key(), sender))
        })
        .for_each(|(certificate, key, sender)| {
            let fixture_id = parse(&certificate).unwrap().peer_id();
            let scheme = parse(&certificate).unwrap().signature_scheme().unwrap();
            let client_key = identity::Keypair::generate_ed25519();
            let server_key = identity::Keypair::generate_ed25519();
            let (client_id, server_id) = match sender {
                Sender::Client => (fixture_id, server_key.public().to_peer_id()),
                Sender::Server => (client_key.public().to_peer_id(), fixture_id),
            };
            let mut client_config =
                crate::make_client_config(&client_key, Some(server_id)).unwrap();
            let mut server_config = crate::make_server_config(&server_key).unwrap();
            match sender {
                Sender::Client => {
                    client_config.client_auth_cert_resolver = resolver(certificate, &key)
                }
                Sender::Server => server_config.cert_resolver = resolver(certificate, &key),
            }
            let mut client = client(Arc::new(client_config));
            let mut server = server(Arc::new(server_config));
            drive(&mut client, &mut server, |_, _| {})
                .unwrap_or_else(|error| panic!("{scheme:?} from {sender:?}: {error:?}"));
            assert!(!client.is_handshaking());
            assert!(!server.is_handshaking());
            assert_eq!(
                parse(&client.peer_certificates().unwrap()[0])
                    .unwrap()
                    .peer_id(),
                server_id
            );
            assert_eq!(
                parse(&server.peer_certificates().unwrap()[0])
                    .unwrap()
                    .peer_id(),
                client_id
            );
        });
}

#[test]
fn shared_verifier_isolates_concurrent_failed_and_cancelled_handshakes() {
    let server_key = identity::Keypair::generate_ed25519();
    let server_id = server_key.public().to_peer_id();
    let server_config = Arc::new(crate::make_server_config(&server_key).unwrap());
    let barrier = Arc::new(std::sync::Barrier::new(8));
    std::thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| {
                let server_config = server_config.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    let client_key = identity::Keypair::generate_ed25519();
                    let client_id = client_key.public().to_peer_id();
                    let client_config =
                        Arc::new(crate::make_client_config(&client_key, Some(server_id)).unwrap());
                    barrier.wait();
                    // Cancel after the first flight, then fail a different session on the same
                    // verifier.
                    {
                        let mut client = client(client_config.clone());
                        let mut server = server(server_config.clone());
                        deliver(&mut client, &mut server, |_| {}).unwrap();
                    }
                    {
                        let mut client = client(client_config.clone());
                        let mut server = server(server_config.clone());
                        assert!(
                            drive(&mut client, &mut server, |source, bytes| {
                                if source == Sender::Client {
                                    invalidate_transcript(bytes);
                                }
                            })
                            .is_err()
                        );
                    }
                    let mut client = client(client_config);
                    let mut server = server(server_config);
                    drive(&mut client, &mut server, |_, _| {}).unwrap();
                    assert!(!client.is_handshaking());
                    assert!(!server.is_handshaking());
                    assert_eq!(
                        parse(&client.peer_certificates().unwrap()[0])
                            .unwrap()
                            .peer_id(),
                        server_id
                    );
                    assert_eq!(
                        parse(&server.peer_certificates().unwrap()[0])
                            .unwrap()
                            .peer_id(),
                        client_id
                    );
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .for_each(|handle| handle.join().unwrap());
    });
}
