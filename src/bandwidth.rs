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

use crate::{
    core::{
        transport::{TransportError, TransportEvent},
        Transport,
    },
    Multiaddr,
};

use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
    ready,
};
use libp2p_core::muxing::StreamMuxerEvent;
use libp2p_core::transport::ListenerId;
use libp2p_core::{PeerId, StreamMuxer};
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

/// Wraps around a `Transport` and counts the number of bytes that go through all the opened
/// connections.
#[derive(Clone)]
#[pin_project::pin_project]
pub struct BandwidthLogging<TInner> {
    #[pin]
    inner: TInner,
    sinks: Arc<BandwidthSinks>,
}

impl<TInner> BandwidthLogging<TInner> {
    /// Creates a new [`BandwidthLogging`] around the transport.
    pub fn new(inner: TInner) -> (Self, Arc<BandwidthSinks>) {
        let sink = Arc::new(BandwidthSinks {
            inbound: AtomicU64::new(0),
            outbound: AtomicU64::new(0),
        });

        let trans = BandwidthLogging {
            inner,
            sinks: sink.clone(),
        };

        (trans, sink)
    }
}

pub trait WithBandwidthSinks {
    type Output;

    fn with_bandwidth_sinks(self, sinks: Arc<BandwidthSinks>) -> Self::Output;
}

#[cfg(all(feature = "tcp", feature = "async-std"))]
impl WithBandwidthSinks for libp2p_tcp::async_io::Stream {
    type Output = BandwidthConnecLogging<Self>;

    fn with_bandwidth_sinks(self, sinks: Arc<BandwidthSinks>) -> Self::Output {
        BandwidthConnecLogging { inner: self, sinks }
    }
}

#[cfg(all(feature = "tcp", feature = "tokio"))]
impl WithBandwidthSinks for libp2p_tcp::tokio::Stream {
    type Output = BandwidthConnecLogging<Self>;

    fn with_bandwidth_sinks(self, sinks: Arc<BandwidthSinks>) -> Self::Output {
        BandwidthConnecLogging { inner: self, sinks }
    }
}

#[cfg(feature = "webrtc")]
impl WithBandwidthSinks for (PeerId, libp2p_webrtc::tokio::Connection) {
    type Output = (PeerId, Connection<libp2p_webrtc::tokio::Connection>);

    fn with_bandwidth_sinks(self, sinks: Arc<BandwidthSinks>) -> Self::Output {
        (
            self.0,
            Connection {
                inner: self.1,
                sinks,
            },
        )
    }
}

#[pin_project::pin_project]
pub struct Connection<I> {
    #[pin]
    inner: I,
    sinks: Arc<BandwidthSinks>,
}

impl<I> StreamMuxer for Connection<I>
where
    I: StreamMuxer + UpdateBandwidthSinks,
{
    type Substream = I::Substream;
    type Error = I::Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.project().inner.poll_inbound(cx)
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.project().inner.poll_outbound(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close(cx)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        let mut this = self.project();

        this.inner.as_mut().update_bandwidth_sinks(this.sinks);

        this.inner.poll(cx)
    }
}

pub trait UpdateBandwidthSinks {
    fn update_bandwidth_sinks(self: Pin<&mut Self>, sinks: &BandwidthSinks);
}

#[cfg(feature = "webrtc")]
impl UpdateBandwidthSinks for libp2p_webrtc::tokio::Connection {
    fn update_bandwidth_sinks(self: Pin<&mut Self>, _: &BandwidthSinks) {
        // TODO: Fetch data from connection via public functions and update sinks
    }
}

impl<TInner> Transport for BandwidthLogging<TInner>
where
    TInner: Transport,
    TInner::Output: WithBandwidthSinks,
{
    type Output = <TInner::Output as WithBandwidthSinks>::Output;
    type Error = TInner::Error;
    type ListenerUpgrade = BandwidthFuture<TInner::ListenerUpgrade>;
    type Dial = BandwidthFuture<TInner::Dial>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        match this.inner.poll(cx) {
            Poll::Ready(event) => {
                let event = event.map_upgrade({
                    let sinks = this.sinks.clone();
                    |inner| BandwidthFuture { inner, sinks }
                });
                Poll::Ready(event)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        self.inner.listen_on(addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let sinks = self.sinks.clone();
        self.inner
            .dial(addr)
            .map(move |fut| BandwidthFuture { inner: fut, sinks })
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let sinks = self.sinks.clone();
        self.inner
            .dial_as_listener(addr)
            .map(move |fut| BandwidthFuture { inner: fut, sinks })
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(server, observed)
    }
}

/// Wraps around a `Future` that produces a connection. Wraps the connection around a bandwidth
/// counter.
#[pin_project::pin_project]
pub struct BandwidthFuture<TInner> {
    #[pin]
    inner: TInner,
    sinks: Arc<BandwidthSinks>,
}

impl<TInner, TOk, TErr> Future for BandwidthFuture<TInner>
where
    TInner: Future<Output = Result<TOk, TErr>>,
    TOk: WithBandwidthSinks,
{
    type Output = Result<<TOk as WithBandwidthSinks>::Output, TErr>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let inner = ready!(this.inner.poll(cx))?;

        let output = inner.with_bandwidth_sinks(this.sinks.clone());

        Poll::Ready(Ok(output))
    }
}

/// Allows obtaining the average bandwidth of the connections created from a [`BandwidthLogging`].
pub struct BandwidthSinks {
    inbound: AtomicU64,
    outbound: AtomicU64,
}

impl BandwidthSinks {
    /// Returns the total number of bytes that have been downloaded on all the connections spawned
    /// through the [`BandwidthLogging`].
    ///
    /// > **Note**: This method is by design subject to race conditions. The returned value should
    /// >           only ever be used for statistics purposes.
    pub fn total_inbound(&self) -> u64 {
        self.inbound.load(Ordering::Relaxed)
    }

    /// Returns the total number of bytes that have been uploaded on all the connections spawned
    /// through the [`BandwidthLogging`].
    ///
    /// > **Note**: This method is by design subject to race conditions. The returned value should
    /// >           only ever be used for statistics purposes.
    pub fn total_outbound(&self) -> u64 {
        self.outbound.load(Ordering::Relaxed)
    }
}

/// Wraps around an `AsyncRead + AsyncWrite` and logs the bandwidth that goes through it.
#[pin_project::pin_project]
pub struct BandwidthConnecLogging<TInner> {
    #[pin]
    inner: TInner,
    sinks: Arc<BandwidthSinks>,
}

impl<TInner: AsyncRead> AsyncRead for BandwidthConnecLogging<TInner> {
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

impl<TInner: AsyncWrite> AsyncWrite for BandwidthConnecLogging<TInner> {
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
