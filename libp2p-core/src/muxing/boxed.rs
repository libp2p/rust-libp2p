use crate::muxing::{StreamMuxer, StreamMuxerEvent};
use futures::{AsyncRead, AsyncWrite};
use pin_project::pin_project;
use std::error::Error;
use std::fmt;
use std::io;
use std::io::{IoSlice, IoSliceMut};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Abstract `StreamMuxer`.
pub struct StreamMuxerBox {
    inner: Pin<Box<dyn StreamMuxer<Substream = SubstreamBox, Error = io::Error> + Send>>,
}

/// Abstract type for asynchronous reading and writing.
///
/// A [`SubstreamBox`] erases the concrete type it is given and only retains its `AsyncRead`
/// and `AsyncWrite` capabilities.
pub struct SubstreamBox(Pin<Box<dyn AsyncReadWrite + Send>>);

#[pin_project]
struct Wrap<T>
where
    T: StreamMuxer,
{
    #[pin]
    inner: T,
}

impl<T> StreamMuxer for Wrap<T>
where
    T: StreamMuxer,
    T::Substream: Send + 'static,
    T::Error: Send + Sync + 'static,
{
    type Substream = SubstreamBox;
    type Error = io::Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.project()
            .inner
            .poll_inbound(cx)
            .map_ok(SubstreamBox::new)
            .map_err(into_io_error)
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.project()
            .inner
            .poll_outbound(cx)
            .map_ok(SubstreamBox::new)
            .map_err(into_io_error)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close(cx).map_err(into_io_error)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        self.project().inner.poll(cx).map_err(into_io_error)
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
        T: StreamMuxer + Send + 'static,
        T::Substream: Send + 'static,
        T::Error: Send + Sync + 'static,
    {
        let wrap = Wrap { inner: muxer };

        StreamMuxerBox {
            inner: Box::pin(wrap),
        }
    }

    fn project(
        self: Pin<&mut Self>,
    ) -> Pin<&mut (dyn StreamMuxer<Substream = SubstreamBox, Error = io::Error> + Send)> {
        self.get_mut().inner.as_mut()
    }
}

impl StreamMuxer for StreamMuxerBox {
    type Substream = SubstreamBox;
    type Error = io::Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.project().poll_inbound(cx)
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.project().poll_outbound(cx)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().poll_close(cx)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        self.project().poll(cx)
    }
}

impl SubstreamBox {
    /// Construct a new [`SubstreamBox`] from something that implements [`AsyncRead`] and [`AsyncWrite`].
    pub fn new<S: AsyncRead + AsyncWrite + Send + 'static>(stream: S) -> Self {
        Self(Box::pin(stream))
    }
}

impl fmt::Debug for SubstreamBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SubstreamBox({})", self.0.type_name())
    }
}

/// Workaround because Rust does not allow `Box<dyn AsyncRead + AsyncWrite>`.
trait AsyncReadWrite: AsyncRead + AsyncWrite {
    /// Helper function to capture the erased inner type.
    ///
    /// Used to make the [`Debug`] implementation of [`SubstreamBox`] more useful.
    fn type_name(&self) -> &'static str;
}

impl<S> AsyncReadWrite for S
where
    S: AsyncRead + AsyncWrite,
{
    fn type_name(&self) -> &'static str {
        std::any::type_name::<S>()
    }
}

impl AsyncRead for SubstreamBox {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        self.0.as_mut().poll_read(cx, buf)
    }

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        self.0.as_mut().poll_read_vectored(cx, bufs)
    }
}

impl AsyncWrite for SubstreamBox {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.0.as_mut().poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        self.0.as_mut().poll_write_vectored(cx, bufs)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.0.as_mut().poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.0.as_mut().poll_close(cx)
    }
}
