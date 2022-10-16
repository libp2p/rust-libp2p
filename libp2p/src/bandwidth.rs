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
use libp2p_core::transport::ListenerId;
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

impl<TInner> Transport for BandwidthLogging<TInner>
where
    TInner: Transport,
{
    type Output = BandwidthConnecLogging<TInner::Output>;
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

impl<TInner: TryFuture> Future for BandwidthFuture<TInner> {
    type Output = Result<BandwidthConnecLogging<TInner::Ok>, TInner::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let inner = ready!(this.inner.try_poll(cx)?);
        let logged = BandwidthConnecLogging {
            inner,
            sinks: this.sinks.clone(),
        };
        Poll::Ready(Ok(logged))
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
