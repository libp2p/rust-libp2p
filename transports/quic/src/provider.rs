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

use futures::future::BoxFuture;
use if_watch::IfEvent;
use std::{
    io,
    net::{SocketAddr, UdpSocket},
    task::{Context, Poll},
    time::Duration,
};

#[cfg(feature = "async-std")]
pub mod async_std;
#[cfg(feature = "tokio")]
pub mod tokio;

pub enum Runtime {
    #[cfg(feature = "tokio")]
    Tokio,
    #[cfg(feature = "async-std")]
    AsyncStd,
    Dummy,
}

/// Provider for a corresponding quinn runtime and spawning tasks.
pub trait Provider: Unpin + Send + Sized + 'static {
    type IfWatcher: Unpin + Send;

    /// Run the corresponding runtime
    fn runtime() -> Runtime;

    /// Create a new [`if_watch`] watcher that reports [`IfEvent`]s for network interface changes.
    fn new_if_watcher() -> io::Result<Self::IfWatcher>;

    /// Poll for an address change event.
    fn poll_if_event(
        watcher: &mut Self::IfWatcher,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<IfEvent>>;

    /// Sleep for specified amount of time.
    fn sleep(duration: Duration) -> BoxFuture<'static, ()>;

    /// Sends data on the socket to the given address. On success, returns the number of bytes written.
    fn send_to<'a>(
        udp_socket: &'a UdpSocket,
        buf: &'a [u8],
        target: SocketAddr,
    ) -> BoxFuture<'a, io::Result<usize>>;
}
