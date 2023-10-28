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

use libp2p_core::{
    muxing::{StreamMuxer, StreamMuxerBox, StreamMuxerEvent},
    transport::{ListenerId, TransportError, TransportEvent},
    Multiaddr,
};

use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
    ready,
};
use libp2p_identity::PeerId;
use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{counter::Counter, family::Family},
    registry::{Registry, Unit},
};
use std::{
    convert::TryFrom as _,
    io,
    pin::Pin,
    task::{Context, Poll},
};

// TODO: rename to Transport
/// See `Transport::map`.
#[derive(Debug, Clone)]
#[pin_project::pin_project]
pub struct Transport<T> {
    #[pin]
    transport: T,
    metrics: Family<Labels, Counter>,
}

impl<T> Transport<T> {
    pub fn new(transport: T, registry: &mut Registry) -> Self {
        let metrics = Family::<Labels, Counter>::default();

        registry.register_with_unit(
            // TODO: Ideally no prefix would be needed.
            "libp2p_swarm_bandwidth",
            "Bandwidth usage by direction and transport protocols",
            Unit::Bytes,
            metrics.clone(),
        );

        Transport { transport, metrics }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct Labels {
    protocols: String,
    direction: Direction,
}

#[derive(Clone, Hash, PartialEq, Eq, EncodeLabelValue, Debug)]
enum Direction {
    Inbound,
    Outbound,
}

impl<T> libp2p_core::Transport for Transport<T>
where
    // TODO: Consider depending on StreamMuxer only.
    T: libp2p_core::Transport<Output = (PeerId, StreamMuxerBox)>,
{
    type Output = (PeerId, StreamMuxerBox);
    type Error = T::Error;
    type ListenerUpgrade = MapFuture<T::ListenerUpgrade>;
    type Dial = MapFuture<T::Dial>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.transport.listen_on(id, addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.transport.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        Ok(MapFuture {
            metrics: Some(ConnectionMetrics::from_family_and_protocols(
                &self.metrics,
                as_string(&addr),
            )),
            inner: self.transport.dial(addr.clone())?,
        })
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Ok(MapFuture {
            metrics: Some(ConnectionMetrics::from_family_and_protocols(
                &self.metrics,
                as_string(&addr),
            )),
            inner: self.transport.dial_as_listener(addr.clone())?,
        })
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.address_translation(server, observed)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        match this.transport.poll(cx) {
            Poll::Ready(TransportEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr,
            }) => Poll::Ready(TransportEvent::Incoming {
                listener_id,
                upgrade: MapFuture {
                    metrics: Some(ConnectionMetrics::from_family_and_protocols(
                        this.metrics,
                        as_string(&send_back_addr),
                    )),
                    inner: upgrade,
                },
                local_addr,
                send_back_addr,
            }),
            Poll::Ready(other) => {
                let mapped = other.map_upgrade(|_upgrade| unreachable!("case already matched"));
                Poll::Ready(mapped)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Clone, Debug)]
struct ConnectionMetrics {
    outbound: Counter,
    inbound: Counter,
}

impl ConnectionMetrics {
    fn from_family_and_protocols(family: &Family<Labels, Counter>, protocols: String) -> Self {
        ConnectionMetrics {
            outbound: family
                .get_or_create(&Labels {
                    protocols: protocols.clone(),
                    direction: Direction::Outbound,
                })
                .clone(),
            inbound: family
                .get_or_create(&Labels {
                    protocols: protocols,
                    direction: Direction::Inbound,
                })
                .clone(),
        }
    }
}

/// Custom `Future` to avoid boxing.
///
/// Applies a function to the inner future's result.
#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapFuture<T> {
    #[pin]
    inner: T,
    metrics: Option<ConnectionMetrics>,
}

impl<T> Future for MapFuture<T>
where
    T: TryFuture<Ok = (PeerId, StreamMuxerBox)>,
{
    type Output = Result<(PeerId, StreamMuxerBox), T::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let (peer_id, stream_muxer) = match TryFuture::try_poll(this.inner, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(v)) => v,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        Poll::Ready(Ok((
            peer_id,
            StreamMuxerBox::new(Muxer::new(stream_muxer, this.metrics.take().expect("todo"))),
        )))
    }
}

/// Wraps around a [`StreamMuxer`] and counts the number of bytes that go through all the opened
/// streams.
#[derive(Clone)]
#[pin_project::pin_project]
pub struct Muxer<SMInner> {
    #[pin]
    inner: SMInner,
    metrics: ConnectionMetrics,
}

impl<SMInner> Muxer<SMInner> {
    /// Creates a new [`BandwidthLogging`] around the stream muxer.
    fn new(inner: SMInner, metrics: ConnectionMetrics) -> Self {
        Self { inner, metrics }
    }
}

impl<SMInner> StreamMuxer for Muxer<SMInner>
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
            metrics: this.metrics.clone(),
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
            metrics: this.metrics.clone(),
        };
        Poll::Ready(Ok(logged))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        this.inner.poll_close(cx)
    }
}

/// Wraps around an [`AsyncRead`] + [`AsyncWrite`] and logs the bandwidth that goes through it.
#[pin_project::pin_project]
pub struct InstrumentedStream<SMInner> {
    #[pin]
    inner: SMInner,
    metrics: ConnectionMetrics,
}

impl<SMInner: AsyncRead> AsyncRead for InstrumentedStream<SMInner> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_read(cx, buf))?;
        this.metrics
            .inbound
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::max_value()));
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_read_vectored(cx, bufs))?;
        this.metrics
            .inbound
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::max_value()));
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
        this.metrics
            .outbound
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::max_value()));
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_write_vectored(cx, bufs))?;
        this.metrics
            .outbound
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::max_value()));
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

// TODO: rename
fn as_string(ma: &Multiaddr) -> String {
    let len = ma
        .protocol_stack()
        .fold(0, |acc, proto| acc + proto.len() + 1);
    let mut protocols = String::with_capacity(len);
    for proto_tag in ma.protocol_stack() {
        protocols.push('/');
        protocols.push_str(proto_tag);
    }
    protocols
}
