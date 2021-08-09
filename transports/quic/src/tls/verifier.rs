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

use libp2p::identity::PublicKey;
use libp2p::PeerId;
use ring::io::der;
use rustls::{
    internal::msgs::handshake::DigitallySignedStruct, Certificate, ClientCertVerified,
    HandshakeSignatureValid, ServerCertVerified, TLSError,
};
use untrusted::{Input, Reader};
use webpki::Error;

/// Implementation of the `rustls` certificate verification traits for libp2p.
///
/// Only TLS 1.3 is supported. TLS 1.2 should be disabled in the configuration of `rustls`.
pub(crate) struct Libp2pServerCertificateVerifier(pub(crate) PeerId);

/// libp2p requires the following of X.509 server certificate chains:
///
/// - Exactly one certificate must be presented.
/// - The certificate must be self-signed.
/// - The certificate must have a valid libp2p extension that includes a
///   signature of its public key.
impl rustls::ServerCertVerifier for Libp2pServerCertificateVerifier {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef<'_>,
        _ocsp_response: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        let peer_id = verify_presented_certs(presented_certs)?;
        if peer_id != self.0 {
            return Err(TLSError::PeerIncompatibleError(
                "Unexpected peer id".to_string(),
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &Certificate,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        Err(TLSError::PeerIncompatibleError(
            "Only TLS 1.3 certificates are supported".to_string(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &Certificate,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        verify_tls13_signature(message, cert, dss)
    }
}

/// Implementation of the `rustls` certificate verification traits for libp2p.
///
/// Only TLS 1.3 is supported. TLS 1.2 should be disabled in the configuration of `rustls`.
pub(crate) struct Libp2pClientCertificateVerifier;

/// libp2p requires the following of X.509 client certificate chains:
///
/// - Exactly one certificate must be presented. In particular, client
///   authentication is mandatory in libp2p.
/// - The certificate must be self-signed.
/// - The certificate must have a valid libp2p extension that includes a
///   signature of its public key.
impl rustls::ClientCertVerifier for Libp2pClientCertificateVerifier {
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
        presented_certs: &[Certificate],
        _dns_name: Option<&webpki::DNSName>,
    ) -> Result<ClientCertVerified, rustls::TLSError> {
        verify_presented_certs(presented_certs).map(|_| ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &Certificate,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        Err(TLSError::PeerIncompatibleError(
            "Only TLS 1.3 certificates are supported".to_string(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &Certificate,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        barebones_x509::parse_certificate(cert.as_ref())
            .map_err(rustls::TLSError::WebPKIError)?
            .check_tls13_signature(dss.scheme, message, dss.sig.0.as_ref())
            .map_err(rustls::TLSError::WebPKIError)
            .map(|()| rustls::HandshakeSignatureValid::assertion())
    }
}

fn verify_tls13_signature(
    message: &[u8],
    cert: &Certificate,
    dss: &DigitallySignedStruct,
) -> Result<HandshakeSignatureValid, TLSError> {
    barebones_x509::parse_certificate(cert.as_ref())
        .map_err(rustls::TLSError::WebPKIError)?
        .check_tls13_signature(dss.scheme, message, dss.sig.0.as_ref())
        .map_err(rustls::TLSError::WebPKIError)
        .map(|()| rustls::HandshakeSignatureValid::assertion())
}

fn verify_libp2p_signature(
    libp2p_extension: &Libp2pExtension<'_>,
    x509_pkey_bytes: &[u8],
) -> Result<(), Error> {
    let mut v = Vec::with_capacity(super::LIBP2P_SIGNING_PREFIX_LENGTH + x509_pkey_bytes.len());
    v.extend_from_slice(&super::LIBP2P_SIGNING_PREFIX[..]);
    v.extend_from_slice(x509_pkey_bytes);
    if libp2p_extension
        .peer_key
        .verify(&v, libp2p_extension.signature)
    {
        Ok(())
    } else {
        Err(Error::UnknownIssuer)
    }
}

fn parse_certificate(
    certificate: &[u8],
) -> Result<(barebones_x509::X509Certificate<'_>, Libp2pExtension<'_>), Error> {
    let parsed = barebones_x509::parse_certificate(certificate)?;
    let mut libp2p_extension = None;

    parsed
        .extensions()
        .iterate(&mut |oid, critical, extension| {
            match oid {
                super::LIBP2P_OID_BYTES if libp2p_extension.is_some() => return Err(Error::BadDER),
                super::LIBP2P_OID_BYTES => {
                    libp2p_extension = Some(parse_libp2p_extension(extension)?)
                }
                _ if critical => return Err(Error::UnsupportedCriticalExtension),
                _ => {}
            };
            Ok(())
        })?;
    let libp2p_extension = libp2p_extension.ok_or(Error::UnknownIssuer)?;
    Ok((parsed, libp2p_extension))
}

fn verify_presented_certs(presented_certs: &[Certificate]) -> Result<PeerId, TLSError> {
    if presented_certs.len() != 1 {
        return Err(TLSError::NoCertificatesPresented);
    }
    let (certificate, extension) =
        parse_certificate(presented_certs[0].as_ref()).map_err(TLSError::WebPKIError)?;
    certificate.valid().map_err(TLSError::WebPKIError)?;
    certificate
        .check_self_issued()
        .map_err(TLSError::WebPKIError)?;
    verify_libp2p_signature(&extension, certificate.subject_public_key_info().spki())
        .map_err(TLSError::WebPKIError)?;
    Ok(PeerId::from_public_key(extension.peer_key))
}

struct Libp2pExtension<'a> {
    peer_key: PublicKey,
    signature: &'a [u8],
}

fn parse_libp2p_extension(extension: Input<'_>) -> Result<Libp2pExtension<'_>, Error> {
    fn read_bit_string<'a>(input: &mut Reader<'a>, e: Error) -> Result<Input<'a>, Error> {
        // The specification states that this is a BIT STRING, but the Go implementation
        // uses an OCTET STRING.  OCTET STRING is superior in this context, so use it.
        der::expect_tag_and_get_value(input, der::Tag::OctetString).map_err(|_| e)
    }

    let e = Error::ExtensionValueInvalid;
    Input::read_all(&extension, e, |input| {
        der::nested(input, der::Tag::Sequence, e, |input| {
            let public_key = read_bit_string(input, e)?.as_slice_less_safe();
            let signature = read_bit_string(input, e)?.as_slice_less_safe();
            // We deliberately discard the error information because this is
            // either a broken peer or an attack.
            let peer_key = PublicKey::from_protobuf_encoding(public_key).map_err(|_| e)?;
            Ok(Libp2pExtension {
                peer_key,
                signature,
            })
        })
    })
}

/// Extracts the `PeerId` from a certificate’s libp2p extension. It is erroneous
/// to call this unless the certificate is known to be a well-formed X.509
/// certificate with a valid libp2p extension. The certificate verifier in this
/// module check this.
///
/// # Panics
///
/// Panics if called on an invalid certificate.
pub fn extract_peerid_or_panic(certificate: &[u8]) -> PeerId {
    let r = parse_certificate(certificate)
        .expect("we already checked that the certificate was valid during the handshake; qed");
    PeerId::from_public_key(r.1.peer_key)
}
