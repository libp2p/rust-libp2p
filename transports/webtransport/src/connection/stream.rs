use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use wtransport::{RecvStream, SendStream};

/// A single stream on a connection
pub struct Stream {
    /// A send part of the stream
    send: SendStream,
    /// A reception part of the stream
    recv: RecvStream,
    /// Whether the stream is closed or not
    close_result: Option<Result<(), io::ErrorKind>>,
}

impl Stream {
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self {
            send,
            recv,
            close_result: None,
        }
    }
}

impl futures::AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if let Some(close_result) = self.close_result {
            if close_result.is_err() {
                return Poll::Ready(Ok(0));
            }
        }
        let mut read_buf = ReadBuf::new(buf);
        AsyncRead::poll_read(Pin::new(&mut self.recv), cx, &mut read_buf)
            .map_ok(|()| read_buf.filled().len())
    }
}

impl futures::AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.send), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.send), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(close_result) = self.close_result {
            // For some reason poll_close needs to be 'fuse'able
            return Poll::Ready(close_result.map_err(Into::into));
        }
        let close_result = futures::ready!(AsyncWrite::poll_shutdown(Pin::new(&mut self.send), cx));
        self.close_result = Some(close_result.as_ref().map_err(|e| e.kind()).copied());
        Poll::Ready(close_result)
    }
}
