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

//! The interface for providers of non-blocking TCP implementations.

#[cfg(feature = "async-io")]
pub mod async_io;

#[cfg(feature = "tokio")]
pub mod tokio;

use futures::future::BoxFuture;
use futures::io::{AsyncRead, AsyncWrite};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::task::{Context, Poll};
use std::{fmt, io};

/// An incoming connection returned from [`Provider::poll_accept()`].
pub struct Incoming<S> {
    pub stream: S,
    pub local_addr: SocketAddr,
    pub remote_addr: SocketAddr,
}

/// The interface for non-blocking TCP I/O providers.
pub trait Provider: Clone + Send + 'static {
    /// The type of TCP streams obtained from [`Provider::new_stream`]
    /// and [`Provider::poll_accept`].
    type Stream: AsyncRead + AsyncWrite + Send + Unpin + fmt::Debug;
    /// The type of TCP listeners obtained from [`Provider::new_listener`].
    type Listener: Send + Unpin;

    /// Creates a new listener wrapping the given [`TcpListener`] that
    /// can be polled for incoming connections via [`Self::poll_accept()`].
    fn new_listener(_: TcpListener) -> io::Result<Self::Listener>;

    /// Creates a new stream for an outgoing connection, wrapping the
    /// given [`TcpStream`]. The given `TcpStream` is initiating a
    /// connection, but implementations must wait for the connection
    /// setup to complete, i.e. for the stream to be writable.
    fn new_stream(_: TcpStream) -> BoxFuture<'static, io::Result<Self::Stream>>;

    /// Polls a [`Self::Listener`] for an incoming connection, ensuring a task wakeup,
    /// if necessary.
    fn poll_accept(
        _: &mut Self::Listener,
        _: &mut Context<'_>,
    ) -> Poll<io::Result<Incoming<Self::Stream>>>;
}
