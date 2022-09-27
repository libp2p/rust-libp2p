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

use std::{io, net::SocketAddr};

use futures::Future;
use tokio_crate::net::UdpSocket;

use crate::QuicTransport;

use super::Provider;

pub type TokioTransport = QuicTransport<Tokio>;
pub struct Tokio;

#[async_trait::async_trait]
impl Provider for Tokio {
    type Socket = UdpSocket;

    fn from_socket(socket: std::net::UdpSocket) -> std::io::Result<Self::Socket> {
        UdpSocket::from_std(socket)
    }

    async fn recv_from(socket: &Self::Socket, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        socket.recv_from(buf).await
    }

    async fn send_to(socket: &Self::Socket, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        socket.send_to(buf, addr).await
    }

    fn spawn(future: impl Future<Output = ()> + Send + 'static) {
        tokio_crate::spawn(future);
    }
}
