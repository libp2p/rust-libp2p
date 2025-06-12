// Copyright 2020 Parity Technologies (UK) Ltd.
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

use std::{
    io,
    os::unix::net,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    future::{BoxFuture, FutureExt},
    prelude::*,
};

use super::{Incoming, Provider};

/// A UNIX-domain stream [`Transport`](libp2p_core::Transport) that works with the `tokio`
/// ecosystem.
///
/// # Example
///
/// ```rust
/// # use libp2p_unix_stream as unix_stream;
/// # use libp2p_core::{Transport, transport::ListenerId};
/// # use futures::future;
/// # use std::pin::Pin;
/// #
/// # #[tokio::main]
/// # async fn main() {
/// let mut transport = unix_stream::tokio::Transport::new();
/// let _ = std::fs::remove_file("socket");
/// let id = transport
///     .listen_on(ListenerId::next(), "/unix/socket".parse().unwrap())
///     .unwrap();
///
/// let addr = future::poll_fn(|cx| Pin::new(&mut transport).poll(cx))
///     .await
///     .into_new_address()
///     .unwrap();
///
/// println!("Listening on {addr}");
/// # let _ = std::fs::remove_file("socket");
/// # }
/// ```
pub type Transport = crate::Transport<UnixStrm>;

#[derive(Copy, Clone)]
#[doc(hidden)]
pub enum UnixStrm {}

impl Provider for UnixStrm {
    type Stream = UnixStream;
    type Listener = tokio::net::UnixListener;

    fn new_listener(l: net::UnixListener) -> io::Result<Self::Listener> {
        tokio::net::UnixListener::try_from(l)
    }

    fn new_stream(s: net::UnixStream) -> BoxFuture<'static, io::Result<Self::Stream>> {
        async move {
            // Taken from [`tokio::net::UnixStream::connect_mio`].

            let stream = tokio::net::UnixStream::try_from(s)?;

            // Once we've connected, wait for the stream to be writable as
            // that's when the actual connection has been initiated. Once we're
            // writable we check for `take_socket_error` to see if the connect
            // actually hit an error or not.
            //
            // If all that succeeded then we ship everything on up.
            stream.writable().await?;

            if let Some(e) = stream.take_error()? {
                return Err(e);
            }

            Ok(UnixStream(stream))
        }
        .boxed()
    }

    fn poll_accept(
        l: &mut Self::Listener,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Incoming<Self::Stream>>> {
        let (stream, remote_addr) = match l.poll_accept(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok((stream, remote_addr))) => (stream, remote_addr.into()),
        };

        let local_addr = stream.local_addr()?.into();
        let stream = UnixStream(stream);

        Poll::Ready(Ok(Incoming {
            stream,
            local_addr,
            remote_addr,
        }))
    }
}

/// A [`tokio::net::UnixStream`] that implements [`AsyncRead`] and [`AsyncWrite`].
#[derive(Debug)]
pub struct UnixStream(pub tokio::net::UnixStream);

impl From<UnixStream> for tokio::net::UnixStream {
    fn from(t: UnixStream) -> tokio::net::UnixStream {
        t.0
    }
}

impl AsyncRead for UnixStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut read_buf = tokio::io::ReadBuf::new(buf);
        futures::ready!(tokio::io::AsyncRead::poll_read(
            Pin::new(&mut self.0),
            cx,
            &mut read_buf
        ))?;
        Poll::Ready(Ok(read_buf.filled().len()))
    }
}

impl AsyncWrite for UnixStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        tokio::io::AsyncWrite::poll_write(Pin::new(&mut self.0), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio::io::AsyncWrite::poll_flush(Pin::new(&mut self.0), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.0), cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        tokio::io::AsyncWrite::poll_write_vectored(Pin::new(&mut self.0), cx, bufs)
    }
}
