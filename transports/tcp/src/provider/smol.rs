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
    io, net,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    future::{BoxFuture, FutureExt},
    prelude::*,
};

use super::{Incoming, Provider};

/// A TCP [`Transport`](libp2p_core::Transport) that works with the `smol` ecosystem.
///
/// # Example
///
/// ```rust
/// # use libp2p_tcp as tcp;
/// # use libp2p_core::{Transport, transport::ListenerId};
/// # use futures::future;
/// # use std::pin::Pin;
/// #
/// # fn main() {
/// # smol::block_on(async {
/// let mut transport = tcp::smol::Transport::new(tcp::Config::default());
/// let id = transport
///     .listen_on(ListenerId::next(), "/ip4/127.0.0.1/tcp/0".parse().unwrap())
///     .unwrap();
///
/// let addr = future::poll_fn(|cx| Pin::new(&mut transport).poll(cx))
///     .await
///     .into_new_address()
///     .unwrap();
///
/// println!("Listening on {addr}");
/// # });
/// # }
/// ```
pub type Transport = crate::Transport<Tcp>;

#[derive(Copy, Clone)]
#[doc(hidden)]
pub enum Tcp {}

impl Provider for Tcp {
    type Stream = TcpStream;
    type Listener = async_io::Async<net::TcpListener>;
    type IfWatcher = if_watch::smol::IfWatcher;

    fn new_if_watcher() -> io::Result<Self::IfWatcher> {
        Self::IfWatcher::new()
    }

    fn addrs(if_watcher: &Self::IfWatcher) -> Vec<if_watch::IpNet> {
        if_watcher.iter().copied().collect()
    }

    fn new_listener(l: net::TcpListener) -> io::Result<Self::Listener> {
        async_io::Async::new(l)
    }

    fn new_stream(s: net::TcpStream) -> BoxFuture<'static, io::Result<Self::Stream>> {
        async move {
            let stream = async_io::Async::new(s)?;

            // Wait for the stream to be writable as that's when the actual
            // connection has been initiated.
            stream.writable().await?;

            if let Some(e) = stream.get_ref().take_error()? {
                return Err(e);
            }

            Ok(TcpStream(stream))
        }
        .boxed()
    }

    fn poll_accept(
        l: &mut Self::Listener,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Incoming<Self::Stream>>> {
        match l.poll_readable(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }

        match l.get_ref().accept() {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
            Ok((stream, remote_addr)) => {
                let local_addr = stream.local_addr()?;
                let stream = async_io::Async::new(stream)?;

                Poll::Ready(Ok(Incoming {
                    stream: TcpStream(stream),
                    local_addr,
                    remote_addr,
                }))
            }
        }
    }
}

/// A TCP stream wrapped in [`async_io::Async`] that implements [`AsyncRead`] and [`AsyncWrite`].
#[derive(Debug)]
pub struct TcpStream(pub async_io::Async<net::TcpStream>);

impl From<TcpStream> for async_io::Async<net::TcpStream> {
    fn from(t: TcpStream) -> async_io::Async<net::TcpStream> {
        t.0
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.0).poll_close(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write_vectored(cx, bufs)
    }
}
