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

use futures::{prelude::*, ready};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use std::{io, iter, pin::Pin, task::Context, task::Poll};

#[derive(Debug, Copy, Clone)]
pub struct DeflateConfig {
    compression: flate2::Compression,
}

impl Default for DeflateConfig {
    fn default() -> Self {
        DeflateConfig {
            compression: flate2::Compression::fast(),
        }
    }
}

impl UpgradeInfo for DeflateConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/deflate/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for DeflateConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = DeflateOutput<C>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, r: C, _: Self::Info) -> Self::Future {
        future::ok(DeflateOutput::new(r, self.compression))
    }
}

impl<C> OutboundUpgrade<C> for DeflateConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = DeflateOutput<C>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, w: C, _: Self::Info) -> Self::Future {
        future::ok(DeflateOutput::new(w, self.compression))
    }
}

/// Decodes and encodes traffic using DEFLATE.
#[derive(Debug)]
pub struct DeflateOutput<S> {
    /// Inner stream where we read compressed data from and write compressed data to.
    inner: S,
    /// Internal object used to hold the state of the compression.
    compress: flate2::Compress,
    /// Internal object used to hold the state of the decompression.
    decompress: flate2::Decompress,
    /// Temporary buffer between `compress` and `inner`. Stores compressed bytes that need to be
    /// sent out once `inner` is ready to accept more.
    write_out: Vec<u8>,
    /// Temporary buffer between `decompress` and `inner`. Stores compressed bytes that need to be
    /// given to `decompress`.
    read_interm: Vec<u8>,
    /// When we read from `inner` and `Ok(0)` is returned, we set this to `true` so that we don't
    /// read from it again.
    inner_read_eof: bool,
}

impl<S> DeflateOutput<S> {
    fn new(inner: S, compression: flate2::Compression) -> Self {
        DeflateOutput {
            inner,
            compress: flate2::Compress::new(compression, false),
            decompress: flate2::Decompress::new(false),
            write_out: Vec::with_capacity(256),
            read_interm: Vec::with_capacity(256),
            inner_read_eof: false,
        }
    }

    /// Tries to write the content of `self.write_out` to `self.inner`.
    /// Returns `Ready(Ok(()))` if `self.write_out` is empty.
    fn flush_write_out(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>>
    where
        S: AsyncWrite + Unpin,
    {
        loop {
            if self.write_out.is_empty() {
                return Poll::Ready(Ok(()));
            }

            match AsyncWrite::poll_write(Pin::new(&mut self.inner), cx, &self.write_out) {
                Poll::Ready(Ok(0)) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
                Poll::Ready(Ok(n)) => self.write_out = self.write_out.split_off(n),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            };
        }
    }
}

impl<S> AsyncRead for DeflateOutput<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        // We use a `this` variable because the compiler doesn't allow multiple mutable borrows
        // across a `Deref`.
        let this = &mut *self;

        loop {
            // Read from `self.inner` into `self.read_interm` if necessary.
            if this.read_interm.is_empty() && !this.inner_read_eof {
                this.read_interm
                    .resize(this.read_interm.capacity() + 256, 0);

                match AsyncRead::poll_read(Pin::new(&mut this.inner), cx, &mut this.read_interm) {
                    Poll::Ready(Ok(0)) => {
                        this.inner_read_eof = true;
                        this.read_interm.clear();
                    }
                    Poll::Ready(Ok(n)) => this.read_interm.truncate(n),
                    Poll::Ready(Err(err)) => {
                        this.read_interm.clear();
                        return Poll::Ready(Err(err));
                    }
                    Poll::Pending => {
                        this.read_interm.clear();
                        return Poll::Pending;
                    }
                }
            }
            debug_assert!(!this.read_interm.is_empty() || this.inner_read_eof);

            let before_out = this.decompress.total_out();
            let before_in = this.decompress.total_in();
            let ret = this.decompress.decompress(
                &this.read_interm,
                buf,
                if this.inner_read_eof {
                    flate2::FlushDecompress::Finish
                } else {
                    flate2::FlushDecompress::None
                },
            )?;

            // Remove from `self.read_interm` the bytes consumed by the decompressor.
            let consumed = (this.decompress.total_in() - before_in) as usize;
            this.read_interm = this.read_interm.split_off(consumed);

            let read = (this.decompress.total_out() - before_out) as usize;
            if read != 0 || ret == flate2::Status::StreamEnd {
                return Poll::Ready(Ok(read));
            }
        }
    }
}

impl<S> AsyncWrite for DeflateOutput<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        // We use a `this` variable because the compiler doesn't allow multiple mutable borrows
        // across a `Deref`.
        let this = &mut *self;

        // We don't want to accumulate too much data in `self.write_out`, so we only proceed if it
        // is empty.
        ready!(this.flush_write_out(cx))?;

        // We special-case this, otherwise an empty buffer would make the loop below infinite.
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        // Unfortunately, the compressor might be in a "flushing mode", not accepting any input
        // data. We don't want to return `Ok(0)` in that situation, as that would be wrong.
        // Instead, we invoke the compressor in a loop until it accepts some of our data.
        loop {
            let before_in = this.compress.total_in();
            this.write_out.reserve(256); // compress_vec uses the Vec's capacity
            let ret = this.compress.compress_vec(
                buf,
                &mut this.write_out,
                flate2::FlushCompress::None,
            )?;
            let written = (this.compress.total_in() - before_in) as usize;

            if written != 0 || ret == flate2::Status::StreamEnd {
                return Poll::Ready(Ok(written));
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        // We use a `this` variable because the compiler doesn't allow multiple mutable borrows
        // across a `Deref`.
        let this = &mut *self;

        ready!(this.flush_write_out(cx))?;
        this.compress
            .compress_vec(&[], &mut this.write_out, flate2::FlushCompress::Sync)?;

        loop {
            ready!(this.flush_write_out(cx))?;

            debug_assert!(this.write_out.is_empty());
            // We ask the compressor to flush everything into `self.write_out`.
            this.write_out.reserve(256); // compress_vec uses the Vec's capacity
            this.compress
                .compress_vec(&[], &mut this.write_out, flate2::FlushCompress::None)?;
            if this.write_out.is_empty() {
                break;
            }
        }

        AsyncWrite::poll_flush(Pin::new(&mut this.inner), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        // We use a `this` variable because the compiler doesn't allow multiple mutable borrows
        // across a `Deref`.
        let this = &mut *self;

        loop {
            ready!(this.flush_write_out(cx))?;

            // We ask the compressor to flush everything into `self.write_out`.
            debug_assert!(this.write_out.is_empty());
            this.write_out.reserve(256); // compress_vec uses the Vec's capacity
            this.compress
                .compress_vec(&[], &mut this.write_out, flate2::FlushCompress::Finish)?;
            if this.write_out.is_empty() {
                break;
            }
        }

        AsyncWrite::poll_close(Pin::new(&mut this.inner), cx)
    }
}
