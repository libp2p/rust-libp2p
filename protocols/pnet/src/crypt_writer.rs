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

use futures::{
    io::{self, AsyncWrite},
    ready,
    task::{Context, Poll},
};
use log::trace;
use pin_project::pin_project;
use salsa20::{stream_cipher::SyncStreamCipher, XSalsa20};
use std::{fmt, pin::Pin};

/// A writer that encrypts and forwards to an inner writer
#[pin_project]
pub struct CryptWriter<W> {
    #[pin]
    inner: W,
    buf: Vec<u8>,
    written: usize,
    cipher: XSalsa20,
}

impl<W: AsyncWrite> CryptWriter<W> {
    /// Creates a new `CryptWriter` with the specified buffer capacity.
    pub fn with_capacity(capacity: usize, inner: W, cipher: XSalsa20) -> CryptWriter<W> {
        CryptWriter {
            inner,
            buf: Vec::with_capacity(capacity),
            written: 0,
            cipher,
        }
    }

    /// Gets a mutable reference to the inner writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Gets a pinned mutable reference to the inner writer.
    ///
    /// It is inadvisable to directly write to the inner writer.
    fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut W> {
        self.project().inner
    }

    /// Poll buffer flushing until completion
    ///
    /// Copied from async_std BufWriter
    fn poll_flush_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut this = self.project();
        let len = this.buf.len();
        let mut ret = Ok(());
        while *this.written < len {
            match this
                .inner
                .as_mut()
                .poll_write(cx, &this.buf[*this.written..])
            {
                Poll::Ready(Ok(0)) => {
                    ret = Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "Failed to write buffered data",
                    ));
                    break;
                }
                Poll::Ready(Ok(n)) => *this.written += n,
                Poll::Ready(Err(ref e)) if e.kind() == io::ErrorKind::Interrupted => {}
                Poll::Ready(Err(e)) => {
                    ret = Err(e);
                    break;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        if *this.written > 0 {
            this.buf.drain(..*this.written);
        }
        *this.written = 0;
        Poll::Ready(ret)
    }
}

impl<W: AsyncWrite> AsyncWrite for CryptWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // completely flush the buffer, returning pending if not possible
        ready!(self.as_mut().poll_flush_buf(cx))?;
        let this = self.project();
        let res = Pin::new(&mut *this.buf).poll_write(cx, buf);
        if let Poll::Ready(Ok(count)) = res {
            this.cipher.apply_keystream(&mut this.buf[0..count]);
            trace!("encrypted {} bytes", count);
        };
        res
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush_buf(cx))?;
        self.get_pin_mut().poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush_buf(cx))?;
        self.get_pin_mut().poll_close(cx)
    }
}

impl<W: AsyncWrite + fmt::Debug> fmt::Debug for CryptWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CryptWriter")
            .field("writer", &self.inner)
            .field("buf", &self.buf)
            .finish()
    }
}
