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
use salsa20::{cipher::StreamCipher, XSalsa20};
use std::{fmt, pin::Pin};

/// A writer that encrypts and forwards to an inner writer
#[pin_project]
pub struct CryptWriter<W> {
    #[pin]
    inner: W,
    buf: Vec<u8>,
    cipher: XSalsa20,
}

impl<W: AsyncWrite> CryptWriter<W> {
    /// Creates a new `CryptWriter` with the specified buffer capacity.
    pub fn with_capacity(capacity: usize, inner: W, cipher: XSalsa20) -> CryptWriter<W> {
        CryptWriter {
            inner,
            buf: Vec::with_capacity(capacity),
            cipher,
        }
    }

    /// Gets a pinned mutable reference to the inner writer.
    ///
    /// It is inadvisable to directly write to the inner writer.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut W> {
        self.project().inner
    }
}

/// Write the contents of a [`Vec<u8>`] into an [`AsyncWrite`].
///
/// The handling 0 byte progress and the Interrupted error was taken from BufWriter in async_std.
///
/// If this fn returns Ready(Ok(())), the buffer has been completely flushed and is empty.
fn poll_flush_buf<W: AsyncWrite>(
    inner: &mut Pin<&mut W>,
    buf: &mut Vec<u8>,
    cx: &mut Context<'_>,
) -> Poll<io::Result<()>> {
    let mut ret = Poll::Ready(Ok(()));
    let mut written = 0;
    let len = buf.len();
    while written < len {
        match inner.as_mut().poll_write(cx, &buf[written..]) {
            Poll::Ready(Ok(n)) => {
                if n > 0 {
                    // we made progress, so try again
                    written += n;
                } else {
                    // we got Ok but got no progress whatsoever, so bail out so we don't spin writing 0 bytes.
                    ret = Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "Failed to write buffered data",
                    )));
                    break;
                }
            }
            Poll::Ready(Err(e)) => {
                // Interrupted is the only error that we consider to be recoverable by trying again
                if e.kind() != io::ErrorKind::Interrupted {
                    // for any other error, don't try again
                    ret = Poll::Ready(Err(e));
                    break;
                }
            }
            Poll::Pending => {
                ret = Poll::Pending;
                break;
            }
        }
    }
    if written > 0 {
        buf.drain(..written);
    }
    if let Poll::Ready(Ok(())) = ret {
        debug_assert!(buf.is_empty());
    }
    ret
}

impl<W: AsyncWrite> AsyncWrite for CryptWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut this = self.project();
        // completely flush the buffer, returning pending if not possible
        ready!(poll_flush_buf(&mut this.inner, this.buf, cx))?;
        // if we get here, the buffer is empty
        debug_assert!(this.buf.is_empty());
        let res = Pin::new(&mut *this.buf).poll_write(cx, buf);
        if let Poll::Ready(Ok(count)) = res {
            this.cipher.apply_keystream(&mut this.buf[0..count]);
            trace!("encrypted {} bytes", count);
        } else {
            debug_assert!(false);
        };
        // flush immediately afterwards, but if we get a pending we don't care
        if let Poll::Ready(Err(e)) = poll_flush_buf(&mut this.inner, this.buf, cx) {
            Poll::Ready(Err(e))
        } else {
            res
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut this = self.project();
        ready!(poll_flush_buf(&mut this.inner, this.buf, cx))?;
        this.inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut this = self.project();
        ready!(poll_flush_buf(&mut this.inner, this.buf, cx))?;
        this.inner.poll_close(cx)
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
