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

use crate::core::muxing::{StreamMuxer, StreamMuxerEvent};

use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
    ready,
};
use prometheus_client::{
    encoding::{DescriptorEncoder, EncodeMetric},
    metrics::{counter::ConstCounter, MetricType},
};
use std::{
    collections::HashMap,
    convert::TryFrom as _,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    task::{Context, Poll},
};

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
#[derive(Default, Debug)]
pub struct BandwidthSinks {
    inbound: AtomicU64,
    outbound: AtomicU64,
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
            u64::try_from(num_bytes).unwrap_or(u64::max_value()),
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
            u64::try_from(num_bytes).unwrap_or(u64::max_value()),
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
            u64::try_from(num_bytes).unwrap_or(u64::max_value()),
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
            u64::try_from(num_bytes).unwrap_or(u64::max_value()),
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

// TODO: Ideally this should go somewhere else. I.e. good to not depend on prometheus-client in libp2p.
pub fn register_bandwidth_sinks(
    registry: &mut prometheus_client::registry::Registry,
    sinks: Arc<RwLock<HashMap<String, Arc<BandwidthSinks>>>>,
) {
    registry.register_collector(Box::new(SinksCollector(sinks)));
}

#[derive(Debug)]
struct SinksCollector(Arc<RwLock<HashMap<String, Arc<BandwidthSinks>>>>);

impl prometheus_client::collector::Collector for SinksCollector {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
        let mut family_encoder =
            encoder.encode_descriptor("bandwidth", "todo", None, MetricType::Counter)?;
        for (protocols, sink) in self.0.read().expect("todo").iter() {
            let labels = [("protocols", protocols.as_str()), ("direction", "inbound")];
            let metric_encoder = family_encoder.encode_family(&labels)?;
            ConstCounter::new(sink.inbound.load(Ordering::Relaxed)).encode(metric_encoder)?;

            let labels = [("protocols", protocols.as_str()), ("direction", "outbound")];
            let metric_encoder = family_encoder.encode_family(&labels)?;
            ConstCounter::new(sink.outbound.load(Ordering::Relaxed)).encode(metric_encoder)?;
        }

        Ok(())
    }
}
