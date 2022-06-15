use crate::muxing::{OutboundSubstreamId, StreamMuxerEvent};
use crate::StreamMuxer;
use futures::{AsyncRead, AsyncWrite, Stream};
use std::io::{IoSlice, IoSliceMut};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{fmt, io};

/// Abstract [`StreamMuxer`] with [`SubstreamBox`] as its `Substream` type.
pub struct StreamMuxerBox {
    inner: Box<dyn StreamMuxer<Substream = SubstreamBox> + Send + 'static>,
}

/// Abstract type for asynchronous reading and writing.
///
/// A [`SubstreamBox`] erases the concrete type it is given and only retains its `AsyncRead`
/// and `AsyncWrite` capabilities.
pub struct SubstreamBox(Box<dyn AsyncReadWrite + Send + Unpin + 'static>);

/// Helper type to abstract away the concrete [`Substream`](StreamMuxer::Substream) type of a [`StreamMuxer`].
///
/// Within this implementation, we map the concrete [`Substream`](StreamMuxer::Substream) to a [`SubstreamBox`].
/// By parameterizing the entire [`Wrap`] type, we can stuff it into a [`Box`] and erase the concrete type.
struct Wrap<T> {
    inner: T,
}

impl<T> StreamMuxer for Wrap<T>
where
    T: StreamMuxer,
    T::Substream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Substream = SubstreamBox;

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<StreamMuxerEvent<Self::Substream>>> {
        let event = futures::ready!(self.inner.poll(cx))?.map_stream(SubstreamBox::new);

        Poll::Ready(Ok(event))
    }

    fn open_outbound(&mut self) -> OutboundSubstreamId {
        self.inner.open_outbound()
    }

    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_close(cx)
    }
}

impl StreamMuxerBox {
    /// Construct a new [`StreamMuxerBox`].
    pub fn new<S>(muxer: impl StreamMuxer<Substream = S> + Send + 'static) -> Self
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        Self {
            inner: Box::new(Wrap { inner: muxer }),
        }
    }
}

impl StreamMuxer for StreamMuxerBox {
    type Substream = SubstreamBox;

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<StreamMuxerEvent<Self::Substream>>> {
        self.inner.poll(cx)
    }

    fn open_outbound(&mut self) -> OutboundSubstreamId {
        self.inner.open_outbound()
    }

    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_close(cx)
    }
}

impl Stream for StreamMuxerBox {
    type Item = io::Result<StreamMuxerEvent<<Self as StreamMuxer>::Substream>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll(cx).map(Some)
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
    /// Used to make the `Debug` implementation of `SubstreamBox` more useful.
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
