use std::io;
use std::io::{Cursor, Read, Write};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use time::{Duration, OffsetDateTime};
use libp2p_core::multihash::Multihash;
use libp2p_tls::certificate;
use sha2::Digest;

const MULTIHASH_SHA256_CODE: u64 = 0x12;
const CERT_VALID_PERIOD: Duration = Duration::days(14);

pub(crate) fn alpn_protocols() -> Vec<Vec<u8>> {
    vec![b"libp2p".to_vec(),
         b"h3".to_vec(),
         b"h3-32".to_vec(),
         b"h3-31".to_vec(),
         b"h3-30".to_vec(),
         b"h3-29".to_vec(), ]
}


/*
I would like to avoid interacting with the file system as much as possible.
My suggestion would be:
- libp2p::webtransport::Transport::new takes a list of certificates (of type libp2p::webtransport::Certificate)
- libp2p::webtransport::Certificate::generate allows users generate a new certificate with certain parameters (validity date etc)
- libp2p::webtransport::Certificate::{parse,to_bytes} allow users to serialize and deserialize certificates
*/
#[derive(Debug, PartialEq, Eq)]
pub struct Certificate {
    pub cert: CertificateDer<'static>,
    pub private_key: PrivateKeyDer<'static>,
    pub not_before: OffsetDateTime,
    pub not_after: OffsetDateTime,
}

#[derive(Debug)]
pub enum Error {
    GenError(certificate::GenError),
    IoError(io::Error),
}

impl From<certificate::GenError> for Error {
    fn from(value: certificate::GenError) -> Self {
        Self::GenError(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::IoError(value)
    }
}

impl Clone for Certificate {
    fn clone(&self) -> Self {
        Self {
            cert: self.cert.clone(),
            private_key: self.private_key.clone_key(),
            not_before: self.not_before.clone(),
            not_after: self.not_after.clone(),
        }
    }
}

impl Certificate {
    pub fn generate(
        identity_keypair: &libp2p_identity::Keypair,
        not_before: OffsetDateTime,
    ) -> Result<Self, Error> {
        let not_after = not_before.clone()
            .checked_add(CERT_VALID_PERIOD)
            .expect("Addition does not overflow");
        let (cert, private_key) = certificate::generate_with_validity_period(
            identity_keypair,
            not_before.clone(),
            not_after.clone(),
        )?;

        Ok(Self { cert, private_key, not_before, not_after })
    }

    pub(crate) fn cert_hash(&self) -> Multihash<64> {
        Multihash::wrap(
            MULTIHASH_SHA256_CODE, sha2::Sha256::digest(&self.cert.as_ref().as_ref()).as_ref(),
        ).expect("fingerprint's len to be 32 bytes")
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        Self::write_data(&mut bytes, self.cert.as_ref())
            .expect("Write cert data");
        Self::write_data(&mut bytes, self.private_key.secret_der())
            .expect("Write private_key data");

        let nb_buff = self.not_before.unix_timestamp().to_be_bytes();
        std::io::Write::write(&mut bytes, &nb_buff)
            .expect("Write not_before");

        let na_buff = self.not_after.unix_timestamp().to_be_bytes();
        std::io::Write::write(&mut bytes, &na_buff)
            .expect("Write not_after");

        bytes
    }

    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut cursor = Cursor::new(data);
        let cert_data = Self::read_data(&mut cursor)?;
        let private_key_data = Self::read_data(&mut cursor)?;
        let nb = Self::read_i64(&mut cursor).unwrap();
        let na = Self::read_i64(&mut cursor).unwrap();

        let cert = CertificateDer::from(cert_data);
        let private_key = PrivateKeyDer::try_from(private_key_data).unwrap();
        let not_before = OffsetDateTime::from_unix_timestamp(nb).unwrap();
        let not_after = OffsetDateTime::from_unix_timestamp(na).unwrap();

        Ok(Self { cert, private_key, not_before, not_after })
    }

    fn write_data<W: Write>(w: &mut W, data: &[u8]) -> Result<(), io::Error> {
        let size = data.len() as u64;
        let mut size_buf = size.to_be_bytes();

        w.write_all(&mut size_buf)?;
        w.write_all(data)?;

        Ok(())
    }

    fn read_data<R: Read>(r: &mut R) -> Result<Vec<u8>, io::Error> {
        let size = Self::read_i64(r)? as usize;
        let mut res = vec![0u8; size];

        r.read(res.as_mut_slice())?;

        Ok(res)
    }

    fn read_i64<R: Read>(r: &mut R) -> Result<i64, io::Error> {
        let mut buffer = [0u8; 8];
        r.read(&mut buffer)?;

        Ok(i64::from_be_bytes(buffer))
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    #[test]
    fn test_certificate_parsing() {
        let keypair = libp2p_identity::Keypair::generate_ed25519();
        let not_before = datetime!(2025-08-08 0:00 UTC);
        let cert = super::Certificate::generate(&keypair, not_before).unwrap();

        let binary_data = cert.to_bytes();
        let actual = super::Certificate::parse(binary_data.as_slice())
            .unwrap();

        assert_eq!(actual, cert);
    }
}