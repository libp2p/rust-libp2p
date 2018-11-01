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

//! Provides the `TransportExt` trait.

use ratelimit::RateLimited;
use std::io;
use std::time::Duration;
use tokio_executor::DefaultExecutor;
use transport_timeout::TransportTimeout;
use Transport;

/// Trait automatically implemented on all objects that implement `Transport`. Provides some
/// additional utilities.
///
/// # Example
///
/// ```
/// use libp2p::TransportExt;
/// use libp2p::tcp::TcpConfig;
/// use std::time::Duration;
///
/// let _transport = TcpConfig::new()
///     .with_timeout(Duration::from_secs(20))
///     .with_rate_limit(1024 * 1024, 1024 * 1024);
/// ```
///
pub trait TransportExt: Transport {
    /// Adds a timeout to the connection and upgrade steps for all the sockets created by
    /// the transport.
    #[inline]
    fn with_timeout(self, timeout: Duration) -> TransportTimeout<Self>
    where
        Self: Sized,
    {
        TransportTimeout::new(self, timeout)
    }

    /// Adds a timeout to the connection and upgrade steps for all the outgoing sockets created
    /// by the transport.
    #[inline]
    fn with_outbound_timeout(self, timeout: Duration) -> TransportTimeout<Self>
    where
        Self: Sized,
    {
        TransportTimeout::with_outgoing_timeout(self, timeout)
    }

    /// Adds a timeout to the connection and upgrade steps for all the incoming sockets created
    /// by the transport.
    #[inline]
    fn with_inbound_timeout(self, timeout: Duration) -> TransportTimeout<Self>
    where
        Self: Sized,
    {
        TransportTimeout::with_ingoing_timeout(self, timeout)
    }

    /// Adds a maximum transfer rate to the sockets created with the transport.
    #[inline]
    fn with_rate_limit(
        self,
        max_read_bytes_per_sec: usize,
        max_write_bytes_per_sec: usize,
    ) -> io::Result<RateLimited<Self>>
    where
        Self: Sized,
    {
        RateLimited::new(
            &mut DefaultExecutor::current(),
            self,
            max_read_bytes_per_sec,
            max_write_bytes_per_sec,
        )
    }

    // TODO: add methods to easily upgrade for secio/mplex/yamux
}

impl<TTransport> TransportExt for TTransport where TTransport: Transport {}
