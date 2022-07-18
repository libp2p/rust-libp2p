use crate::StreamMuxer;
use futures::{AsyncRead, AsyncWrite};
use multiaddr::Multiaddr;
use std::error::Error;
use std::fmt;
use std::io;
use std::io::{IoSlice, IoSliceMut};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Abstract `StreamMuxer`.
pub struct StreamMuxerBox {
    inner: Box<dyn StreamMuxer<Substream = SubstreamBox, Error = io::Error> + Send + Sync>,
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
}

impl<T> StreamMuxer for Wrap<T>
where
    T: StreamMuxer,
    T::Substream: Send + Unpin + 'static,
    T::Error: Send + Sync + 'static,
{
    type Substream = SubstreamBox;
    type Error = io::Error;

    #[inline]
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_close(cx).map_err(into_io_error)
    }

    fn poll_inbound(&self, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        self.inner
            .poll_inbound(cx)
            .map_ok(SubstreamBox::new)
            .map_err(into_io_error)
    }

    fn poll_outbound(&self, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        self.inner
            .poll_outbound(cx)
            .map_ok(SubstreamBox::new)
            .map_err(into_io_error)
    }

    fn poll_address_change(&self, cx: &mut Context<'_>) -> Poll<Result<Multiaddr, Self::Error>> {
        self.inner.poll_address_change(cx).map_err(into_io_error)
    }
}

fn into_io_error<E>(err: E) -> io::Error
where
    E: Error + Send + Sync + 'static,
{
    io::Error::new(io::ErrorKind::Other, err)
}

impl StreamMuxerBox {
    /// Turns a stream muxer into a `StreamMuxerBox`.
    pub fn new<T>(muxer: T) -> StreamMuxerBox
    where
        T: StreamMuxer + Send + Sync + 'static,
        T::Substream: Send + Unpin + 'static,
        T::Error: Send + Sync + 'static,
    {
        let wrap = Wrap { inner: muxer };

        StreamMuxerBox {
            inner: Box::new(wrap),
        }
    }
}

impl StreamMuxer for StreamMuxerBox {
    type Substream = SubstreamBox;
    type Error = io::Error;

    #[inline]
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_close(cx)
    }

    fn poll_inbound(&self, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        self.inner.poll_inbound(cx)
    }

    fn poll_outbound(&self, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        self.inner.poll_outbound(cx)
    }

    fn poll_address_change(&self, cx: &mut Context<'_>) -> Poll<Result<Multiaddr, Self::Error>> {
        self.inner.poll_address_change(cx)
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
