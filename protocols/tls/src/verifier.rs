// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

static ALL_SUPPORTED_SIGNATURE_ALGORITHMS: &[&webpki::SignatureAlgorithm] = {
    &[
        &webpki::ECDSA_P256_SHA256,
        &webpki::ED25519,
        &webpki::ECDSA_P384_SHA384,
        &webpki::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
        &webpki::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
        &webpki::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
        // Deprecated and/or undesirable algorithms.

        // deprecated elliptic curve algorithms
        &webpki::ECDSA_P384_SHA256,
        &webpki::ECDSA_P256_SHA384,
        // RSA PKCS 1.5
        &webpki::RSA_PKCS1_2048_8192_SHA256,
        &webpki::RSA_PKCS1_2048_8192_SHA384,
        &webpki::RSA_PKCS1_2048_8192_SHA512,
        &webpki::RSA_PKCS1_3072_8192_SHA384,
    ]
};

/// Libp2p client and server certificate verifier.
pub(crate) struct Libp2pCertificateVerifier;

/// libp2p requires the following of X.509 server certificate chains:
///
/// * Exactly one certificate must be presented.
/// * The certificate must be self-signed.
/// * The certificate must have a valid libp2p extension that includes
///   a signature of its public key.
///
/// The check that the [`PeerId`] matches the expected `PeerId` must be done by
/// the caller.
///
/// [`PeerId`]: libp2p_core::PeerId
impl rustls::ServerCertVerifier for Libp2pCertificateVerifier {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef<'_>,
        _ocsp_response: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        let (end_entity_cert, trust_anchor) = verify_presented_certs(presented_certs)?;
        end_entity_cert
            .verify_is_valid_tls_server_cert(
                ALL_SUPPORTED_SIGNATURE_ALGORITHMS,
                &webpki::TLSServerTrustAnchors(&[trust_anchor]),
                &[],
                get_time()?,
            )
            .map(|()| rustls::ServerCertVerified::assertion())
            .map_err(rustls::TLSError::WebPKIError)
    }
}

fn verify_libp2p_extension(
    extension: untrusted::Input<'_>,
    subject_public_key_info: untrusted::Input<'_>,
) -> Result<(), ring::error::Unspecified> {
    use ring::{error::Unspecified, io::der};
    use libp2p_core::identity::PublicKey;
    let certificate_key = subject_public_key_info.read_all(Unspecified, |mut reader| {
        der::expect_tag_and_get_value(&mut reader, der::Tag::Sequence)?;
        der::bit_string_with_no_unused_bits(&mut reader)
    })?;
    extension.read_all(Unspecified, |mut reader| {
        let inner = der::expect_tag_and_get_value(&mut reader, der::Tag::Sequence)?;
        inner.read_all(Unspecified, |mut reader| {
            let public_key = der::bit_string_with_no_unused_bits(&mut reader)?;
            let signature = der::bit_string_with_no_unused_bits(&mut reader)?;
            // We deliberately discard the error information because this is
            // either a broken peer or an attack.
            let public_key = PublicKey::from_protobuf_encoding(public_key.as_slice_less_safe())
                .map_err(|_| Unspecified)?;
            let mut v = Vec::with_capacity(super::LIBP2P_SIGNING_PREFIX_LENGTH + certificate_key.len());
            v.extend_from_slice(&super::LIBP2P_SIGNING_PREFIX[..]);
            v.extend_from_slice(certificate_key.as_slice_less_safe());
            if public_key.verify(&v, signature.as_slice_less_safe()) {
                Ok(())
            } else {
                Err(Unspecified)
            }
        })
    })
}

pub(super) fn verify_single_cert(
    raw_certificate: &[u8],
) -> Result<(webpki::EndEntityCert<'_>, webpki::TrustAnchor<'_>), rustls::TLSError> {
    let mut num_libp2p_extensions = 0usize;
    let cb = &mut |oid: untrusted::Input<'_>, value, _, spki| match oid.as_slice_less_safe() {
        super::LIBP2P_OID_BYTES => {
            num_libp2p_extensions += 1;
            if verify_libp2p_extension(value, spki).is_err() {
                num_libp2p_extensions = 2; // this will force an error
            }
            webpki::Understood::Yes
        }
        _ => webpki::Understood::No,
    };
    let parsed_cert = webpki::EndEntityCert::from_with_extension_cb(raw_certificate, cb)
        .map_err(rustls::TLSError::WebPKIError)?;
    if num_libp2p_extensions != 1 {
        // this also includes the case where an extension was not valid
        return Err(rustls::TLSError::WebPKIError(webpki::Error::UnknownIssuer));
    }
    let trust_anchor = webpki::trust_anchor_util::cert_der_as_trust_anchor(raw_certificate)
        .map_err(rustls::TLSError::WebPKIError)?;
    Ok((parsed_cert, trust_anchor))
}

fn get_time() -> Result<webpki::Time, rustls::TLSError> {
    webpki::Time::try_from(std::time::SystemTime::now())
        .map_err(|ring::error::Unspecified| rustls::TLSError::FailedToGetCurrentTime)
}

fn verify_presented_certs(
    presented_certs: &[rustls::Certificate],
) -> Result<(webpki::EndEntityCert<'_>, webpki::TrustAnchor<'_>), rustls::TLSError> {
    if presented_certs.len() != 1 {
        return Err(rustls::TLSError::NoCertificatesPresented);
    }
    verify_single_cert(presented_certs[0].as_ref())
}

/// libp2p requires the following of X.509 client certificate chains:
///
/// * Exactly one certificate must be presented. In particular, client
///   authentication is mandatory in libp2p.
/// * The certificate must be self-signed.
/// * The certificate must have a valid libp2p extension that includes
///   a signature of its public key.
///
/// The check that the [`PeerId`] matches the expected `PeerId` must be done by
/// the caller.
///
/// [`PeerId`]: libp2p_core::PeerId
impl rustls::ClientCertVerifier for Libp2pCertificateVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_root_subjects(
        &self,
        _dns_name: Option<&webpki::DNSName>,
    ) -> Option<rustls::DistinguishedNames> {
        Some(vec![])
    }

    fn verify_client_cert(
        &self,
        presented_certs: &[rustls::Certificate],
        _dns_name: Option<&webpki::DNSName>,
    ) -> Result<rustls::ClientCertVerified, rustls::TLSError> {
        let (end_entity_cert, trust_anchor) = verify_presented_certs(presented_certs)?;
        end_entity_cert
            .verify_is_valid_tls_client_cert(
                ALL_SUPPORTED_SIGNATURE_ALGORITHMS,
                &webpki::TLSClientTrustAnchors(&[trust_anchor]),
                &[],
                get_time()?,
            )
            .map(|()| rustls::ClientCertVerified::assertion())
            .map_err(rustls::TLSError::WebPKIError)
    }
}
