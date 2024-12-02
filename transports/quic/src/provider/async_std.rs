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
    net::UdpSocket,
    task::{Context, Poll},
    time::Duration,
};

use futures::{future::BoxFuture, FutureExt};

use crate::GenTransport;

/// Transport with [`async-std`] runtime.
pub type Transport = GenTransport<Provider>;

/// Provider for quinn runtime and spawning tasks using [`async-std`].
pub struct Provider;

impl super::Provider for Provider {
    type IfWatcher = if_watch::smol::IfWatcher;

    fn runtime() -> super::Runtime {
        super::Runtime::AsyncStd
    }

    fn new_if_watcher() -> io::Result<Self::IfWatcher> {
        if_watch::smol::IfWatcher::new()
    }

    fn poll_if_event(
        watcher: &mut Self::IfWatcher,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<if_watch::IfEvent>> {
        watcher.poll_if_event(cx)
    }

    fn sleep(duration: Duration) -> BoxFuture<'static, ()> {
        async_std::task::sleep(duration).boxed()
    }

    fn send_to<'a>(
        udp_socket: &'a UdpSocket,
        buf: &'a [u8],
        target: std::net::SocketAddr,
    ) -> BoxFuture<'a, io::Result<usize>> {
        Box::pin(async move {
            async_std::net::UdpSocket::from(udp_socket.try_clone()?)
                .send_to(buf, target)
                .await
        })
    }
}
