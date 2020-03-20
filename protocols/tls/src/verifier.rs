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

mod calendar;
mod der;
mod x509;

/// Libp2p client and server certificate verifier.
pub(crate) struct Libp2pCertificateVerifier;

/// libp2p requires the following of X.509 server certificate chains:
///
/// * Exactly one certificate must be presented.
/// * The certificate must be self-signed.
/// * The certificate must have a valid libp2p extension that includes a
///   signature of its public key.
///
/// The check that the [`PeerId`] matches the expected `PeerId` must be done by
/// the caller.
///
/// [`PeerId`]: libp2p_core::PeerId
impl rustls::ServerCertVerifier for Libp2pCertificateVerifier {
    fn verify_server_cert(
        &self, _roots: &rustls::RootCertStore, presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef<'_>, _ocsp_response: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        verify_presented_certs(presented_certs).map(|()| rustls::ServerCertVerified::assertion())
    }
}

fn get_time() -> Result<webpki::Time, rustls::TLSError> {
    webpki::Time::try_from(std::time::SystemTime::now())
        .map_err(|ring::error::Unspecified| rustls::TLSError::FailedToGetCurrentTime)
}

fn verify_presented_certs(presented_certs: &[rustls::Certificate]) -> Result<(), rustls::TLSError> {
    if presented_certs.len() != 1 {
        return Err(rustls::TLSError::NoCertificatesPresented);
    }
    x509::verify_certificate(
        x509::parse_certificate(presented_certs[0].as_ref())
            .map_err(rustls::TLSError::WebPKIError)?,
        get_time()?,
    )
    .map_err(rustls::TLSError::WebPKIError)
}

/// libp2p requires the following of X.509 client certificate chains:
///
/// * Exactly one certificate must be presented. In particular, client
///   authentication is mandatory in libp2p.
/// * The certificate must be self-signed.
/// * The certificate must have a valid libp2p extension that includes a
///   signature of its public key.
///
/// The check that the [`PeerId`] matches the expected `PeerId` must be done by
/// the caller.
///
/// [`PeerId`]: libp2p_core::PeerId
impl rustls::ClientCertVerifier for Libp2pCertificateVerifier {
    fn offer_client_auth(&self) -> bool { true }

    fn client_auth_root_subjects(
        &self, _dns_name: Option<&webpki::DNSName>,
    ) -> Option<rustls::DistinguishedNames> {
        Some(vec![])
    }

    fn verify_client_cert(
        &self, presented_certs: &[rustls::Certificate], _dns_name: Option<&webpki::DNSName>,
    ) -> Result<rustls::ClientCertVerified, rustls::TLSError> {
        verify_presented_certs(presented_certs).map(|()| rustls::ClientCertVerified::assertion())
    }
}
