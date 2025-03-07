use std::{
    convert::TryFrom as _,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    future::{MapOk, TryFutureExt},
    io::{IoSlice, IoSliceMut},
    prelude::*,
    ready,
};
use libp2p_core::{
    muxing::{StreamMuxer, StreamMuxerEvent},
    transport::{DialOpts, ListenerId, TransportError, TransportEvent},
    Multiaddr,
};
use libp2p_identity::PeerId;
use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{counter::Counter, family::Family},
    registry::{Registry, Unit},
};

use crate::protocol_stack;

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
        registry
            .sub_registry_with_prefix("libp2p")
            .register_with_unit(
                "bandwidth",
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

impl<T, M> libp2p_core::Transport for Transport<T>
where
    T: libp2p_core::Transport<Output = (PeerId, M)>,
    M: StreamMuxer + Send + 'static,
    M::Substream: Send + 'static,
    M::Error: Send + Sync + 'static,
{
    type Output = (PeerId, Muxer<M>);
    type Error = T::Error;
    type ListenerUpgrade =
        MapOk<T::ListenerUpgrade, Box<dyn FnOnce((PeerId, M)) -> (PeerId, Muxer<M>) + Send>>;
    type Dial = MapOk<T::Dial, Box<dyn FnOnce((PeerId, M)) -> (PeerId, Muxer<M>) + Send>>;

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

    fn dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let metrics = ConnectionMetrics::from_family_and_addr(&self.metrics, &addr);
        Ok(self
            .transport
            .dial(addr.clone(), dial_opts)?
            .map_ok(Box::new(|(peer_id, stream_muxer)| {
                (peer_id, Muxer::new(stream_muxer, metrics))
            })))
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
                let metrics =
                    ConnectionMetrics::from_family_and_addr(this.metrics, &send_back_addr);
                Poll::Ready(TransportEvent::Incoming {
                    listener_id,
                    upgrade: upgrade.map_ok(Box::new(|(peer_id, stream_muxer)| {
                        (peer_id, Muxer::new(stream_muxer, metrics))
                    })),
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

#[derive(Clone, Debug)]
struct ConnectionMetrics {
    outbound: Counter,
    inbound: Counter,
}

impl ConnectionMetrics {
    fn from_family_and_addr(family: &Family<Labels, Counter>, protocols: &Multiaddr) -> Self {
        let protocols = protocol_stack::as_string(protocols);

        // Additional scope to make sure to drop the lock guard from `get_or_create`.
        let outbound = {
            let m = family.get_or_create(&Labels {
                protocols: protocols.clone(),
                direction: Direction::Outbound,
            });
            m.clone()
        };
        // Additional scope to make sure to drop the lock guard from `get_or_create`.
        let inbound = {
            let m = family.get_or_create(&Labels {
                protocols,
                direction: Direction::Inbound,
            });
            m.clone()
        };
        ConnectionMetrics { outbound, inbound }
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
    /// Creates a new [`Muxer`] wrapping around the provided stream muxer.
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
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::MAX));
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
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::MAX));
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
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::MAX));
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
            .inc_by(u64::try_from(num_bytes).unwrap_or(u64::MAX));
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
