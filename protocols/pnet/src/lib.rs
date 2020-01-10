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
use futures::future::BoxFuture;
use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::prelude::*;
use libp2p_core::{upgrade::Negotiated, InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use salsa20::stream_cipher::{NewStreamCipher, SyncStreamCipher, SyncStreamCipherSeek};
use salsa20::XSalsa20;
use std::{
    io, iter,
    pin::Pin,
    task::{Context, Poll},
};

use rand::RngCore;
use std::error;
use std::fmt;
use std::io::Error as IoError;

const KEY_LENGTH: usize = 32;
const NONCE_LENGTH: usize = 24;

#[derive(Debug, Copy, Clone)]
pub struct PnetConfig {
    key: [u8; KEY_LENGTH],
}
impl PnetConfig {
    pub fn default() -> Self {
        Self {
            key: *b"01234567890123456789012345678901",
        }
    }

    async fn handshake<TSocket>(self, mut socket: TSocket) -> Result<PnetOutput<TSocket>, PnetError>
    where
        TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let mut local_nonce = [0u8; NONCE_LENGTH];
        let mut remote_nonce = [0u8; NONCE_LENGTH];
        rand::thread_rng().fill_bytes(&mut local_nonce);
        socket.write_all(&local_nonce).await?;
        socket.read_exact(&mut remote_nonce).await?;
        let write_cipher = XSalsa20::new(&self.key.into(), &local_nonce.into());
        let read_cipher = XSalsa20::new(&self.key.into(), &remote_nonce.into());
        Ok(PnetOutput::new(socket, write_cipher, read_cipher))
    }
}

impl UpgradeInfo for PnetConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/pnet/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for PnetConfig
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = PnetOutput<Negotiated<C>>;
    type Error = PnetError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, i: Negotiated<C>, _: Self::Info) -> Self::Future {
        Box::pin(self.handshake(i))
    }
}

impl<C> OutboundUpgrade<C> for PnetConfig
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = PnetOutput<Negotiated<C>>;
    type Error = PnetError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, i: Negotiated<C>, _: Self::Info) -> Self::Future {
        Box::pin(self.handshake(i))
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
        self.write_cipher.seek(self.write_offset);
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
            PnetError::IoError(ref err) => Some(err),
        }
    }
}

impl fmt::Display for PnetError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            PnetError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}
