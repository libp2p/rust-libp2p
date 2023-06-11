// Copyright 2022 Protocol Labs.
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

use futures::{future::BoxFuture, ready, Future, FutureExt};
use std::{
    io,
    net::SocketAddr,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{io::ReadBuf, net::UdpSocket};

use crate::GenTransport;

/// Transport with [`tokio`] runtime.
pub type Transport = GenTransport<Provider>;

/// Provider for reading / writing to a sockets and spawning
/// tasks using [`tokio`].
pub struct Provider {
    socket: UdpSocket,
    socket_recv_buffer: Vec<u8>,
    next_packet_out: Option<(Vec<u8>, SocketAddr)>,
}

impl super::Provider for Provider {
    type IfWatcher = if_watch::tokio::IfWatcher;

    fn from_socket(socket: std::net::UdpSocket) -> std::io::Result<Self> {
        let socket = UdpSocket::from_std(socket)?;
        Ok(Provider {
            socket,
            socket_recv_buffer: vec![0; super::RECEIVE_BUFFER_SIZE],
            next_packet_out: None,
        })
    }

    fn poll_send_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let (data, addr) = match self.next_packet_out.as_ref() {
            Some(pending) => pending,
            None => return Poll::Ready(Ok(())),
        };
        match self.socket.poll_send_to(cx, data.as_slice(), *addr) {
            Poll::Ready(result) => {
                self.next_packet_out = None;
                Poll::Ready(result.map(|_| ()))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_recv_from(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<(Vec<u8>, SocketAddr)>> {
        let Self {
            socket,
            socket_recv_buffer,
            ..
        } = self;
        let mut read_buf = ReadBuf::new(socket_recv_buffer.as_mut_slice());
        let packet_src = ready!(socket.poll_recv_from(cx, &mut read_buf)?);
        let bytes = read_buf.filled().to_vec();
        Poll::Ready(Ok((bytes, packet_src)))
    }

    fn start_send(&mut self, data: Vec<u8>, addr: SocketAddr) {
        self.next_packet_out = Some((data, addr));
    }

    fn spawn(future: impl Future<Output = ()> + Send + 'static) {
        tokio::spawn(future);
    }

    fn new_if_watcher() -> io::Result<Self::IfWatcher> {
        if_watch::tokio::IfWatcher::new()
    }

    fn poll_if_event(
        watcher: &mut Self::IfWatcher,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<if_watch::IfEvent>> {
        watcher.poll_if_event(cx)
    }

    fn sleep(duration: Duration) -> BoxFuture<'static, ()> {
        tokio::time::sleep(duration).boxed()
    }
}
