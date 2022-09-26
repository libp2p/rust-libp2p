// Copyright 2021 Parity Technologies (UK) Ltd.
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

//! TLS configuration for QUIC based on libp2p TLS specs.

pub(crate) mod certificate;
mod verifier;

use std::sync::Arc;

use rustls::{
    cipher_suite::{
        TLS13_AES_128_GCM_SHA256, TLS13_AES_256_GCM_SHA384, TLS13_CHACHA20_POLY1305_SHA256,
    },
    SupportedCipherSuite,
};

/// A list of the TLS 1.3 cipher suites supported by rustls.
// By default rustls creates client/server configs with both
// TLS 1.3 __and__ 1.2 cipher suites. But we don't need 1.2.
static TLS13_CIPHERSUITES: &[SupportedCipherSuite] = &[
    // TLS1.3 suites
    TLS13_CHACHA20_POLY1305_SHA256,
    TLS13_AES_256_GCM_SHA384,
    TLS13_AES_128_GCM_SHA256,
];

const P2P_ALPN: [u8; 6] = *b"libp2p";

/// Create a TLS client configuration for libp2p.
pub fn make_client_config(
    keypair: &libp2p_core::identity::Keypair,
) -> Result<rustls::ClientConfig, rcgen::RcgenError> {
    let (certificate, key) = make_cert_key(keypair)?;

    let mut crypto = rustls::ClientConfig::builder()
        .with_cipher_suites(TLS13_CIPHERSUITES)
        .with_safe_default_kx_groups()
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("Cipher suites and kx groups are configured; qed")
        .with_custom_certificate_verifier(Arc::new(verifier::Libp2pCertificateVerifier))
        .with_single_cert(vec![certificate], key)
        .expect("Client cert key DER is valid; qed");
    crypto.alpn_protocols = vec![P2P_ALPN.to_vec()];

    Ok(crypto)
}

/// Create a TLS server configuration for libp2p.
pub fn make_server_config(
    keypair: &libp2p_core::identity::Keypair,
) -> Result<rustls::ServerConfig, rcgen::RcgenError> {
    let (certificate, key) = make_cert_key(keypair)?;

    let mut crypto = rustls::ServerConfig::builder()
        .with_cipher_suites(TLS13_CIPHERSUITES)
        .with_safe_default_kx_groups()
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("Cipher suites and kx groups are configured; qed")
        .with_client_cert_verifier(Arc::new(verifier::Libp2pCertificateVerifier))
        .with_single_cert(vec![certificate], key)
        .expect("Server cert key DER is valid; qed");
    crypto.alpn_protocols = vec![P2P_ALPN.to_vec()];

    Ok(crypto)
}

/// Create a random private key and certificate signed with this key for rustls.
fn make_cert_key(
    keypair: &libp2p_core::identity::Keypair,
) -> Result<(rustls::Certificate, rustls::PrivateKey), rcgen::RcgenError> {
    let cert = certificate::make_certificate(keypair)?;
    let private_key = cert.serialize_private_key_der();

    let cert = rustls::Certificate(cert.serialize_der()?);
    let key = rustls::PrivateKey(private_key);

    Ok((cert, key))
}
