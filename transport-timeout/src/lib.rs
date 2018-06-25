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

#[macro_use]
extern crate futures;
extern crate libp2p_core;
#[macro_use]
extern crate log;
extern crate tokio_timer;

use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::time::{Duration, Instant};
use futures::{Future, Poll, Async, Stream};
use libp2p_core::{Transport, Multiaddr, MuxedTransport};
use tokio_timer::{Deadline, DeadlineError};

/// Wraps around a `Transport` and adds a timeout to all the incoming and outgoing connections.
///
/// The timeout includes the upgrade. There is no timeout on the listener or on stream of incoming
/// substreams.
#[derive(Debug, Copy, Clone)]
pub struct TransportTimeout<InnerTrans> {
    inner: InnerTrans,
    outgoing_timeout: Duration,
    incoming_timeout: Duration,
}

impl<InnerTrans> TransportTimeout<InnerTrans> {
    /// Wraps around a `Transport` to add timeouts to all the sockets created by it.
    #[inline]
    pub fn new(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: timeout,
            incoming_timeout: timeout,
        }
    }

    /// Wraps around a `Transport` to add timeouts to the outgoing connections.
    #[inline]
    pub fn with_outgoing_timeout(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: timeout,
            incoming_timeout: Duration::from_secs(100 * 365 * 24 * 3600),   // 100 years
        }
    }

    /// Wraps around a `Transport` to add timeouts to the ingoing connections.
    #[inline]
    pub fn with_ingoing_timeout(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: Duration::from_secs(100 * 365 * 24 * 3600),   // 100 years
            incoming_timeout: timeout,
        }
    }
}

impl<InnerTrans> Transport for TransportTimeout<InnerTrans>
where InnerTrans: Transport,
{
    type Output = InnerTrans::Output;
    type MultiaddrFuture = InnerTrans::MultiaddrFuture;
    type Listener = TimeoutListener<InnerTrans::Listener>;
    type ListenerUpgrade = TokioTimerMapErr<Deadline<InnerTrans::ListenerUpgrade>>;
    type Dial = TokioTimerMapErr<Deadline<InnerTrans::Dial>>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        match self.inner.listen_on(addr) {
            Ok((listener, addr)) => {
                let listener = TimeoutListener {
                    inner: listener,
                    timeout: self.incoming_timeout,
                };

                Ok((listener, addr))
            },
            Err((inner, addr)) => {
                let transport = TransportTimeout {
                    inner,
                    outgoing_timeout: self.outgoing_timeout,
                    incoming_timeout: self.incoming_timeout,
                };

                Err((transport, addr))
            }
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match self.inner.dial(addr) {
            Ok(dial) => {
                Ok(TokioTimerMapErr {
                    inner: Deadline::new(dial, Instant::now() + self.outgoing_timeout)
                })
            },
            Err((inner, addr)) => {
                let transport = TransportTimeout {
                    inner,
                    outgoing_timeout: self.outgoing_timeout,
                    incoming_timeout: self.incoming_timeout,
                };

                Err((transport, addr))
            }
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}

impl<InnerTrans> MuxedTransport for TransportTimeout<InnerTrans>
where InnerTrans: MuxedTransport
{
    type Incoming = TimeoutIncoming<InnerTrans::Incoming>;
    type IncomingUpgrade = TokioTimerMapErr<Deadline<InnerTrans::IncomingUpgrade>>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        TimeoutIncoming {
            inner: self.inner.next_incoming(),
            timeout: self.incoming_timeout,
        }
    }
}

// TODO: can be removed and replaced with an `impl Stream` once impl Trait is fully stable
//       in Rust (https://github.com/rust-lang/rust/issues/34511)
pub struct TimeoutListener<InnerStream> {
    inner: InnerStream,
    timeout: Duration,
}

impl<InnerStream> Stream for TimeoutListener<InnerStream>
where InnerStream: Stream,
{
    type Item = TokioTimerMapErr<Deadline<InnerStream::Item>>;
    type Error = InnerStream::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let inner_fut = try_ready!(self.inner.poll());
        if let Some(inner_fut) = inner_fut {
            let fut = TokioTimerMapErr {
                inner: Deadline::new(inner_fut, Instant::now() + self.timeout)
            };
            Ok(Async::Ready(Some(fut)))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

// TODO: can be removed and replaced with an `impl Future` once impl Trait is fully stable
//       in Rust (https://github.com/rust-lang/rust/issues/34511)
pub struct TimeoutIncoming<InnerFut> {
    inner: InnerFut,
    timeout: Duration,
}

impl<InnerFut> Future for TimeoutIncoming<InnerFut>
where InnerFut: Future,
{
    type Item = TokioTimerMapErr<Deadline<InnerFut::Item>>;
    type Error = InnerFut::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let inner_fut = try_ready!(self.inner.poll());
        let fut = TokioTimerMapErr {
            inner: Deadline::new(inner_fut, Instant::now() + self.timeout)
        };
        Ok(Async::Ready(fut))
    }
}

/// Wraps around a `Future`. Turns the error type from `DeadlineError<IoError>` to `IoError`.
// TODO: can be replaced with `impl Future` once `impl Trait` are fully stable in Rust
//       (https://github.com/rust-lang/rust/issues/34511)
pub struct TokioTimerMapErr<InnerFut> {
    inner: InnerFut,
}

impl<InnerFut> Future for TokioTimerMapErr<InnerFut>
where InnerFut: Future<Error = DeadlineError<IoError>>
{
    type Item = InnerFut::Item;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll()
            .map_err(|err: DeadlineError<IoError>| {
                if err.is_inner() {
                    err.into_inner().expect("ensured by is_inner()")
                } else if err.is_elapsed() {
                    debug!("timeout elapsed for connection");
                    IoErrorKind::TimedOut.into()
                } else {
                    assert!(err.is_timer());
                    debug!("tokio timer error in timeout wrapper");
                    let err = err.into_timer().expect("ensure by is_timer()");
                    IoError::new(IoErrorKind::Other, err)
                }
            })
    }
}
