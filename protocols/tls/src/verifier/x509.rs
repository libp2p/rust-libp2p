// Copyright 2020 Parity Technologies (UK) Ltd.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.

// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

use super::{
    super::{LIBP2P_OID_BYTES, LIBP2P_SIGNING_PREFIX, LIBP2P_SIGNING_PREFIX_LENGTH},
    der,
    der::Tag,
};
use libp2p_core::identity::PublicKey;
use ring::signature;
use untrusted::{Input, Reader};
use webpki::{Error, Time};

pub(super) struct X509Certificate<'a> {
    libp2p_extension: Libp2pExtension<'a>,
    x509_tbs: Input<'a>,
    x509_validity: Input<'a>,
    x509_spki: Input<'a>,
    x509_signature_algorithm: &'a [u8],
    x509_signature: Input<'a>,
}

struct Libp2pExtension<'a> {
    peer_key: PublicKey,
    signature: &'a [u8],
}

#[inline(always)]
fn read_sequence<'a>(input: &mut Reader<'a>) -> Result<Input<'a>, Error> {
    der::expect_tag_and_get_value(input, Tag::Sequence)
}

#[inline(always)]
fn read_bit_string<'a>(input: &mut Reader<'a>, e: Error) -> Result<Input<'a>, Error> {
    der::bit_string_with_no_unused_bits(input).map_err(|_| e)
}

fn parse_libp2p_extension<'a>(extension: Input<'a>) -> Result<Libp2pExtension<'a>, Error> {
    let e = Error::ExtensionValueInvalid;
    Input::read_all(&extension, e, |input| {
        der::nested(input, Tag::Sequence, e, |input| {
            let public_key = read_bit_string(input, e)?.as_slice_less_safe();
            let signature = read_bit_string(input, e)?.as_slice_less_safe();
            // We deliberately discard the error information because this is
            // either a broken peer or an attack.
            let peer_key = PublicKey::from_protobuf_encoding(public_key).map_err(|_| e)?;
            Ok(Libp2pExtension {
                signature,
                peer_key,
            })
        })
    })
}

/// Parse X.509 extensions
fn parse_extensions<'a>(input: &mut Reader<'a>) -> Result<Libp2pExtension<'a>, Error> {
    let mut libp2p_extension = None;
    while !input.at_end() {
        der::nested(input, Tag::Sequence, Error::BadDER, |input| {
            let oid = der::expect_tag_and_get_value(input, Tag::OID)?;
            let mut critical = false;
            if input.peek(Tag::Boolean as _) {
                critical = true;
                der::expect_bytes(input, &[Tag::Boolean as _, 1, 0xFF], Error::BadDER)?
            }
            let extension = der::expect_tag_and_get_value(input, Tag::OctetString)?;
            match oid.as_slice_less_safe() {
                LIBP2P_OID_BYTES if libp2p_extension.is_some() => return Err(Error::BadDER),
                LIBP2P_OID_BYTES => libp2p_extension = Some(parse_libp2p_extension(extension)?),
                _ if critical => return Err(Error::UnsupportedCriticalExtension),
                _ => {},
            }
            Ok(())
        })?
    }
    libp2p_extension.ok_or(Error::UnknownIssuer)
}

/// Extracts the algorithm id and public key from a certificate
pub(super) fn parse_certificate(certificate: &[u8]) -> Result<X509Certificate<'_>, Error> {
    let ((x509_tbs, inner_tbs), x509_signature_algorithm, x509_signature) =
        Input::from(certificate).read_all(Error::BadDER, |input| {
            der::nested(input, Tag::Sequence, Error::BadDER, |reader| {
                // tbsCertificate
                let tbs = reader.read_partial(|reader| read_sequence(reader))?;
                // signatureAlgorithm
                let signature_algorithm = read_sequence(reader)?.as_slice_less_safe();
                // signatureValue
                let signature = read_bit_string(reader, Error::BadDER)?;
                Ok((tbs, signature_algorithm, signature))
            })
        })?;
    let (x509_validity, x509_spki, extensions) =
        inner_tbs.read_all(Error::BadDER, |mut input| {
            // We require extensions, which means we require version 3
            der::expect_bytes(input, &[160, 3, 2, 1, 2], Error::UnsupportedCertVersion)?;
            // serialNumber
            der::positive_integer(&mut input)?;
            // signature
            if read_sequence(input)?.as_slice_less_safe() != x509_signature_algorithm {
                // signature algorithms don’t match
                return Err(Error::SignatureAlgorithmMismatch);
            }
            // issuer
            read_sequence(input)?;
            // validity
            let x509_validity = read_sequence(input)?;
            // subject
            read_sequence(input)?;
            // subjectPublicKeyInfo
            let spki = read_sequence(input)?;
            // subjectUniqueId and issuerUniqueId are unsupported
            // parse the extensions
            let extensions =
                der::expect_tag_and_get_value(input, Tag::ContextSpecificConstructed3)?;
            Ok((x509_validity, spki, extensions))
        })?;
    let extensions = extensions.read_all(Error::BadDER, read_sequence)?;
    let libp2p_extension = extensions.read_all(Error::BadDER, parse_extensions)?;

    Ok(X509Certificate {
        libp2p_extension,
        x509_tbs,
        x509_validity,
        x509_spki,
        x509_signature,
        x509_signature_algorithm,
    })
}

fn verify_libp2p_signature(
    libp2p_extension: Libp2pExtension<'_>, x509_pkey_bytes: &[u8],
) -> Result<(), Error> {
    let mut v = Vec::with_capacity(LIBP2P_SIGNING_PREFIX_LENGTH + x509_pkey_bytes.len());
    v.extend_from_slice(&LIBP2P_SIGNING_PREFIX[..]);
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

pub(super) fn get_peerid(certificate: X509Certificate<'_>) -> libp2p_core::PeerId {
    certificate.libp2p_extension.peer_key.into_peer_id()
}

pub(super) fn verify_certificate(
    certificate: X509Certificate<'_>, time: Time,
) -> Result<(), Error> {
    let X509Certificate {
        x509_tbs,
        x509_spki,
        x509_signature,
        x509_signature_algorithm,
        libp2p_extension,
        x509_validity,
    } = certificate;

    let (x509_pkey_alg, x509_pkey_bytes) = x509_spki.read_all(Error::BadDER, |input| {
        let x509_pkey_alg = read_sequence(input)?.as_slice_less_safe();
        let x509_pkey_bytes = read_bit_string(input, Error::BadDER)?.as_slice_less_safe();
        Ok((x509_pkey_alg, x509_pkey_bytes))
    })?;
    verify_libp2p_signature(libp2p_extension, x509_pkey_bytes)?;
    get_public_key(x509_pkey_alg, x509_pkey_bytes, x509_signature_algorithm)?
        .verify(
            x509_tbs.as_slice_less_safe(),
            x509_signature.as_slice_less_safe(),
        )
        .map_err(|_| Error::InvalidSignatureForPublicKey)?;
    x509_validity.read_all(Error::BadDER, |input| {
        let not_before = der::time_choice(input)?;
        let not_after = der::time_choice(input)?;
        if time < not_before {
            Err(Error::CertNotValidYet)
        } else if time > not_after {
            Err(Error::CertExpired)
        } else {
            Ok(())
        }
    })
}

fn get_public_key<'a>(
    x509_pkey_alg: &[u8], x509_pkey_bytes: &'a [u8], x509_signature_algorithm: &[u8],
) -> Result<ring::signature::UnparsedPublicKey<&'a [u8]>, Error> {
    const RSASSA_PSS_PREFIX: &[u8; 11] = include_bytes!("data/alg-rsa-pss.der");
    use signature::{
        RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_2048_8192_SHA384, RSA_PKCS1_2048_8192_SHA512,
    };
    let algorithm: &'static dyn signature::VerificationAlgorithm = match x509_signature_algorithm {
        include_bytes!("data/alg-rsa-pkcs1-sha256.der") => match x509_pkey_alg {
            include_bytes!("data/alg-rsa-encryption.der") => &RSA_PKCS1_2048_8192_SHA256,
            _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
        },
        include_bytes!("data/alg-rsa-pkcs1-sha384.der") => match x509_pkey_alg {
            include_bytes!("data/alg-rsa-encryption.der") => &RSA_PKCS1_2048_8192_SHA384,
            _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
        },
        include_bytes!("data/alg-rsa-pkcs1-sha512.der") => match x509_pkey_alg {
            include_bytes!("data/alg-rsa-encryption.der") => &RSA_PKCS1_2048_8192_SHA512,
            _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
        },
        include_bytes!("data/alg-ecdsa-sha256.der") => match x509_pkey_alg {
            include_bytes!("data/alg-ecdsa-p256.der") => &signature::ECDSA_P256_SHA256_ASN1,
            include_bytes!("data/alg-ecdsa-p384.der") => &signature::ECDSA_P384_SHA256_ASN1,
            _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
        },
        include_bytes!("data/alg-ecdsa-sha384.der") => match x509_pkey_alg {
            include_bytes!("data/alg-ecdsa-p256.der") => &signature::ECDSA_P256_SHA384_ASN1,
            include_bytes!("data/alg-ecdsa-p384.der") => &signature::ECDSA_P384_SHA384_ASN1,
            _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
        },
        include_bytes!("data/alg-ed25519.der") => match x509_pkey_alg {
            include_bytes!("data/alg-ed25519.der") => &signature::ED25519,
            _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
        },
        e if e.starts_with(&RSASSA_PSS_PREFIX[..]) => {
            let alg = parse_rsa_pss(&e[RSASSA_PSS_PREFIX.len()..])?;
            match x509_pkey_alg {
                include_bytes!("data/alg-rsa-encryption.der") => alg,
                _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
            }
        },
        _ => return Err(Error::UnsupportedSignatureAlgorithm),
    };
    Ok(signature::UnparsedPublicKey::new(
        algorithm,
        x509_pkey_bytes,
    ))
}

// While the RSA-PSS parameters are a ASN.1 SEQUENCE, it is simpler to match
// against the 12 different possibilities. The binary files are *generated* by a
// Go program.
fn parse_rsa_pss(data: &[u8]) -> Result<&'static signature::RsaParameters, Error> {
    match data {
        include_bytes!("data/alg-rsa-pss-sha256-v0.der")
        | include_bytes!("data/alg-rsa-pss-sha256-v1.der")
        | include_bytes!("data/alg-rsa-pss-sha256-v2.der")
        | include_bytes!("data/alg-rsa-pss-sha256-v3.der") =>
            Ok(&signature::RSA_PSS_2048_8192_SHA256),
        include_bytes!("data/alg-rsa-pss-sha384-v0.der")
        | include_bytes!("data/alg-rsa-pss-sha384-v1.der")
        | include_bytes!("data/alg-rsa-pss-sha384-v2.der")
        | include_bytes!("data/alg-rsa-pss-sha384-v3.der") =>
            Ok(&signature::RSA_PSS_2048_8192_SHA384),
        include_bytes!("data/alg-rsa-pss-sha512-v0.der")
        | include_bytes!("data/alg-rsa-pss-sha512-v1.der")
        | include_bytes!("data/alg-rsa-pss-sha512-v2.der")
        | include_bytes!("data/alg-rsa-pss-sha512-v3.der") =>
            Ok(&signature::RSA_PSS_2048_8192_SHA512),
        _ => Err(Error::UnsupportedSignatureAlgorithm),
    }
}
