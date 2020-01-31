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

static ALL_SUPPORTED_SIGNATURE_ALGORITHMS: &'static [&'static webpki::SignatureAlgorithm] = {
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
pub struct VeryInsecureRequireExactlyOneSelfSignedServerCertificate;

/// A ClientCertVerifier that requires client authentication, and requires the certificate to be self-signed.
///
/// “Isn’t that insecure?”, you may ask.  Yes, it is!  That’s why this struct has the name it does!
/// This doesn’t cause a vulnerability in libp2p-quic, however.  libp2p-quic accepts any self-signed
/// certificate with a valid libp2p extension **by design**.  Instead, it is the application’s job
/// to check the peer ID that libp2p-quic provides.  libp2p-quic does guarantee that the connection
/// is to a peer with the secret key corresponing to its `PeerId`, unless that endpoint has done
/// something insecure.
pub struct VeryInsecureRequireExactlyOneSelfSignedClientCertificate;

impl rustls::ServerCertVerifier for VeryInsecureRequireExactlyOneSelfSignedServerCertificate {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef,
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

fn verify_presented_certs(
    presented_certs: &[rustls::Certificate],
    cb: &dyn Fn(
        webpki::Time,
        webpki::EndEntityCert,
        webpki::TrustAnchor,
    ) -> Result<(), webpki::Error>,
) -> Result<(), rustls::TLSError> {
    if presented_certs.len() != 1 {
        Err(rustls::TLSError::NoCertificatesPresented)?
    }
    let time = webpki::Time::try_from(std::time::SystemTime::now())
        .map_err(|ring::error::Unspecified| rustls::TLSError::FailedToGetCurrentTime)?;
    let raw_certificate = presented_certs[0].as_ref();
    let inner_func = || {
        let parsed_cert = webpki::EndEntityCert::from(raw_certificate)?;
        let trust_anchor = webpki::trust_anchor_util::cert_der_as_trust_anchor(raw_certificate)?;
        cb(time, parsed_cert, trust_anchor)
    };
    inner_func().map_err(rustls::TLSError::WebPKIError)
}

impl rustls::ClientCertVerifier for VeryInsecureRequireExactlyOneSelfSignedClientCertificate {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_root_subjects(&self) -> rustls::internal::msgs::handshake::DistinguishedNames {
        Default::default()
    }

    fn verify_client_cert(
        &self,
        presented_certs: &[rustls::Certificate],
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
