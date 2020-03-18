use libp2p_core::identity::PublicKey;
use ring::{error::Unspecified, io::der, signature};
use webpki::Error;

pub(super) struct X509Certificate<'a> {
    libp2p_extension: Libp2pExtension<'a>,
    x509_tbs: untrusted::Input<'a>,
    #[allow(dead_code)]
    x509_validity: untrusted::Input<'a>,
    x509_spki: untrusted::Input<'a>,
    x509_signature_algorithm: untrusted::Input<'a>,
    x509_signature: untrusted::Input<'a>,
}

struct Libp2pExtension<'a> {
    peer_key: PublicKey,
    signature: &'a [u8],
}

fn expect_sequence<'a>(reader: &mut untrusted::Reader<'a>) -> Result<untrusted::Input<'a>, Error> {
    der::expect_tag_and_get_value(reader, der::Tag::Sequence).map_err(|Unspecified| Error::BadDER)
}

fn expect_oid<'a>(reader: &mut untrusted::Reader<'a>) -> Result<untrusted::Input<'a>, Error> {
    der::expect_tag_and_get_value(reader, der::Tag::OID).map_err(|Unspecified| Error::BadDER)
}

fn expect_octet_string<'a>(
    reader: &mut untrusted::Reader<'a>,
) -> Result<untrusted::Input<'a>, Error> {
    der::expect_tag_and_get_value(reader, der::Tag::OctetString)
        .map_err(|Unspecified| Error::BadDER)
}

fn parse_libp2p_extension<'a>(
    extension: untrusted::Input<'a>,
) -> Result<Libp2pExtension<'a>, Error> {
    untrusted::Input::read_all(&extension, Unspecified, |reader| {
        der::nested(reader, der::Tag::Sequence, Unspecified, |reader| {
            let public_key = der::bit_string_with_no_unused_bits(reader)?;
            let signature = der::bit_string_with_no_unused_bits(reader)?;
            // We deliberately discard the error information because this is
            // either a broken peer or an attack.
            let peer_key = PublicKey::from_protobuf_encoding(public_key.as_slice_less_safe())
                .map_err(|_| Unspecified)?;
            Ok(Libp2pExtension {
                signature: signature.as_slice_less_safe(),
                peer_key,
            })
        })
    })
    .map_err(|Unspecified| Error::ExtensionValueInvalid)
}

/// Parse X.509 extensions
fn parse_extensions<'a>(reader: &mut untrusted::Reader<'a>) -> Result<Libp2pExtension<'a>, Error> {
    let mut libp2p_extension = None;
    while !reader.at_end() {
        const BOOLEAN: u8 = der::Tag::Boolean as _;
        const OCTET_STRING: u8 = der::Tag::OctetString as _;
        der::nested(reader, der::Tag::Sequence, Error::BadDER, |reader| {
            let oid = expect_oid(reader)?;
            let (tag, data) = der::read_tag_and_get_value(reader).map_err(|_| Error::BadDER)?;
            let (critical, extension) = match tag {
                BOOLEAN => match data.as_slice_less_safe() {
                    [0xFF] => (true, expect_octet_string(reader)?),
                    [0] => (false, expect_octet_string(reader)?),
                    _ => return Err(Error::BadDER),
                },
                OCTET_STRING => (false, data),
                _ => return Err(Error::BadDER),
            };
            match oid.as_slice_less_safe() {
                super::super::LIBP2P_OID_BYTES if libp2p_extension.is_some() => Err(Error::BadDER),
                super::super::LIBP2P_OID_BYTES => {
                    libp2p_extension = Some(parse_libp2p_extension(extension)?);
                    Ok(())
                }
                _ if critical => return Err(Error::UnsupportedCriticalExtension),
                _ => Ok(()),
            }
        })?
    }
    libp2p_extension.ok_or(Error::BadDER)
}

/// Extracts the algorithm id and public key from a certificate
pub(super) fn parse_certificate(certificate: &[u8]) -> Result<X509Certificate<'_>, Error> {
    let ((x509_tbs, inner_tbs), x509_signature_algorithm, x509_signature) =
        untrusted::Input::from(certificate)
            .read_all(Error::BadDER, expect_sequence)?
            .read_all(Error::BadDER, |reader| {
                // tbsCertificate
                let tbs = reader.read_partial(|reader| expect_sequence(reader))?;
                // signatureAlgorithm
                let signature_algorithm = expect_sequence(reader)?;
                // signatureValue
                let signature =
                    der::bit_string_with_no_unused_bits(reader).map_err(|_| Error::BadDER)?;
                Ok((tbs, signature_algorithm, signature))
            })?;
    let (x509_validity, x509_spki, libp2p_extension) =
        inner_tbs.read_all(Error::BadDER, |mut reader| {
            // We require extensions, which means we require version 3
            if reader
                .read_bytes(5)
                .map_err(|_| Error::BadDER)?
                .as_slice_less_safe()
                != [160, 3, 2, 1, 2]
            {
                return Err(Error::UnsupportedCertVersion);
            }
            // serialNumber
            der::positive_integer(&mut reader).map_err(|_| Error::BadDER)?;
            // signature
            if expect_sequence(reader)? != x509_signature_algorithm {
                // signature algorithms don’t match
                return Err(Error::SignatureAlgorithmMismatch);
            }
            // issuer
            expect_sequence(reader)?;
            // validity
            let x509_validity = expect_sequence(reader)?;
            // subject
            expect_sequence(reader)?;
            // subjectPublicKeyInfo
            let spki = expect_sequence(reader)?;
            // subjectUniqueId and issuerUniqueId are unsupported
            // parse the extensions
            let libp2p_extension = der::nested(
                reader,
                der::Tag::ContextSpecificConstructed3,
                Error::BadDER,
                |reader| der::nested(reader, der::Tag::Sequence, Error::BadDER, parse_extensions),
            )?;
            Ok((x509_validity, spki, libp2p_extension))
        })?;
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
    libp2p_extension: Libp2pExtension<'_>,
    x509_pkey_bytes: &[u8],
) -> Result<(), Error> {
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

pub(super) fn verify_certificate(certificate: X509Certificate<'_>) -> Result<(), Error> {
    let X509Certificate {
        x509_tbs,
        x509_spki,
        x509_signature,
        x509_signature_algorithm,
        libp2p_extension,
        ..
    } = certificate;

    let (x509_pkey_alg, x509_pkey_bytes) = x509_spki.read_all(Error::BadDER, |reader| {
        let x509_pkey_alg = expect_sequence(reader)?.as_slice_less_safe();
        let x509_pkey_bytes = der::bit_string_with_no_unused_bits(reader)
            .map_err(|Unspecified| Error::BadDER)?
            .as_slice_less_safe();
        Ok((x509_pkey_alg, x509_pkey_bytes))
    })?;
    let pkey = verify_self_signature(
        x509_pkey_alg,
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
) -> Result<ring::signature::UnparsedPublicKey<&'a [u8]>, Error> {
    const BUF: &[u8] = &[
        0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a,
    ];
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
        e if e.len() > 11 && &e[..11] == BUF => {
            let alg = parse_rsa_pss(&e[11..])?;
            match x509_pkey_alg {
                include_bytes!("data/alg-rsa-encryption.der") => alg,
                _ => return Err(Error::UnsupportedSignatureAlgorithmForPublicKey),
            }
        }
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
