use futures::*;
use tokio_io::codec::length_delimited;
use crate::codec::{FullCodec};
use tokio_io::{AsyncRead, AsyncWrite};
use crate::error::TlsError;
use crate::{TlsConfig};
use libp2p_core::{PublicKey, Negotiated};
use bytes::BytesMut;
use std::sync::Arc;
use std::io::Write;
use rustls::{RootCertStore, Session, NoClientAuth, AllowAnyAuthenticatedClient,
             AllowAnyAnonymousOrAuthenticatedClient};

pub fn handshake<S>(mut socket: S, config: TlsConfig) -> impl Future<Item = (FullCodec<S>, PublicKey), Error = TlsError>
    where
        S: AsyncRead + AsyncWrite + Send,
{

    let (certificates, private_key) = {
        let key = openssl::rsa::Rsa::generate(2048).unwrap();

        let mut certif = openssl::x509::X509Builder::new().unwrap();        // TODO:
        certif.set_version(2).unwrap();
        let mut serial: [u8; 20] = rand::random();
        certif.set_serial_number(&openssl::asn1::Asn1Integer::from_bn(&openssl::bn::BigNum::from_slice(&serial[..]).unwrap()).unwrap()).unwrap();
        let mut x509_name = openssl::x509::X509NameBuilder::new().unwrap();
        x509_name.append_entry_by_text("C", "US").unwrap();
        x509_name.append_entry_by_text("ST", "CA").unwrap();
        x509_name.append_entry_by_text("O", "Some organization").unwrap();
        x509_name.append_entry_by_text("CN", "libp2p.io").unwrap();
        let x509_name = x509_name.build();

        certif.set_issuer_name(&x509_name).unwrap();
        certif.set_subject_name(&x509_name).unwrap();
        // TODO: libp2p specs says we shouldn't have these date fields
        certif.set_not_before(&openssl::asn1::Asn1Time::days_from_now(0).unwrap()).unwrap();
        certif.set_not_after(&openssl::asn1::Asn1Time::days_from_now(365).unwrap()).unwrap();
        certif.set_pubkey(openssl::pkey::PKey::from_rsa(key.clone()).unwrap().as_ref()).unwrap();        // TODO: unwrap

        let ext = openssl::x509::extension::BasicConstraints::new()
            .critical()
            //.ca()
            .build().unwrap();
        certif.append_extension(ext).unwrap();

        let ext = openssl::x509::extension::SubjectKeyIdentifier::new()
            .build(&certif.x509v3_context(None, None)).unwrap();
        certif.append_extension(ext).unwrap();

        let ext = openssl::x509::extension::AuthorityKeyIdentifier::new()
            .issuer(true)
            .keyid(true)
            .build(&certif.x509v3_context(None, None)).unwrap();
        certif.append_extension(ext).unwrap();

        let ext = openssl::x509::extension::SubjectAlternativeName::new()
            .dns("libp2p.io")    // TODO: must match the domain name in the QUIC requests being made
            .build(&certif.x509v3_context(None, None)).unwrap();
        certif.append_extension(ext).unwrap();

        // TODO:
        /*{
            let ext = format!("publicKey={}", bs58::encode(keypair.public().into_protobuf_encoding()).into_string());  // TODO: signature
            certif.append_extension(openssl::x509::X509Extension::new(None, None, "1.3.6.1.4.1.53594.1.1", &ext).unwrap());
        }*/

        certif.sign(&openssl::pkey::PKey::from_rsa(key.clone()).unwrap(), openssl::hash::MessageDigest::sha256()).unwrap();
        let certif_gen = certif.build();
        debug_assert!(certif_gen.verify(&openssl::pkey::PKey::from_rsa(key.clone()).unwrap()).unwrap());
        let pkey_bytes = key.private_key_to_der().unwrap();
        (vec![rustls::Certificate(certif_gen.to_der().unwrap())], rustls::PrivateKey(pkey_bytes))
    };

    let mut server_config = rustls::ServerConfig::new(rustls::NoClientAuth::new());
    server_config.set_single_cert(certificates, private_key)
        .expect("bad certificates/private key");
    let mut server_session = rustls::ServerSession::new(&Arc::new(server_config));

    let mut client_config = rustls::ClientConfig::new();
    let libp2p_io = webpki::DNSNameRef::try_from_ascii_str("libp2p.io").unwrap();
    let mut client_session = rustls::ClientSession::new(&Arc::new(client_config), libp2p_io);

    let full_codec = FullCodec::new(
        socket,
        client_session,
        server_session,
    );

    future::ok((full_codec, config.key.public()))
    /*
    socket.send(BytesMut::from(config.key.public().into_protobuf_encoding()))
        .from_err()
        .and_then(|s| {
            s.into_future()
                .map_err(|(e, _)| e.into())
                .and_then(move |(bytes, s)| {
                    let mut client_config = rustls::ClientConfig::new();
                    let libp2p_io = webpki::DNSNameRef::try_from_ascii_str("libp2p.io").unwrap();
                    let mut client_session = rustls::ClientSession::new(&Arc::new(client_config), libp2p_io);

                    let pubkey = PublicKey::from_protobuf_encoding(&bytes.unwrap());
                    Ok((full_codec(s), pubkey.unwrap()))
                })
        })
        */
}
