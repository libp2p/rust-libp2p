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

//! Implementation of the [pnet](https://github.com/libp2p/specs/blob/master/pnet/Private-Networks-PSK-V1.md) protocol.
//!
//| The `pnet` protocol implements *Pre-shared Key Based Private Networks in libp2p*.
//! Libp2p nodes configured with a pre-shared key can only communicate with other nodes with
//! the same key.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod crypt_writer;
use crypt_writer::CryptWriter;
use futures::prelude::*;
use log::trace;
use pin_project::pin_project;
use rand::RngCore;
use salsa20::{
    cipher::{KeyIvInit, StreamCipher},
    Salsa20, XSalsa20,
};
use sha3::{digest::ExtendableOutput, Shake128};
use std::{
    error,
    fmt::{self, Write},
    io,
    io::Error as IoError,
    num::ParseIntError,
    pin::Pin,
    str::FromStr,
    task::{Context, Poll},
};

const KEY_SIZE: usize = 32;
const NONCE_SIZE: usize = 24;
const WRITE_BUFFER_SIZE: usize = 1024;
const FINGERPRINT_SIZE: usize = 16;

/// A pre-shared key, consisting of 32 bytes of random data.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct PreSharedKey([u8; KEY_SIZE]);

impl PreSharedKey {
    /// Create a new pre shared key from raw bytes
    pub fn new(data: [u8; KEY_SIZE]) -> Self {
        Self(data)
    }

    /// Compute PreSharedKey fingerprint identical to the go-libp2p fingerprint.
    /// The computation of the fingerprint is not specified in the spec.
    ///
    /// This provides a way to check that private keys are properly configured
    /// without dumping the key itself to the console.
    pub fn fingerprint(&self) -> Fingerprint {
        use std::io::{Read, Write};
        let mut enc = [0u8; 64];
        let nonce: [u8; 8] = *b"finprint";
        let mut out = [0u8; 16];
        let mut cipher = Salsa20::new(&self.0.into(), &nonce.into());
        cipher.apply_keystream(&mut enc);
        let mut hasher = Shake128::default();
        hasher.write_all(&enc).expect("shake128 failed");
        hasher
            .finalize_xof()
            .read_exact(&mut out)
            .expect("shake128 failed");
        Fingerprint(out)
    }
}

fn parse_hex_key(s: &str) -> Result<[u8; KEY_SIZE], KeyParseError> {
    if s.len() == KEY_SIZE * 2 {
        let mut r = [0u8; KEY_SIZE];
        for i in 0..KEY_SIZE {
            r[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(KeyParseError::InvalidKeyChar)?;
        }
        Ok(r)
    } else {
        Err(KeyParseError::InvalidKeyLength)
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        write!(hex, "{byte:02x}").expect("Can't fail on writing to string");
    }

    hex
}

/// Parses a PreSharedKey from a key file
///
/// currently supports only base16 encoding.
impl FromStr for PreSharedKey {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let [keytype, encoding, key] = *s.lines().take(3).collect::<Vec<_>>().as_slice() {
            if keytype != "/key/swarm/psk/1.0.0/" {
                return Err(KeyParseError::InvalidKeyType);
            }
            if encoding != "/base16/" {
                return Err(KeyParseError::InvalidKeyEncoding);
            }
            parse_hex_key(key.trim_end()).map(PreSharedKey)
        } else {
            Err(KeyParseError::InvalidKeyFile)
        }
    }
}

impl fmt::Debug for PreSharedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PreSharedKey")
            .field(&to_hex(&self.0))
            .finish()
    }
}

/// Dumps a PreSharedKey in key file format compatible with go-libp2p
impl fmt::Display for PreSharedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "/key/swarm/psk/1.0.0/")?;
        writeln!(f, "/base16/")?;
        writeln!(f, "{}", to_hex(&self.0))
    }
}

/// A PreSharedKey fingerprint computed from a PreSharedKey
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Fingerprint([u8; FINGERPRINT_SIZE]);

/// Dumps the fingerprint as hex
impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}

/// Error when parsing a PreSharedKey
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyParseError {
    /// file does not have the expected structure
    InvalidKeyFile,
    /// unsupported key type
    InvalidKeyType,
    /// unsupported key encoding. Currently only base16 is supported
    InvalidKeyEncoding,
    /// Key is of the wrong length
    InvalidKeyLength,
    /// key string contains a char that is not consistent with the specified encoding
    InvalidKeyChar(ParseIntError),
}

impl fmt::Display for KeyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl error::Error for KeyParseError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            KeyParseError::InvalidKeyChar(ref err) => Some(err),
            _ => None,
        }
    }
}

/// Private network configuration
#[derive(Debug, Copy, Clone)]
pub struct PnetConfig {
    /// the PreSharedKey to use for encryption
    key: PreSharedKey,
}
impl PnetConfig {
    pub fn new(key: PreSharedKey) -> Self {
        Self { key }
    }

    /// upgrade a connection to use pre shared key encryption.
    ///
    /// the upgrade works by both sides exchanging 24 byte nonces and then encrypting
    /// subsequent traffic with XSalsa20
    pub async fn handshake<TSocket>(
        self,
        mut socket: TSocket,
    ) -> Result<PnetOutput<TSocket>, PnetError>
    where
        TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        trace!("exchanging nonces");
        let mut local_nonce = [0u8; NONCE_SIZE];
        let mut remote_nonce = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut local_nonce);
        socket
            .write_all(&local_nonce)
            .await
            .map_err(PnetError::HandshakeError)?;
        socket.flush().await?;
        socket
            .read_exact(&mut remote_nonce)
            .await
            .map_err(PnetError::HandshakeError)?;
        trace!("setting up ciphers");
        let write_cipher = XSalsa20::new(&self.key.0.into(), &local_nonce.into());
        let read_cipher = XSalsa20::new(&self.key.0.into(), &remote_nonce.into());
        Ok(PnetOutput::new(socket, write_cipher, read_cipher))
    }
}

/// The result of a handshake. This implements AsyncRead and AsyncWrite and can therefore
/// be used as base for additional upgrades.
#[pin_project]
pub struct PnetOutput<S> {
    #[pin]
    inner: CryptWriter<S>,
    read_cipher: XSalsa20,
}

impl<S: AsyncRead + AsyncWrite> PnetOutput<S> {
    fn new(inner: S, write_cipher: XSalsa20, read_cipher: XSalsa20) -> Self {
        Self {
            inner: CryptWriter::with_capacity(WRITE_BUFFER_SIZE, inner, write_cipher),
            read_cipher,
        }
    }
}

impl<S: AsyncRead + AsyncWrite> AsyncRead for PnetOutput<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.project();
        let result = this.inner.get_pin_mut().poll_read(cx, buf);
        if let Poll::Ready(Ok(size)) = &result {
            trace!("read {} bytes", size);
            this.read_cipher.apply_keystream(&mut buf[..*size]);
            trace!("decrypted {} bytes", size);
        }
        result
    }
}

impl<S: AsyncRead + AsyncWrite> AsyncWrite for PnetOutput<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.project().inner.poll_close(cx)
    }
}

/// Error when writing or reading private swarms
#[derive(Debug)]
pub enum PnetError {
    /// Error during handshake.
    HandshakeError(IoError),
    /// I/O error.
    IoError(IoError),
}

impl From<IoError> for PnetError {
    #[inline]
    fn from(err: IoError) -> PnetError {
        PnetError::IoError(err)
    }
}

impl error::Error for PnetError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            PnetError::HandshakeError(ref err) => Some(err),
            PnetError::IoError(ref err) => Some(err),
        }
    }
}

impl fmt::Display for PnetError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            PnetError::HandshakeError(e) => write!(f, "Handshake error: {e}"),
            PnetError::IoError(e) => write!(f, "I/O error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;

    impl Arbitrary for PreSharedKey {
        fn arbitrary(g: &mut Gen) -> PreSharedKey {
            let key = core::array::from_fn(|_| u8::arbitrary(g));
            PreSharedKey(key)
        }
    }

    #[test]
    fn psk_tostring_parse() {
        fn prop(key: PreSharedKey) -> bool {
            let text = key.to_string();
            text.parse::<PreSharedKey>()
                .map(|res| res == key)
                .unwrap_or(false)
        }
        QuickCheck::new()
            .tests(10)
            .quickcheck(prop as fn(PreSharedKey) -> _);
    }

    #[test]
    fn psk_parse_failure() {
        use KeyParseError::*;
        assert_eq!("".parse::<PreSharedKey>().unwrap_err(), InvalidKeyFile);
        assert_eq!(
            "a\nb\nc".parse::<PreSharedKey>().unwrap_err(),
            InvalidKeyType
        );
        assert_eq!(
            "/key/swarm/psk/1.0.0/\nx\ny"
                .parse::<PreSharedKey>()
                .unwrap_err(),
            InvalidKeyEncoding
        );
        assert_eq!(
            "/key/swarm/psk/1.0.0/\n/base16/\ny"
                .parse::<PreSharedKey>()
                .unwrap_err(),
            InvalidKeyLength
        );
    }

    #[test]
    fn fingerprint() {
        // checked against go-ipfs output
        let key = "/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683".parse::<PreSharedKey>().unwrap();
        let expected = "45fc986bbc9388a11d939df26f730f0c";
        let actual = key.fingerprint().to_string();
        assert_eq!(expected, actual);
    }
}
