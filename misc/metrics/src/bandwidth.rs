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
use prometheus_client::registry::Registry;
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

// TODO: rename to Transport
/// See `Transport::map`.
#[derive(Debug, Clone)]
#[pin_project::pin_project]
pub struct Transport<T> {
    #[pin]
    transport: T,
    sinks: Arc<RwLock<HashMap<String, Arc<BandwidthSinks>>>>,
}

impl<T> Transport<T> {
    pub fn new(transport: T, registry: &mut Registry) -> Self {
        let sinks: Arc<RwLock<HashMap<_, Arc<BandwidthSinks>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        registry.register_collector(Box::new(SinksCollector(sinks.clone())));

        Transport { transport, sinks }
    }
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
        let sinks = self
            .sinks
            .write()
            .expect("todo")
            .entry(as_string(&addr))
            .or_default()
            .clone();
        let future = self.transport.dial(addr.clone())?;
        Ok(MapFuture {
            inner: future,
            sinks: Some(sinks),
        })
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let sinks = self
            .sinks
            .write()
            .expect("todo")
            .entry(as_string(&addr))
            .or_default()
            .clone();
        let future = self.transport.dial_as_listener(addr.clone())?;
        Ok(MapFuture {
            inner: future,
            sinks: Some(sinks),
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
            }) => {
                // TODO: Abstract into method?
                let sinks = this
                    .sinks
                    .write()
                    .expect("todo")
                    .entry(as_string(&send_back_addr))
                    .or_default()
                    .clone();
                Poll::Ready(TransportEvent::Incoming {
                    listener_id,
                    upgrade: MapFuture {
                        inner: upgrade,
                        sinks: Some(sinks),
                    },
                    local_addr,
                    send_back_addr,
                })
            }
            Poll::Ready(other) => {
                let mapped = other.map_upgrade(|_upgrade| unreachable!("case already matched"));
                Poll::Ready(mapped)
            }
            Poll::Pending => Poll::Pending,
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
    sinks: Option<Arc<BandwidthSinks>>,
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
            StreamMuxerBox::new(Muxer::new(stream_muxer, this.sinks.take().expect("todo"))),
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
    sinks: Arc<BandwidthSinks>,
}

impl<SMInner> Muxer<SMInner> {
    /// Creates a new [`BandwidthLogging`] around the stream muxer.
    pub fn new(inner: SMInner, sinks: Arc<BandwidthSinks>) -> Self {
        Self { inner, sinks }
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
pub struct InstrumentedStream<SMInner> {
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

#[derive(Debug)]
pub struct SinksCollector(Arc<RwLock<HashMap<String, Arc<BandwidthSinks>>>>);

impl prometheus_client::collector::Collector for SinksCollector {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
        let mut family_encoder = encoder.encode_descriptor(
            "libp2p_swarm_bandwidth",
            "Bandwidth usage by direction and transport protocols",
            None,
            MetricType::Counter,
        )?;

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
