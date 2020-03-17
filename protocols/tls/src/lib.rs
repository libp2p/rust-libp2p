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
#![deny(
    exceeding_bitshifts,
    invalid_type_param_default,
    missing_fragment_specifier,
    mutable_transmutes,
    no_mangle_const_items,
    overflowing_literals,
    patterns_in_fns_without_body,
    pub_use_of_private_extern_crate,
    unknown_crate_types,
    const_err,
    order_dependent_trait_objects,
    illegal_floating_point_literal_pattern,
    improper_ctypes,
    late_bound_lifetime_arguments,
    non_camel_case_types,
    non_shorthand_field_patterns,
    non_snake_case,
    non_upper_case_globals,
    no_mangle_generic_items,
    path_statements,
    private_in_public,
    safe_packed_borrows,
    stable_features,
    type_alias_bounds,
    tyvar_behind_raw_pointer,
    unconditional_recursion,
    unused,
    unused_allocation,
    unused_comparisons,
    unused_mut,
    unreachable_pub,
    while_true,
    anonymous_parameters,
    bare_trait_objects,
    elided_lifetimes_in_paths,
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    clippy::all
)]
#![forbid(unsafe_code)]

mod certificate;
mod verifier;

pub use certificate::extract_peerid;
use std::sync::Arc;

const LIBP2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";
const LIBP2P_SIGNING_PREFIX_LENGTH: usize = LIBP2P_SIGNING_PREFIX.len();
const LIBP2P_OID_BYTES: &[u8] = &[43, 6, 1, 4, 1, 131, 162, 90, 1, 1];

fn make_client_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
    verifier: Arc<verifier::Libp2pCertificateVerifier>,
) -> rustls::ClientConfig {
    let mut crypto = rustls::ClientConfig::new();
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto.alpn_protocols = vec![b"libp2p".to_vec()];
    crypto.enable_early_data = false;
    crypto
        .set_single_client_cert(vec![certificate], key)
        .expect("we have a valid certificate; qed");
    crypto.dangerous().set_certificate_verifier(verifier);
    crypto
}

fn make_server_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
    verifier: Arc<verifier::Libp2pCertificateVerifier>,
) -> rustls::ServerConfig {
    let mut crypto = rustls::ServerConfig::new(verifier);
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto.alpn_protocols = vec![b"libp2p".to_vec()];
    crypto
        .set_single_cert(vec![certificate], key)
        .expect("we have a valid certificate; qed");
    crypto
}

/// Create TLS client and server configurations for libp2p.
pub fn make_tls_config(
    keypair: &libp2p_core::identity::Keypair,
) -> (rustls::ClientConfig, rustls::ServerConfig) {
    let cert = certificate::make_cert(&keypair);
    let private_key = cert.serialize_private_key_der();
    let verifier = Arc::new(verifier::Libp2pCertificateVerifier);
    let cert = rustls::Certificate(
        cert.serialize_der()
            .expect("serialization of a valid cert will succeed; qed"),
    );
    let key = rustls::PrivateKey(private_key);
    (
        make_client_config(cert.clone(), key.clone(), verifier.clone()),
        make_server_config(cert, key, verifier),
    )
}
