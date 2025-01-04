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

#![allow(deprecated)]

use std::{
    convert::TryFrom as _,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
    ready,
};

use crate::core::muxing::{StreamMuxer, StreamMuxerEvent};

/// Wraps around a [`StreamMuxer`] and counts the number of bytes that go through all the opened
/// streams.
#[derive(Clone)]
#[pin_project::pin_project]
pub(crate) struct BandwidthLogging<SMInner> {
    #[pin]
    inner: SMInner,
    sinks: Arc<BandwidthSinks>,
}

impl<SMInner> BandwidthLogging<SMInner> {
    /// Creates a new [`BandwidthLogging`] around the stream muxer.
    pub(crate) fn new(inner: SMInner, sinks: Arc<BandwidthSinks>) -> Self {
        Self { inner, sinks }
    }
}

impl<SMInner> StreamMuxer for BandwidthLogging<SMInner>
where
    SMInner: StreamMuxer,
{
    type Substream = InstrumentedStream<SMInner::Substream>;
    type Error = SMInner::Error;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        let this = self.project();
        this.inner.poll(cx)
    }

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.project();
        let inner = ready!(this.inner.poll_inbound(cx)?);
        let logged = InstrumentedStream {
            inner,
            sinks: this.sinks.clone(),
        };
        Poll::Ready(Ok(logged))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.project();
        let inner = ready!(this.inner.poll_outbound(cx)?);
        let logged = InstrumentedStream {
            inner,
            sinks: this.sinks.clone(),
        };
        Poll::Ready(Ok(logged))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        this.inner.poll_close(cx)
    }
}

/// Allows obtaining the average bandwidth of the streams.
#[deprecated(
    note = "Use `libp2p::SwarmBuilder::with_bandwidth_metrics` or `libp2p_metrics::BandwidthTransport` instead."
)]
pub struct BandwidthSinks {
    inbound: AtomicU64,
    outbound: AtomicU64,
}

impl BandwidthSinks {
    /// Returns a new [`BandwidthSinks`].
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            inbound: AtomicU64::new(0),
            outbound: AtomicU64::new(0),
        })
    }

    /// Returns the total number of bytes that have been downloaded on all the streams.
    ///
    /// > **Note**: This method is by design subject to race conditions. The returned value should
    /// > only ever be used for statistics purposes.
    pub fn total_inbound(&self) -> u64 {
        self.inbound.load(Ordering::Relaxed)
    }

    /// Returns the total number of bytes that have been uploaded on all the streams.
    ///
    /// > **Note**: This method is by design subject to race conditions. The returned value should
    /// > only ever be used for statistics purposes.
    pub fn total_outbound(&self) -> u64 {
        self.outbound.load(Ordering::Relaxed)
    }
}

/// Wraps around an [`AsyncRead`] + [`AsyncWrite`] and logs the bandwidth that goes through it.
#[pin_project::pin_project]
pub(crate) struct InstrumentedStream<SMInner> {
    #[pin]
    inner: SMInner,
    sinks: Arc<BandwidthSinks>,
}

impl<SMInner: AsyncRead> AsyncRead for InstrumentedStream<SMInner> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_read(cx, buf))?;
        this.sinks.inbound.fetch_add(
            u64::try_from(num_bytes).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_read_vectored(cx, bufs))?;
        this.sinks.inbound.fetch_add(
            u64::try_from(num_bytes).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        Poll::Ready(Ok(num_bytes))
    }
}

impl<SMInner: AsyncWrite> AsyncWrite for InstrumentedStream<SMInner> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_write(cx, buf))?;
        this.sinks.outbound.fetch_add(
            u64::try_from(num_bytes).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_write_vectored(cx, bufs))?;
        this.sinks.outbound.fetch_add(
            u64::try_from(num_bytes).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_close(cx)
    }
}
