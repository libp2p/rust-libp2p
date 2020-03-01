// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! TLS configuration for `libp2p-quic`.

mod certificate;
mod verifier;

pub(crate) use certificate::extract_peerid;
use std::sync::Arc;

const LIBP2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";
const LIBP2P_SIGNING_PREFIX_LENGTH: usize = LIBP2P_SIGNING_PREFIX.len();
const LIBP2P_OID_BYTES: &[u8] = &[43, 6, 1, 4, 1, 131, 162, 90, 1, 1];

fn make_client_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
) -> rustls::ClientConfig {
    let mut crypto = rustls::ClientConfig::new();
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto.enable_early_data = true;
    crypto
        .set_single_client_cert(vec![certificate], key)
        .expect("we have a valid certificate; qed");
    let verifier = verifier::VeryInsecureRequireExactlyOneSelfSignedServerCertificate;
    crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(verifier));
    crypto
}

fn make_server_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
) -> rustls::ServerConfig {
    let mut crypto = rustls::ServerConfig::new(Arc::new(
        verifier::VeryInsecureRequireExactlyOneSelfSignedClientCertificate,
    ));
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto
        .set_single_cert(vec![certificate], key)
        .expect("we have a valid certificate; qed");
    crypto
}

/// Create TLS client and server configurations for libp2p.
pub(crate) fn make_tls_config(
    keypair: &libp2p_core::identity::Keypair,
) -> (rustls::ClientConfig, rustls::ServerConfig) {
    let cert = certificate::make_cert(&keypair);
    let private_key = cert.serialize_private_key_der();
    let (cert, key) = (
        rustls::Certificate(
            cert.serialize_der()
                .expect("serialization of a valid cert will succeed; qed"),
        ),
        rustls::PrivateKey(private_key),
    );
    (
        make_client_config(cert.clone(), key.clone()),
        make_server_config(cert, key),
    )
}
