#[cfg(feature = "tokio")]
use std::pin::Pin;

use arti_client::DataStream;
use futures::{AsyncRead, AsyncWrite};

pub trait OnionStream: AsyncRead + AsyncWrite + From<DataStream> {}

impl OnionStream for DataStream {}

#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
#[derive(Debug)]
pub struct OnionTokioStream {
    inner: DataStream,
}

#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
impl From<DataStream> for OnionTokioStream {
    fn from(inner: DataStream) -> Self {
        Self { inner }
    }
}

#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
impl OnionStream for OnionTokioStream {}

#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
impl AsyncRead for OnionTokioStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut read_buf = tokio_crate::io::ReadBuf::new(buf);
        futures::ready!(tokio_crate::io::AsyncRead::poll_read(
            Pin::new(&mut self.inner),
            cx,
            &mut read_buf
        ))?;
        std::task::Poll::Ready(Ok(read_buf.filled().len()))
    }
}

#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
impl AsyncWrite for OnionTokioStream {
    #[inline]
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        tokio_crate::io::AsyncWrite::poll_write(Pin::new(&mut self.inner), cx, buf)
    }

    #[inline]
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio_crate::io::AsyncWrite::poll_flush(Pin::new(&mut self.inner), cx)
    }

    #[inline]
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio_crate::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.inner), cx)
    }

    #[inline]
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        tokio_crate::io::AsyncWrite::poll_write_vectored(Pin::new(&mut self.inner), cx, bufs)
    }
}
