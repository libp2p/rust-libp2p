use crate::muxing::StreamMuxerEvent;
use crate::StreamMuxer;
use fnv::FnvHashMap;
use futures::{AsyncRead, AsyncWrite};
use parking_lot::Mutex;
use std::fmt;
use std::io;
use std::io::{IoSlice, IoSliceMut};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

/// Abstract `StreamMuxer`.
pub struct StreamMuxerBox {
    inner: Box<
        dyn StreamMuxer<Substream = SubstreamBox, OutboundSubstream = usize, Error = io::Error>
            + Send
            + Sync,
    >,
}

/// Abstract type for asynchronous reading and writing.
///
/// A [`SubstreamBox`] erases the concrete type it is given and only retains its `AsyncRead`
/// and `AsyncWrite` capabilities.
pub struct SubstreamBox(Box<dyn AsyncReadWrite + Send + Unpin>);

struct Wrap<T>
where
    T: StreamMuxer,
{
    inner: T,
    outbound: Mutex<FnvHashMap<usize, T::OutboundSubstream>>,
    next_outbound: AtomicUsize,
}

impl<T> StreamMuxer for Wrap<T>
where
    T: StreamMuxer,
    T::Substream: Send + Unpin + 'static,
{
    type Substream = SubstreamBox;
    type OutboundSubstream = usize; // TODO: use a newtype
    type Error = io::Error;

    #[inline]
    fn poll_event(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent<Self::Substream>, Self::Error>> {
        let substream = match self.inner.poll_event(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(StreamMuxerEvent::AddressChange(a))) => {
                return Poll::Ready(Ok(StreamMuxerEvent::AddressChange(a)))
            }
            Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(s))) => s,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err.into())),
        };

        Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(SubstreamBox::new(
            substream,
        ))))
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
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut list = self.outbound.lock();
        let substream = match self
            .inner
            .poll_outbound(cx, list.get_mut(substream).unwrap())
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(s)) => s,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err.into())),
        };

        Poll::Ready(Ok(SubstreamBox::new(substream)))
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        let mut list = self.outbound.lock();
        self.inner
            .destroy_outbound(list.remove(&substream).unwrap())
    }

    #[inline]
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_close(cx).map_err(|e| e.into())
    }
}

impl StreamMuxerBox {
    /// Turns a stream muxer into a `StreamMuxerBox`.
    pub fn new<T>(muxer: T) -> StreamMuxerBox
    where
        T: StreamMuxer + Send + Sync + 'static,
        T::OutboundSubstream: Send,
        T::Substream: Send + Unpin + 'static,
    {
        let wrap = Wrap {
            inner: muxer,
            outbound: Mutex::new(Default::default()),
            next_outbound: AtomicUsize::new(0),
        };

        StreamMuxerBox {
            inner: Box::new(wrap),
        }
    }
}

impl StreamMuxer for StreamMuxerBox {
    type Substream = SubstreamBox;
    type OutboundSubstream = usize; // TODO: use a newtype
    type Error = io::Error;

    #[inline]
    fn poll_event(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent<Self::Substream>, Self::Error>> {
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
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.inner.poll_outbound(cx, s)
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        self.inner.destroy_outbound(substream)
    }

    #[inline]
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_close(cx)
    }
}

impl SubstreamBox {
    /// Construct a new [`SubstreamBox`] from something that implements [`AsyncRead`] and [`AsyncWrite`].
    pub fn new<S: AsyncRead + AsyncWrite + Send + Unpin + 'static>(stream: S) -> Self {
        Self(Box::new(stream))
    }
}

impl fmt::Debug for SubstreamBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SubstreamBox({})", self.0.type_name())
    }
}

/// Workaround because Rust does not allow `Box<dyn AsyncRead + AsyncWrite>`.
trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin {
    /// Helper function to capture the erased inner type.
    ///
    /// Used to make the [`Debug`] implementation of [`SubstreamBox`] more useful.
    fn type_name(&self) -> &'static str;
}

impl<S> AsyncReadWrite for S
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn type_name(&self) -> &'static str {
        std::any::type_name::<S>()
    }
}

impl AsyncRead for SubstreamBox {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_read_vectored(cx, bufs)
    }
}

impl AsyncWrite for SubstreamBox {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write_vectored(cx, bufs)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_close(cx)
    }
}
