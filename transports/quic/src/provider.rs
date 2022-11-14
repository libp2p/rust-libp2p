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

use futures::Future;
use if_watch::IfEvent;
use std::{
    io,
    net::SocketAddr,
    task::{Context, Poll},
};

#[cfg(feature = "async-std")]
pub mod async_std;
#[cfg(feature = "tokio")]
pub mod tokio;

/// Size of the buffer for reading data 0x10000.
const RECEIVE_BUFFER_SIZE: usize = 65536;

/// Provider for non-blocking receiving and sending on a [`std::net::UdpSocket`]
/// and spawning tasks.
pub trait Provider: Unpin + Send + Sized + 'static {
    type IfWatcher: Unpin + Send;

    /// Create a new providing that is wrapping the socket.
    ///
    /// Note: The socket must be set to non-blocking.
    fn from_socket(socket: std::net::UdpSocket) -> io::Result<Self>;

    /// Receive a single packet.
    ///
    /// Returns the message and the address the message came from.
    fn poll_recv_from(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<(Vec<u8>, SocketAddr)>>;

    /// Set sending a packet on the socket.
    ///
    /// Since only one packet can be sent at a time, this may only be called if a preceding
    /// call to [`Provider::poll_send_flush`] returned [`Poll::Ready`].
    fn start_send(&mut self, data: Vec<u8>, addr: SocketAddr);

    /// Flush a packet send in [`Provider::start_send`].
    ///
    /// If [`Poll::Ready`] is returned the socket is ready for sending a new packet.
    fn poll_send_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>>;

    /// Run the given future in the background until it ends.
    ///
    /// This is used to spawn the task that is driving the endpoint.
    fn spawn(future: impl Future<Output = ()> + Send + 'static);

    /// Create a new [`if_watch`] watcher that reports [`IfEvent`]s for network interface changes.
    fn new_if_watcher() -> io::Result<Self::IfWatcher>;

    /// Poll for an address change event.
    fn poll_if_event(
        watcher: &mut Self::IfWatcher,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<IfEvent>>;
}
