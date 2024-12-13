// Copyright 2018 Parity Technologies (UK) Ltd.
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
    io::Error,
    net::{SocketAddr, UdpSocket},
    task::{Context, Poll},
};

/// Interface that must be implemented by the different runtimes to use the [`UdpSocket`] in async
/// mode
#[allow(unreachable_pub)] // Users should not depend on this.
pub trait AsyncSocket: Unpin + Send + 'static {
    /// Create the async socket from the [`std::net::UdpSocket`]
    fn from_std(socket: UdpSocket) -> std::io::Result<Self>
    where
        Self: Sized;

    /// Attempts to receive a single packet on the socket
    /// from the remote address to which it is connected.
    fn poll_read(
        &mut self,
        _cx: &mut Context,
        _buf: &mut [u8],
    ) -> Poll<Result<(usize, SocketAddr), Error>>;

    /// Attempts to send data on the socket to a given address.
    fn poll_write(
        &mut self,
        _cx: &mut Context,
        _packet: &[u8],
        _to: SocketAddr,
    ) -> Poll<Result<(), Error>>;
}

#[cfg(feature = "async-io")]
pub(crate) mod asio {
    use async_io::Async;
    use futures::FutureExt;

    use super::*;

    /// AsyncIo UdpSocket
    pub(crate) type AsyncUdpSocket = Async<UdpSocket>;
    impl AsyncSocket for AsyncUdpSocket {
        fn from_std(socket: UdpSocket) -> std::io::Result<Self> {
            Async::new(socket)
        }

        fn poll_read(
            &mut self,
            cx: &mut Context,
            buf: &mut [u8],
        ) -> Poll<Result<(usize, SocketAddr), Error>> {
            // Poll receive socket.
            futures::ready!(self.poll_readable(cx))?;
            match self.recv_from(buf).now_or_never() {
                Some(data) => Poll::Ready(data),
                None => Poll::Pending,
            }
        }

        fn poll_write(
            &mut self,
            cx: &mut Context,
            packet: &[u8],
            to: SocketAddr,
        ) -> Poll<Result<(), Error>> {
            futures::ready!(self.poll_writable(cx))?;
            match self.send_to(packet, to).now_or_never() {
                Some(Ok(_)) => Poll::Ready(Ok(())),
                Some(Err(err)) => Poll::Ready(Err(err)),
                None => Poll::Pending,
            }
        }
    }
}

#[cfg(feature = "tokio")]
pub(crate) mod tokio {
    use ::tokio::{io::ReadBuf, net::UdpSocket as TkUdpSocket};

    use super::*;

    /// Tokio ASync Socket`
    pub(crate) type TokioUdpSocket = TkUdpSocket;
    impl AsyncSocket for TokioUdpSocket {
        fn from_std(socket: UdpSocket) -> std::io::Result<Self> {
            socket.set_nonblocking(true)?;
            TokioUdpSocket::from_std(socket)
        }

        fn poll_read(
            &mut self,
            cx: &mut Context,
            buf: &mut [u8],
        ) -> Poll<Result<(usize, SocketAddr), Error>> {
            let mut rbuf = ReadBuf::new(buf);
            match self.poll_recv_from(cx, &mut rbuf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                Poll::Ready(Ok(addr)) => Poll::Ready(Ok((rbuf.filled().len(), addr))),
            }
        }

        fn poll_write(
            &mut self,
            cx: &mut Context,
            packet: &[u8],
            to: SocketAddr,
        ) -> Poll<Result<(), Error>> {
            match self.poll_send_to(cx, packet, to) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                Poll::Ready(Ok(_len)) => Poll::Ready(Ok(())),
            }
        }
    }
}
