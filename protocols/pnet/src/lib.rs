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
use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::prelude::*;
use salsa20::stream_cipher::{NewStreamCipher, SyncStreamCipher, SyncStreamCipherSeek};
use salsa20::XSalsa20;
use std::num::ParseIntError;
use std::str::FromStr;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use rand::RngCore;
use std::error;
use std::fmt::{self, Write};
use std::io::Error as IoError;

const KEY_LENGTH: usize = 32;
const NONCE_LENGTH: usize = 24;

#[derive(Copy, Clone)]
pub struct PSK([u8; KEY_LENGTH]);

fn parse_hex_key(s: &str) -> Result<[u8; KEY_LENGTH], KeyParseError> {
    if s.len() == KEY_LENGTH * 2 {
        let mut r = [0u8; KEY_LENGTH];
        for i in 0..KEY_LENGTH {
            r[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(KeyParseError::InvalidKeyChar)?;
        }
        Ok(r)
    } else {
        Err(KeyParseError::InvalidKeyLength)
    }
}

/// Convert bytes to a hex representation
fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        write!(hex, "{:02x}", byte).expect("Can't fail on writing to string");
    }

    hex
}

impl FromStr for PSK {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut lines = s.lines();
        let keytype = lines.next().ok_or(KeyParseError::InvalidKeyFile)?;
        let encoding = lines.next().ok_or(KeyParseError::InvalidKeyFile)?;
        let key = lines.next().ok_or(KeyParseError::InvalidKeyFile)?;
        if keytype != "/key/swarm/psk/1.0.0/" {
            return Err(KeyParseError::InvalidKeyType);
        }
        if encoding != "/base16/" {
            return Err(KeyParseError::InvalidKeyEncoding);
        }
        let key = parse_hex_key(key.trim_end())?;
        Ok(PSK(key))
    }
}

impl fmt::Debug for PSK {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PSK").field(&to_hex(&self.0)).finish()
    }
}

impl fmt::Display for PSK {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "/key/swarm/psk/1.0.0/")?;
        writeln!(f, "/base16/")?;
        writeln!(f, "{}", to_hex(&self.0))
    }
}

#[derive(Clone, Debug)]
pub enum KeyParseError {
    InvalidKeyFile,
    InvalidKeyType,
    InvalidKeyEncoding,
    InvalidKeyLength,
    InvalidKeyChar(ParseIntError),
}

#[derive(Debug, Copy, Clone)]
pub struct PnetConfig {
    key: PSK,
}
impl PnetConfig {
    pub fn new(key: PSK) -> Self {
        Self { key }
    }
    pub fn default() -> Self {
        let key = PSK::from_str("/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683").unwrap();
        Self { key }
    }

    pub async fn handshake<TSocket>(
        self,
        mut socket: TSocket,
    ) -> Result<PnetOutput<TSocket>, PnetError>
    where
        TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let mut local_nonce = [0u8; NONCE_LENGTH];
        let mut remote_nonce = [0u8; NONCE_LENGTH];
        rand::thread_rng().fill_bytes(&mut local_nonce);
        socket.write_all(&local_nonce).await.map_err(PnetError::HandshakeError)?;
        socket.read_exact(&mut remote_nonce).await.map_err(PnetError::HandshakeError)?;
        let write_cipher = XSalsa20::new(&self.key.0.into(), &local_nonce.into());
        let read_cipher = XSalsa20::new(&self.key.0.into(), &remote_nonce.into());
        Ok(PnetOutput::new(socket, write_cipher, read_cipher))
    }
}

pub struct PnetOutput<S> {
    inner: S,
    read_cipher: XSalsa20,
    write_cipher: XSalsa20,
    write_offset: u64,
    write_buffer: [u8; 1024],
}

impl<S: AsyncRead + AsyncWrite + Unpin> PnetOutput<S> {
    fn new(inner: S, write_cipher: XSalsa20, read_cipher: XSalsa20) -> Self {
        Self {
            inner,
            write_cipher,
            read_cipher,
            write_offset: 0,
            write_buffer: [0; 1024],
        }
    }

    fn encrypt_and_write(
        &mut self,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let size = std::cmp::min(buf.len(), self.write_buffer.len());
        let write_buffer = &mut self.write_buffer[0..size];
        write_buffer.copy_from_slice(&buf[..size]);
        if self.write_cipher.current_pos() != self.write_offset {
            self.write_cipher.seek(self.write_offset);
        }
        self.write_cipher.apply_keystream(write_buffer);
        println!("sending");
        println!("p: {:02x?}", &buf[..size]);
        println!("c: {:02x?}", &write_buffer);
        Pin::new(&mut self.inner).poll_write(cx, &self.write_buffer[..size])
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for PnetOutput<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let result = Pin::new(&mut self.as_mut().inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(size)) = &result {
            println!("receiving");
            println!("c: {:02x?}", &buf[..*size]);
            self.read_cipher.apply_keystream(&mut buf[..*size]);
            println!("p: {:02x?}", &buf[..*size]);
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
        let result = self.as_mut().encrypt_and_write(cx, buf);
        if let Poll::Ready(Ok(size)) = &result {
            self.write_offset += *size as u64;
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.as_mut().inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.as_mut().inner).poll_close(cx)
    }
}

/// Error at the SECIO layer communication.
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
            PnetError::IoError(e) => write!(f, "I/O error: {}", e),
            PnetError::HandshakeError(e) => write!(f, "Handshake error: {}", e),
        }
    }
}
