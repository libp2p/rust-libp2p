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

//! Wraps around a `Transport` and adds a timeout to all the incoming and outgoing connections.
//!
//! The timeout includes the upgrading process.
// TODO: add example

extern crate futures;
extern crate libp2p_core;
extern crate log;
extern crate tokio_timer;

use futures::{Async, Future, Poll, Stream, try_ready};
use libp2p_core::{Multiaddr, transport::{Dialer, Listener}};
use log::debug;
use std::{fmt, io, time::Duration};
use tokio_timer::Timeout;

/// Wraps around a `Transport` and adds a timeout to all the incoming and outgoing connections.
///
/// The timeout includes the upgrade. There is no timeout on the listener or on stream of incoming
/// substreams.
#[derive(Debug, Copy, Clone)]
pub struct TransportTimeout<T> {
    transport: T,
    timeout: Duration
}

impl<T> TransportTimeout<T> {
    /// Wraps around a `Transport` to add timeouts to all the sockets created by it.
    #[inline]
    pub fn new(transport: T, timeout: Duration) -> Self {
        TransportTimeout { transport, timeout }
    }
}

#[derive(Debug)]
pub enum Error<E> {
    Transport(E),
    Timeout,
    Timer
}

impl<E> fmt::Display for Error<E>
where
    E: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Transport(e) => write!(f, "transport error: {}", e),
            Error::Timeout => f.write_str("timeout"),
            Error::Timer => f.write_str("timer error")
        }
    }
}

impl<E> std::error::Error for Error<E>
where
    E: std::error::Error
{
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            Error::Transport(e) => Some(e),
            Error::Timeout | Error::Timer => None
        }
    }
}


impl<T> Listener for TransportTimeout<T>
where
    T: Listener
{
    type Output = T::Output;
    type Error = Error<T::Error>;
    type Inbound = TimeoutListener<T::Inbound>;
    type Upgrade = TokioTimerMapErr<T::Upgrade>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        match self.transport.listen_on(addr) {
            Ok((listener, addr)) => {
                let listener = TimeoutListener {
                    inner: listener,
                    timeout: self.timeout
                };
                Ok((listener, addr))
            }
            Err((transport, addr)) => {
                let transport = TransportTimeout {
                    transport,
                    timeout: self.timeout
                };
                Err((transport, addr))
            }
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

impl<T> Dialer for TransportTimeout<T>
where
    T: Dialer
{
    type Output = T::Output;
    type Error = Error<T::Error>;
    type Outbound = TokioTimerMapErr<T::Outbound>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        match self.transport.dial(addr) {
            Ok(dial) => Ok(TokioTimerMapErr {
                inner: Timeout::new(dial, self.timeout)
            }),
            Err((transport, addr)) => {
                let transport = TransportTimeout {
                    transport,
                    timeout: self.timeout
                };
                Err((transport, addr))
            }
        }
    }
}

// TODO: can be removed and replaced with an `impl Stream` once impl Trait is fully stable
//       in Rust (https://github.com/rust-lang/rust/issues/34511)
pub struct TimeoutListener<T> {
    inner: T,
    timeout: Duration
}

impl<T, Upgrade> Stream for TimeoutListener<T>
where
    T: Stream<Item = (Upgrade, Multiaddr), Error = io::Error>,
{
    type Item = (TokioTimerMapErr<Upgrade>, Multiaddr);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let poll_out = try_ready!(self.inner.poll());
        if let Some((inner_fut, addr)) = poll_out {
            let fut = TokioTimerMapErr {
                inner: Timeout::new(inner_fut, self.timeout),
            };
            Ok(Async::Ready(Some((fut, addr))))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

/// Wraps around a `Future`. Turns the error type from `TimeoutError<IoError>` to `IoError`.
// TODO: can be replaced with `impl Future` once `impl Trait` are fully stable in Rust
//       (https://github.com/rust-lang/rust/issues/34511)
#[must_use = "futures do nothing unless polled"]
pub struct TokioTimerMapErr<T> { inner: Timeout<T> }

impl<T> Future for TokioTimerMapErr<T>
where
    T: Future
{
    type Item = T::Item;
    type Error = Error<T::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll().map_err(|err| {
            if err.is_inner() {
                Error::Transport(err.into_inner().expect("ensured by is_inner()"))
            } else if err.is_elapsed() {
                debug!("timeout elapsed for connection");
                Error::Timeout
            } else {
                assert!(err.is_timer());
                debug!("tokio timer error in timeout wrapper");
                Error::Timer
            }
        })
    }
}
