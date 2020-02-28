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

use libp2p_core::identity;

const LIBP2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";
const LIBP2P_SIGNING_PREFIX_LENGTH: usize = LIBP2P_SIGNING_PREFIX.len();

static ALL_SUPPORTED_SIGNATURE_ALGORITHMS: &[&webpki::SignatureAlgorithm] = {
    &[
        &webpki::ECDSA_P256_SHA256,
        &webpki::ECDSA_P256_SHA384,
        &webpki::ECDSA_P384_SHA256,
        &webpki::ECDSA_P384_SHA384,
        &webpki::ED25519,
        &webpki::RSA_PKCS1_2048_8192_SHA256,
        &webpki::RSA_PKCS1_2048_8192_SHA384,
        &webpki::RSA_PKCS1_2048_8192_SHA512,
        &webpki::RSA_PKCS1_3072_8192_SHA384,
        &webpki::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
        &webpki::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
        &webpki::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
    ]
};

/// A ServerCertVerifier that considers any self-signed certificate to be valid.
///
/// “Isn’t that insecure?”, you may ask.  Yes, it is!  That’s why this struct has the name it does!
/// This doesn’t cause a vulnerability in libp2p-quic, however.  libp2p-quic accepts any self-signed
/// certificate with a valid libp2p extension **by design**.  Instead, it is the application’s job
/// to check the peer ID that libp2p-quic provides.  libp2p-quic does guarantee that the connection
/// is to a peer with the secret key corresponing to its `PeerId`, unless that endpoint has done
/// something insecure.
pub(crate) struct VeryInsecureRequireExactlyOneSelfSignedServerCertificate;

/// A ClientCertVerifier that requires client authentication, and requires the certificate to be
/// self-signed.
///
/// “Isn’t that insecure?”, you may ask.  Yes, it is!  That’s why this struct has the name it does!
/// This doesn’t cause a vulnerability in libp2p-quic, however.  libp2p-quic accepts any self-signed
/// certificate with a valid libp2p extension **by design**.  Instead, it is the application’s job
/// to check the peer ID that libp2p-quic provides.  libp2p-quic does guarantee that the connection
/// is to a peer with the secret key corresponing to its `PeerId`, unless that endpoint has done
/// something insecure.
pub(crate) struct VeryInsecureRequireExactlyOneSelfSignedClientCertificate;

impl rustls::ServerCertVerifier for VeryInsecureRequireExactlyOneSelfSignedServerCertificate {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef<'_>,
        _ocsp_response: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        verify_presented_certs(presented_certs, &|time, end_entity_cert, trust_anchor| {
            end_entity_cert.verify_is_valid_tls_server_cert(
                ALL_SUPPORTED_SIGNATURE_ALGORITHMS,
                &webpki::TLSServerTrustAnchors(&[trust_anchor]),
                &[],
                time,
            )
        })
        .map(|()| rustls::ServerCertVerified::assertion())
    }
}

#[derive(Copy, Clone)]
enum ExtensionStatus {
    NotFound,
    Good,
    Error,
}

fn verify_presented_certs(
    presented_certs: &[rustls::Certificate],
    cb: &dyn Fn(
        webpki::Time,
        webpki::EndEntityCert<'_>,
        webpki::TrustAnchor<'_>,
    ) -> Result<(), webpki::Error>,
) -> Result<(), rustls::TLSError> {
    if presented_certs.len() != 1 {
        return Err(rustls::TLSError::NoCertificatesPresented);
    }
    let time = webpki::Time::try_from(std::time::SystemTime::now())
        .map_err(|ring::error::Unspecified| rustls::TLSError::FailedToGetCurrentTime)?;
    let raw_certificate = presented_certs[0].as_ref();
    let inner_func = || {
        let mut status = ExtensionStatus::NotFound;
        let parsed_cert = webpki::EndEntityCert::from_with_extension_cb(
            raw_certificate,
            &mut |oid, value, _critical, spki| match (oid.as_slice_less_safe(), status) {
                ([43, 6, 1, 4, 1, 131, 162, 90, 1, 1], ExtensionStatus::NotFound) => {
                    status = if verify_libp2p_extension(value, spki).is_ok() {
                        ExtensionStatus::Good
                    } else {
                        ExtensionStatus::Error
                    };
                    webpki::Understood::Yes
                }
                ([43, 6, 1, 4, 1, 131, 162, 90, 1, 1], _) => {
                    status = ExtensionStatus::Error;
                    webpki::Understood::Yes
                }
                _ => webpki::Understood::No,
            },
        )?;
        match status {
            ExtensionStatus::Good => {}
            ExtensionStatus::Error | ExtensionStatus::NotFound => {
                return Err(webpki::Error::UnknownIssuer)
            }
        }
        let trust_anchor = webpki::trust_anchor_util::cert_der_as_trust_anchor(raw_certificate)?;
        cb(time, parsed_cert, trust_anchor)
    };
    inner_func().map_err(rustls::TLSError::WebPKIError)
}

fn verify_libp2p_extension(
    extension: untrusted::Input<'_>,
    subject_public_key_info: untrusted::Input<'_>,
) -> Result<identity::PublicKey, ring::error::Unspecified> {
    use ring::{error::Unspecified, io::der};
    let spki = subject_public_key_info.read_all(Unspecified, |mut reader| {
        der::expect_tag_and_get_value(&mut reader, der::Tag::Sequence)?;
        Ok(der::bit_string_with_no_unused_bits(&mut reader)?.as_slice_less_safe())
    })?;
    extension.read_all(Unspecified, |mut reader| {
        let inner = der::expect_tag_and_get_value(&mut reader, der::Tag::Sequence)?;
        inner.read_all(Unspecified, |mut reader| {
            let public_key = der::bit_string_with_no_unused_bits(&mut reader)?.as_slice_less_safe();
            let signature = der::bit_string_with_no_unused_bits(&mut reader)?.as_slice_less_safe();
            let public_key =
                identity::PublicKey::from_protobuf_encoding(public_key).map_err(|_| Unspecified)?;
            let mut v = Vec::with_capacity(LIBP2P_SIGNING_PREFIX_LENGTH + spki.len());
            v.extend_from_slice(&LIBP2P_SIGNING_PREFIX[..]);
            v.extend_from_slice(spki);
            if public_key.verify(&v, signature) {
                Ok(public_key)
            } else {
                Err(Unspecified)
            }
        })
    })
}

impl rustls::ClientCertVerifier for VeryInsecureRequireExactlyOneSelfSignedClientCertificate {
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
        verify_presented_certs(presented_certs, &|time, end_entity_cert, trust_anchor| {
            end_entity_cert.verify_is_valid_tls_client_cert(
                ALL_SUPPORTED_SIGNATURE_ALGORITHMS,
                &webpki::TLSClientTrustAnchors(&[trust_anchor]),
                &[],
                time,
            )
        })
        .map(|()| rustls::ClientCertVerified::assertion())
    }
}
