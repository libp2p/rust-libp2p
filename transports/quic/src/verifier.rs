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

/// A ServerCertVerifier that considers any certificate to be valid, and does no checking
/// whatsoever.
///
/// “Isn’t that insecure?”, you may ask.  Yes, it is!  That’s why this struct has the name it does!
/// This doesn’t cause a vulnerability in libp2p-quic, however.  libp2p-quic accepts any certificate
/// **by design**.  Instead, it is the application’s job to check the peer ID that libp2p-quic
/// provides.  libp2p-quic does guarantee that the connection is to a peer with the secret key
/// corresponing to its `PeerId`, unless that endpoint has done something insecure.
pub struct VeryInsecureAllowAllCertificatesWithoutChecking;

/// A ClientCertVerifier that requires client authentication, but considers any certificate to be
/// valid, and does no checking whatsoever.
///
/// “Isn’t that insecure?”, you may ask.  Yes, it is!  That’s why this struct has the name it does!
/// This doesn’t cause a vulnerability in libp2p-quic, however.  libp2p-quic accepts any certificate
/// **by design**.  Instead, it is the application’s job to check the peer ID that libp2p-quic
/// provides.  libp2p-quic does guarantee that the connection is to a peer with the secret key
/// corresponing to its `PeerId`, unless that endpoint has done something insecure.
pub struct VeryInsecureRequireClientCertificateButDoNotCheckIt;

impl rustls::ServerCertVerifier for VeryInsecureAllowAllCertificatesWithoutChecking {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef,
        _ocsp_response: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
		if presented_certs.len() > 0 {
			Ok(rustls::ServerCertVerified::assertion())
		} else {
			Err(rustls::TLSError::NoCertificatesPresented)
		}
    }
}

impl rustls::ClientCertVerifier for VeryInsecureRequireClientCertificateButDoNotCheckIt {
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
		if presented_certs.len() > 0 {
			Ok(rustls::ClientCertVerified::assertion())
		} else {
			Err(rustls::TLSError::NoCertificatesPresented)
		}
    }
}
