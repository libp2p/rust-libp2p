// Copyright 2019 Parity Technologies (UK) Ltd.
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

mod crypt_writer;
use futures::prelude::*;
use pin_project::pin_project;
use log::trace;
use salsa20::{
    stream_cipher::{NewStreamCipher, SyncStreamCipher},
    Salsa20, XSalsa20,
};
use std::{
    io,
    error,
    num::ParseIntError,
    str::FromStr,
    pin::Pin,
    io::Error as IoError,
    task::{Context, Poll},
    fmt::{self, Write},
};
use rand::RngCore;
use sha3::{digest::ExtendableOutput, Shake128};
use crypt_writer::CryptWriter;

const KEY_SIZE: usize = 32;
const NONCE_SIZE: usize = 24;
const WRITE_BUFFER_SIZE: usize = 1024;
const FINGERPRINT_SIZE: usize = 16;

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct PreSharedKey([u8; KEY_SIZE]);

impl PreSharedKey {
    /// compute PreSharedKey fingerprint similar to how it is done in go-ipfs
    pub fn fingerprint(&self) -> Fingerprint {
        use std::io::{Read, Write};
        let mut enc = [0u8; 64];
        let nonce: [u8; 8] = *b"finprint";
        let mut out = [0u8; 16];
        let mut cipher = Salsa20::new(&self.0.into(), &nonce.into());
        cipher.apply_keystream(&mut enc);
        let mut hasher = Shake128::default();
        hasher.write_all(&enc).expect("shake128 failed");
        hasher.xof_result().read(&mut out).expect("shake128 failed");
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
        write!(hex, "{:02x}", byte).expect("Can't fail on writing to string");
    }

    hex
}

impl FromStr for PreSharedKey {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let &[keytype, encoding, key] = s.lines().take(3).collect::<Vec<_>>().as_slice() {
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
        f.debug_tuple("PreSharedKey").field(&to_hex(&self.0)).finish()
    }
}

impl fmt::Display for PreSharedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "/key/swarm/psk/1.0.0/")?;
        writeln!(f, "/base16/")?;
        writeln!(f, "{}", to_hex(&self.0))
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Fingerprint([u8; FINGERPRINT_SIZE]);

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", to_hex(&self.0))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyParseError {
    InvalidKeyFile,
    InvalidKeyType,
    InvalidKeyEncoding,
    InvalidKeyLength,
    InvalidKeyChar(ParseIntError),
}

#[derive(Debug, Copy, Clone)]
pub struct PnetConfig {
    key: PreSharedKey,
}
impl PnetConfig {
    pub fn new(key: PreSharedKey) -> Self {
        Self { key }
    }

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

#[pin_project]
pub struct PnetOutput<S> {
    inner: CryptWriter<S>,
    read_cipher: XSalsa20,
}

impl<S: AsyncRead + AsyncWrite + Unpin> PnetOutput<S> {
    fn new(inner: S, write_cipher: XSalsa20, read_cipher: XSalsa20) -> Self {
        Self {
            inner: CryptWriter::with_capacity(WRITE_BUFFER_SIZE, inner, write_cipher),
            read_cipher,
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for PnetOutput<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let result = Pin::new(&mut self.as_mut().inner.get_mut()).poll_read(cx, buf);
        if let Poll::Ready(Ok(size)) = &result {
            trace!("read {} bytes", size);
            self.read_cipher.apply_keystream(&mut buf[..*size]);
            trace!("decrypted {} bytes", size);
        }
        result
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for PnetOutput<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.as_mut().inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.as_mut().inner).poll_close(cx)
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
            PnetError::HandshakeError(e) => write!(f, "Handshake error: {}", e),
            PnetError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;

    impl Arbitrary for PreSharedKey {
        fn arbitrary<G: Gen>(g: &mut G) -> PreSharedKey {
            let mut key = [0; KEY_SIZE];
            g.fill_bytes(&mut key);
            PreSharedKey(key)
        }
    }

    #[test]
    fn psk_tostring_parse() {
        fn prop(key: PreSharedKey) -> bool {
            let text = key.to_string();
            text.parse::<PreSharedKey>().map(|res| res == key).unwrap_or(false)
        }
        QuickCheck::new().tests(10).quickcheck(prop as fn(PreSharedKey) -> _);
    }

    #[test]
    fn psk_parse_failure() {
        use KeyParseError::*;
        assert_eq!("".parse::<PreSharedKey>().unwrap_err(), InvalidKeyFile);
        assert_eq!("a\nb\nc".parse::<PreSharedKey>().unwrap_err(), InvalidKeyType);
        assert_eq!(
            "/key/swarm/psk/1.0.0/\nx\ny".parse::<PreSharedKey>().unwrap_err(),
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
