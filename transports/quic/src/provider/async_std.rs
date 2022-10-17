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

use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use async_std::{net::UdpSocket, task::spawn};
use futures::{future::BoxFuture, ready, Future, FutureExt, Stream, StreamExt};

use crate::GenTransport;

use super::Provider as ProviderTrait;

pub type Transport = GenTransport<Provider>;

pub struct Provider {
    socket: Arc<UdpSocket>,
    send_packet: Option<BoxFuture<'static, Result<(), io::Error>>>,
    recv_stream: ReceiveStream,
}

impl ProviderTrait for Provider {
    fn from_socket(socket: std::net::UdpSocket) -> io::Result<Self> {
        let socket = Arc::new(socket.into());
        let recv_stream = ReceiveStream::new(Arc::clone(&socket));
        Ok(Provider {
            socket,
            send_packet: None,
            recv_stream,
        })
    }

    fn poll_recv_from(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<(Vec<u8>, SocketAddr)>> {
        match self.recv_stream.poll_next_unpin(cx) {
            Poll::Ready(ready) => {
                Poll::Ready(ready.expect("ReceiveStream::poll_next never returns None."))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn start_send(&mut self, data: Vec<u8>, addr: SocketAddr) {
        let _len = data.len();
        let socket = self.socket.clone();
        let send = async move {
            let _send_len = socket.send_to(&data, addr).await?;
            Ok(())
        }
        .boxed();
        self.send_packet = Some(send)
    }

    fn poll_send_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let pending = match self.send_packet.as_mut() {
            Some(pending) => pending,
            None => return Poll::Ready(Ok(())),
        };
        match pending.poll_unpin(cx) {
            Poll::Ready(result) => {
                self.send_packet = None;
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn spawn(future: impl Future<Output = ()> + Send + 'static) {
        spawn(future);
    }
}

/// Wrapper around the socket to implement `Stream` on it.
struct ReceiveStream {
    fut: BoxFuture<
        'static,
        (
            Result<(usize, SocketAddr), io::Error>,
            Arc<UdpSocket>,
            Vec<u8>,
        ),
    >,
}

impl ReceiveStream {
    fn new(socket: Arc<UdpSocket>) -> Self {
        let mut socket_recv_buffer = vec![0; 65536];
        let fut = async move {
            let recv = socket.recv_from(&mut socket_recv_buffer).await;
            (recv, socket, socket_recv_buffer)
        };
        Self { fut: fut.boxed() }
    }
}

impl Stream for ReceiveStream {
    type Item = Result<(Vec<u8>, SocketAddr), io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let (result, socket, mut buffer) = ready!(self.fut.poll_unpin(cx));
        let result = result.map(|(packet_len, packet_src)| {
            debug_assert!(packet_len <= buffer.len());
            // Copies the bytes from the `socket_recv_buffer` they were written into.
            (buffer[..packet_len].into(), packet_src)
        });
        self.fut = async move {
            let recv = socket.recv_from(&mut buffer).await;
            (recv, socket, buffer)
        }
        .boxed();
        Poll::Ready(Some(result))
    }
}
