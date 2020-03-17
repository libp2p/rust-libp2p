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

use super::der;
use der::Tag;
use libp2p_core::identity::PublicKey;
use ring::signature;
use untrusted::{Input, Reader};
use webpki::Error;

pub(super) struct X509Certificate<'a> {
    libp2p_extension: Libp2pExtension<'a>,
    x509_tbs: Input<'a>,
    #[allow(dead_code)]
    x509_validity: Input<'a>,
    x509_spki: SubjectPublicKeyInfo<'a>,
    x509_signature_algorithm: Input<'a>,
    x509_signature: Input<'a>,
}

type Result<T> = std::result::Result<T, Error>;

/// An X.509 SubjectPublicKeyInfo
struct SubjectPublicKeyInfo<'a> {
    x509_pkey_algorithm: &'a [u8],
    x509_pkey_bytes: &'a [u8],
}

/// Read an X.509 SubjectPublicKeyInfo
fn read_spki<'a>(input: Input<'a>) -> Result<SubjectPublicKeyInfo<'a>> {
    input.read_all(Error::BadDER, |input| {
        let algorithm = der::expect_tag_and_get_value(input, Tag::Sequence)?;
        let public_key = der::bit_string_with_no_unused_bits(input)?;
        Ok(SubjectPublicKeyInfo {
            x509_pkey_algorithm: algorithm.as_slice_less_safe(),
            x509_pkey_bytes: public_key.as_slice_less_safe(),
        })
    })
}

/// Read a single extension. If it is the libp2p extension, store it in `libp2p`, unless `libp2p`,
/// is already `Some`, which is an error. Any critical extension that is not the libp2p extension
/// is also an error.
fn read_extension<'a>(
    input: &mut Reader<'a>,
    libp2p: &mut Option<Libp2pExtension<'a>>,
) -> Result<()> {
    use super::super::LIBP2P_OID_BYTES;
    der::nested_sequence(input, |input| {
        let oid = der::expect_tag_and_get_value(input, Tag::OID)?;
        let critical = input.peek(Tag::Boolean as _);
        if critical && input.read_bytes(3) != Ok(Input::from(&[1, 1, 255][..])) {
            return Err(Error::BadDER);
        }
        let extension =
            der::expect_tag_and_get_value(input, Tag::OctetString).map_err(|_| Error::BadDER)?;
        match oid.as_slice_less_safe() {
            LIBP2P_OID_BYTES if libp2p.is_some() => Err(Error::BadDER),
            LIBP2P_OID_BYTES => {
                *libp2p = Some(
                    parse_libp2p_extension(extension).map_err(|_| Error::ExtensionValueInvalid)?,
                );
                Ok(())
            }
            _ if critical => return Err(Error::UnsupportedCriticalExtension),
            _ => Ok(()),
        }
    })
}

/// Read X.509 extensions.
fn read_extensions<'a>(input: Input<'a>) -> Result<Libp2pExtension<'a>> {
    input.read_all(Error::BadDER, |input| {
        der::nested_sequence(input, |input| {
            let mut libp2p = None;
            while !input.at_end() {
                read_extension(input, &mut libp2p)?;
            }
            libp2p.ok_or(Error::RequiredEKUNotFound)
        })
    })
}

struct Libp2pExtension<'a> {
    peer_key: PublicKey,
    signature: &'a [u8],
}

#[inline]
fn expect_sequence<'a>(input: &mut Reader<'a>) -> Result<Input<'a>> {
    der::expect_tag_and_get_value(input, Tag::Sequence)
}

fn parse_libp2p_extension<'a>(extension: Input<'a>) -> Result<Libp2pExtension<'a>> {
    Input::read_all(&extension, Error::ExtensionValueInvalid, |input| {
        der::nested_sequence(input, |input| {
            let public_key = der::bit_string_with_no_unused_bits(input)?.as_slice_less_safe();
            let signature = der::bit_string_with_no_unused_bits(input)?.as_slice_less_safe();
            // We deliberately discard the error information because this is
            // either a broken peer or an attack.
            let peer_key =
                PublicKey::from_protobuf_encoding(public_key).map_err(|_| Error::BadDER)?;
            Ok(Libp2pExtension {
                signature,
                peer_key,
            })
        })
    })
}

/// Extracts the algorithm id and public key from a certificate
pub(super) fn parse_certificate(certificate: &[u8]) -> Result<X509Certificate<'_>> {
    let certificate = Input::from(certificate).read_all(Error::BadDER, expect_sequence)?;
    let (x509_tbs, inner_tbs, x509_signature_algorithm, x509_signature) =
        certificate.read_all(Error::BadDER, |input| {
            // tbsCertificate
            let (signed_tbs, parsed_tbs) = input.read_partial(expect_sequence)?;
            // signatureAlgorithm
            let signature_algorithm = expect_sequence(input)?;
            // signatureValue
            let signature = der::bit_string_with_no_unused_bits(input)?;
            Ok((signed_tbs, parsed_tbs, signature_algorithm, signature))
        })?;

    let (x509_validity, x509_spki, libp2p_extension) =
        inner_tbs.read_all(Error::BadDER, |mut input| {
            // We require extensions, which means we require version 3
            if input
                .read_bytes(5)
                .map_err(|_| Error::BadDER)?
                .as_slice_less_safe()
                != [160, 3, 2, 1, 2]
            {
                return Err(Error::UnsupportedCertVersion);
            }
            // serialNumber
            der::positive_integer(&mut input).map_err(|_| Error::BadDER)?;
            // signature
            if expect_sequence(input)? != x509_signature_algorithm {
                // signature algorithms don’t match
                return Err(Error::SignatureAlgorithmMismatch);
            }
            // issuer
            expect_sequence(input)?;
            // validity
            let x509_validity = expect_sequence(input)?;
            // subject
            expect_sequence(input)?;
            // subjectPublicKeyInfo
            let x509_spki = expect_sequence(input)?;
            // parse the extensions
            let exts = der::expect_tag_and_get_value(input, Tag::ContextSpecificConstructed3)?;
            Ok((x509_validity, x509_spki, exts))
        })?;
    Ok(X509Certificate {
        libp2p_extension: read_extensions(libp2p_extension)?,
        x509_tbs,
        x509_validity,
        x509_spki: read_spki(x509_spki)?,
        x509_signature,
        x509_signature_algorithm,
    })
}

fn verify_libp2p_signature(
    libp2p_extension: Libp2pExtension<'_>,
    x509_pkey_bytes: &[u8],
) -> Result<()> {
    let mut v =
        Vec::with_capacity(super::super::LIBP2P_SIGNING_PREFIX_LENGTH + x509_pkey_bytes.len());
    v.extend_from_slice(&super::super::LIBP2P_SIGNING_PREFIX[..]);
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

pub(super) fn verify_certificate(certificate: X509Certificate<'_>) -> Result<()> {
    let X509Certificate {
        x509_tbs,
        x509_spki:
            SubjectPublicKeyInfo {
                x509_pkey_algorithm,
                x509_pkey_bytes,
            },
        x509_signature,
        x509_signature_algorithm,
        libp2p_extension,
        ..
    } = certificate;
    let pkey = verify_self_signature(
        x509_pkey_algorithm,
        x509_pkey_bytes,
        x509_signature_algorithm.as_slice_less_safe(),
    )?;
    verify_libp2p_signature(libp2p_extension, x509_pkey_bytes)?;
    pkey.verify(
        x509_tbs.as_slice_less_safe(),
        x509_signature.as_slice_less_safe(),
    )
    .map_err(|_| Error::InvalidSignatureForPublicKey)
}

fn verify_self_signature<'a>(
    x509_pkey_alg: &[u8],
    x509_pkey_bytes: &'a [u8],
    x509_signature_algorithm: &[u8],
) -> Result<ring::signature::UnparsedPublicKey<&'a [u8]>> {
    let algorithm: &'static dyn signature::VerificationAlgorithm =
        match (x509_signature_algorithm, x509_pkey_alg) {
            (include_bytes!("data/alg-rsa-pkcs1-sha256.der"), _) => {
                if x509_pkey_alg == include_bytes!("data/alg-rsa-encryption.der") {
                    &signature::RSA_PKCS1_2048_8192_SHA256
                } else {
                    return Err(Error::UnsupportedSignatureAlgorithmForPublicKey);
                }
            }
            (
                include_bytes!("data/alg-rsa-pkcs1-sha384.der"),
                include_bytes!("data/alg-rsa-encryption.der"),
            ) => &signature::RSA_PKCS1_2048_8192_SHA384,
            (
                include_bytes!("data/alg-rsa-pkcs1-sha512.der"),
                include_bytes!("data/alg-rsa-encryption.der"),
            ) => &signature::RSA_PKCS1_2048_8192_SHA512,
            (
                include_bytes!("data/alg-ecdsa-sha256.der"),
                include_bytes!("data/alg-ecdsa-p256.der"),
            ) => &signature::ECDSA_P256_SHA256_ASN1,
            (
                include_bytes!("data/alg-ecdsa-sha384.der"),
                include_bytes!("data/alg-ecdsa-p384.der"),
            ) => &signature::ECDSA_P384_SHA384_ASN1,
            (
                include_bytes!("data/alg-ecdsa-sha384.der"),
                include_bytes!("data/alg-ecdsa-p256.der"),
            ) => &signature::ECDSA_P256_SHA384_ASN1,
            (
                include_bytes!("data/alg-ecdsa-sha256.der"),
                include_bytes!("data/alg-ecdsa-p384.der"),
            ) => &signature::ECDSA_P384_SHA256_ASN1,
            (include_bytes!("data/alg-ed25519.der"), include_bytes!("data/alg-ed25519.der")) => {
                &signature::ED25519
            }
            (
                [0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a, ..],
                include_bytes!("data/alg-rsa-encryption.der"),
            ) => parse_rsa_pss(&x509_pkey_alg[11..])?,
            _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
        };
    Ok(signature::UnparsedPublicKey::new(
        algorithm,
        x509_pkey_bytes,
    ))
}

// While the RSA-PSS parameters are a ASN.1 SEQUENCE, it is simpler to match
// against the 12 different possibilities. The binary files are *generated* by a
// Go program.
fn parse_rsa_pss(data: &[u8]) -> Result<&'static signature::RsaParameters> {
    match data {
        include_bytes!("data/alg-rsa-pss-sha256-v0.der")
        | include_bytes!("data/alg-rsa-pss-sha256-v1.der")
        | include_bytes!("data/alg-rsa-pss-sha256-v2.der")
        | include_bytes!("data/alg-rsa-pss-sha256-v3.der") => {
            Ok(&signature::RSA_PSS_2048_8192_SHA256)
        }
        include_bytes!("data/alg-rsa-pss-sha384-v0.der")
        | include_bytes!("data/alg-rsa-pss-sha384-v1.der")
        | include_bytes!("data/alg-rsa-pss-sha384-v2.der")
        | include_bytes!("data/alg-rsa-pss-sha384-v3.der") => {
            Ok(&signature::RSA_PSS_2048_8192_SHA384)
        }
        include_bytes!("data/alg-rsa-pss-sha512-v0.der")
        | include_bytes!("data/alg-rsa-pss-sha512-v1.der")
        | include_bytes!("data/alg-rsa-pss-sha512-v2.der")
        | include_bytes!("data/alg-rsa-pss-sha512-v3.der") => {
            Ok(&signature::RSA_PSS_2048_8192_SHA512)
        }
        _ => Err(Error::UnsupportedSignatureAlgorithm),
    }
}
