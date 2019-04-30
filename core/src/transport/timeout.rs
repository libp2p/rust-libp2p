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

//! Transports with timeouts on the connection setup.
//!
//! The connection setup includes all protocol upgrades applied on the
//! underlying `Transport`.
// TODO: add example

use crate::{Multiaddr, Transport, transport::{TransportError, ListenerEvent}};
use futures::{try_ready, Async, Future, Poll, Stream};
use log::debug;
use std::{error, fmt, time::Duration};
use wasm_timer::Timeout;
use wasm_timer::timeout::Error as TimeoutError;

/// A `TransportTimeout` is a `Transport` that wraps another `Transport` and adds
/// timeouts to all inbound and outbound connection attempts.
///
/// **Note**: `listen_on` is never subject to a timeout, only the setup of each
/// individual accepted connection.
#[derive(Debug, Copy, Clone)]
pub struct TransportTimeout<InnerTrans> {
    inner: InnerTrans,
    outgoing_timeout: Duration,
    incoming_timeout: Duration,
}

impl<InnerTrans> TransportTimeout<InnerTrans> {
    /// Wraps around a `Transport` to add timeouts to all the sockets created by it.
    pub fn new(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: timeout,
            incoming_timeout: timeout,
        }
    }

    /// Wraps around a `Transport` to add timeouts to the outgoing connections.
    pub fn with_outgoing_timeout(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: timeout,
            incoming_timeout: Duration::from_secs(100 * 365 * 24 * 3600), // 100 years
        }
    }

    /// Wraps around a `Transport` to add timeouts to the ingoing connections.
    pub fn with_ingoing_timeout(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: Duration::from_secs(100 * 365 * 24 * 3600), // 100 years
            incoming_timeout: timeout,
        }
    }
}

impl<InnerTrans> Transport for TransportTimeout<InnerTrans>
where
    InnerTrans: Transport,
    InnerTrans::Error: 'static,
{
    type Output = InnerTrans::Output;
    type Error = TransportTimeoutError<InnerTrans::Error>;
    type Listener = TimeoutListener<InnerTrans::Listener>;
    type ListenerUpgrade = TokioTimerMapErr<Timeout<InnerTrans::ListenerUpgrade>>;
    type Dial = TokioTimerMapErr<Timeout<InnerTrans::Dial>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let listener = self.inner.listen_on(addr)
            .map_err(|err| err.map(TransportTimeoutError::Other))?;

        let listener = TimeoutListener {
            inner: listener,
            timeout: self.incoming_timeout,
        };

        Ok(listener)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dial = self.inner.dial(addr)
            .map_err(|err| err.map(TransportTimeoutError::Other))?;
        Ok(TokioTimerMapErr {
            inner: Timeout::new(dial, self.outgoing_timeout),
        })
    }
}

// TODO: can be removed and replaced with an `impl Stream` once impl Trait is fully stable
//       in Rust (https://github.com/rust-lang/rust/issues/34511)
pub struct TimeoutListener<InnerStream> {
    inner: InnerStream,
    timeout: Duration,
}

impl<InnerStream, O> Stream for TimeoutListener<InnerStream>
where
    InnerStream: Stream<Item = ListenerEvent<O>>
{
    type Item = ListenerEvent<TokioTimerMapErr<Timeout<O>>>;
    type Error = TransportTimeoutError<InnerStream::Error>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let poll_out = try_ready!(self.inner.poll().map_err(TransportTimeoutError::Other));
        if let Some(event) = poll_out {
            let event = event.map(move |inner_fut| {
                TokioTimerMapErr { inner: Timeout::new(inner_fut, self.timeout) }
            });
            Ok(Async::Ready(Some(event)))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

/// Wraps around a `Future`. Turns the error type from `TimeoutError<Err>` to
/// `TransportTimeoutError<Err>`.
// TODO: can be replaced with `impl Future` once `impl Trait` are fully stable in Rust
//       (https://github.com/rust-lang/rust/issues/34511)
#[must_use = "futures do nothing unless polled"]
pub struct TokioTimerMapErr<InnerFut> {
    inner: InnerFut,
}

impl<InnerFut, TErr> Future for TokioTimerMapErr<InnerFut>
where
    InnerFut: Future<Error = TimeoutError<TErr>>,
{
    type Item = InnerFut::Item;
    type Error = TransportTimeoutError<TErr>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll().map_err(|err: TimeoutError<TErr>| {
            if err.is_inner() {
                TransportTimeoutError::Other(err.into_inner().expect("ensured by is_inner()"))
            } else if err.is_elapsed() {
                debug!("timeout elapsed for connection");
                TransportTimeoutError::Timeout
            } else {
                assert!(err.is_timer());
                debug!("tokio timer error in timeout wrapper");
                TransportTimeoutError::TimerError
            }
        })
    }
}

/// Error that can be produced by the `TransportTimeout` layer.
#[derive(Debug, Copy, Clone)]
pub enum TransportTimeoutError<TErr> {
    /// The transport timed out.
    Timeout,
    /// An error happened in the timer.
    TimerError,
    /// Other kind of error.
    Other(TErr),
}

impl<TErr> fmt::Display for TransportTimeoutError<TErr>
where TErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportTimeoutError::Timeout => write!(f, "Timeout has been reached"),
            TransportTimeoutError::TimerError => write!(f, "Error in the timer"),
            TransportTimeoutError::Other(err) => write!(f, "{}", err),
        }
    }
}

impl<TErr> error::Error for TransportTimeoutError<TErr>
where TErr: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            TransportTimeoutError::Timeout => None,
            TransportTimeoutError::TimerError => None,
            TransportTimeoutError::Other(err) => Some(err),
        }
    }
}
