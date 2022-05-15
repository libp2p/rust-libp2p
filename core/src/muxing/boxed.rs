use crate::muxing::StreamMuxerEvent;
use crate::StreamMuxer;
use fnv::FnvHashMap;
use parking_lot::Mutex;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

/// Abstract `StreamMuxer`.
pub struct StreamMuxerBox {
    inner: Box<dyn StreamMuxer<Substream = usize, OutboundSubstream = usize> + Send + Sync>,
}

struct Wrap<T>
where
    T: StreamMuxer,
{
    inner: T,
    substreams: Mutex<FnvHashMap<usize, T::Substream>>,
    next_substream: AtomicUsize,
    outbound: Mutex<FnvHashMap<usize, T::OutboundSubstream>>,
    next_outbound: AtomicUsize,
}

impl<T> StreamMuxer for Wrap<T>
where
    T: StreamMuxer,
{
    type Substream = usize; // TODO: use a newtype
    type OutboundSubstream = usize; // TODO: use a newtype

    #[inline]
    fn poll_event(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<StreamMuxerEvent<Self::Substream>>> {
        let substream = match self.inner.poll_event(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(StreamMuxerEvent::AddressChange(a))) => {
                return Poll::Ready(Ok(StreamMuxerEvent::AddressChange(a)))
            }
            Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(s))) => s,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };

        let id = self.next_substream.fetch_add(1, Ordering::Relaxed);
        self.substreams.lock().insert(id, substream);
        Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(id)))
    }

    #[inline]
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let outbound = self.inner.open_outbound();
        let id = self.next_outbound.fetch_add(1, Ordering::Relaxed);
        self.outbound.lock().insert(id, outbound);
        id
    }

    #[inline]
    fn poll_outbound(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<io::Result<Self::Substream>> {
        let mut list = self.outbound.lock();
        let substream = match self
            .inner
            .poll_outbound(cx, list.get_mut(substream).unwrap())
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(s)) => s,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        let id = self.next_substream.fetch_add(1, Ordering::Relaxed);
        self.substreams.lock().insert(id, substream);
        Poll::Ready(Ok(id))
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        let mut list = self.outbound.lock();
        self.inner
            .destroy_outbound(list.remove(&substream).unwrap())
    }

    #[inline]
    fn read_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut list = self.substreams.lock();
        self.inner.read_substream(cx, list.get_mut(s).unwrap(), buf)
    }

    #[inline]
    fn write_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut list = self.substreams.lock();
        self.inner
            .write_substream(cx, list.get_mut(s).unwrap(), buf)
    }

    #[inline]
    fn flush_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
    ) -> Poll<io::Result<()>> {
        let mut list = self.substreams.lock();
        self.inner.flush_substream(cx, list.get_mut(s).unwrap())
    }

    #[inline]
    fn shutdown_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
    ) -> Poll<io::Result<()>> {
        let mut list = self.substreams.lock();
        self.inner.shutdown_substream(cx, list.get_mut(s).unwrap())
    }

    #[inline]
    fn destroy_substream(&self, substream: Self::Substream) {
        let mut list = self.substreams.lock();
        self.inner
            .destroy_substream(list.remove(&substream).unwrap())
    }

    #[inline]
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_close(cx)
    }
}

impl StreamMuxerBox {
    /// Turns a stream muxer into a `StreamMuxerBox`.
    pub fn new<T>(muxer: T) -> StreamMuxerBox
    where
        T: StreamMuxer + Send + Sync + 'static,
        T::OutboundSubstream: Send,
        T::Substream: Send,
    {
        let wrap = Wrap {
            inner: muxer,
            substreams: Mutex::new(Default::default()),
            next_substream: AtomicUsize::new(0),
            outbound: Mutex::new(Default::default()),
            next_outbound: AtomicUsize::new(0),
        };

        StreamMuxerBox {
            inner: Box::new(wrap),
        }
    }
}

impl StreamMuxer for StreamMuxerBox {
    type Substream = usize; // TODO: use a newtype
    type OutboundSubstream = usize; // TODO: use a newtype

    #[inline]
    fn poll_event(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<StreamMuxerEvent<Self::Substream>>> {
        self.inner.poll_event(cx)
    }

    #[inline]
    fn open_outbound(&self) -> Self::OutboundSubstream {
        self.inner.open_outbound()
    }

    #[inline]
    fn poll_outbound(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::OutboundSubstream,
    ) -> Poll<io::Result<Self::Substream>> {
        self.inner.poll_outbound(cx, s)
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        self.inner.destroy_outbound(substream)
    }

    #[inline]
    fn read_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        self.inner.read_substream(cx, s, buf)
    }

    #[inline]
    fn write_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.inner.write_substream(cx, s, buf)
    }

    #[inline]
    fn flush_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
    ) -> Poll<io::Result<()>> {
        self.inner.flush_substream(cx, s)
    }

    #[inline]
    fn shutdown_substream(
        &self,
        cx: &mut Context<'_>,
        s: &mut Self::Substream,
    ) -> Poll<io::Result<()>> {
        self.inner.shutdown_substream(cx, s)
    }

    #[inline]
    fn destroy_substream(&self, s: Self::Substream) {
        self.inner.destroy_substream(s)
    }

    #[inline]
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_close(cx)
    }
}
