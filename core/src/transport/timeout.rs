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

use crate::{
    transport::{ListenerId, TransportError, TransportEvent},
    Multiaddr, Transport,
};
use futures::prelude::*;
use futures_timer::Delay;
use std::{error, fmt, io, pin::Pin, task::Context, task::Poll, time::Duration};

/// A `TransportTimeout` is a `Transport` that wraps another `Transport` and adds
/// timeouts to all inbound and outbound connection attempts.
///
/// **Note**: `listen_on` is never subject to a timeout, only the setup of each
/// individual accepted connection.
#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct TransportTimeout<InnerTrans> {
    #[pin]
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
    type ListenerUpgrade = Timeout<InnerTrans::ListenerUpgrade>;
    type Dial = Timeout<InnerTrans::Dial>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        self.inner
            .listen_on(addr)
            .map_err(|err| err.map(TransportTimeoutError::Other))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dial = self
            .inner
            .dial(addr)
            .map_err(|err| err.map(TransportTimeoutError::Other))?;
        Ok(Timeout {
            inner: dial,
            timer: Delay::new(self.outgoing_timeout),
        })
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dial = self
            .inner
            .dial_as_listener(addr)
            .map_err(|err| err.map(TransportTimeoutError::Other))?;
        Ok(Timeout {
            inner: dial,
            timer: Delay::new(self.outgoing_timeout),
        })
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(server, observed)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        let timeout = *this.incoming_timeout;
        this.inner.poll(cx).map(|event| {
            event
                .map_upgrade(move |inner_fut| Timeout {
                    inner: inner_fut,
                    timer: Delay::new(timeout),
                })
                .map_err(TransportTimeoutError::Other)
        })
    }
}

/// Wraps around a `Future`. Turns the error type from `TimeoutError<Err>` to
/// `TransportTimeoutError<Err>`.
// TODO: can be replaced with `impl Future` once `impl Trait` are fully stable in Rust
//       (https://github.com/rust-lang/rust/issues/34511)
#[pin_project::pin_project]
#[must_use = "futures do nothing unless polled"]
pub struct Timeout<InnerFut> {
    #[pin]
    inner: InnerFut,
    timer: Delay,
}

impl<InnerFut> Future for Timeout<InnerFut>
where
    InnerFut: TryFuture,
{
    type Output = Result<InnerFut::Ok, TransportTimeoutError<InnerFut::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // It is debatable whether we should poll the inner future first or the timer first.
        // For example, if you start dialing with a timeout of 10 seconds, then after 15 seconds
        // the dialing succeeds on the wire, then after 20 seconds you poll, then depending on
        // which gets polled first, the outcome will be success or failure.

        let mut this = self.project();

        match TryFuture::try_poll(this.inner, cx) {
            Poll::Pending => {}
            Poll::Ready(Ok(v)) => return Poll::Ready(Ok(v)),
            Poll::Ready(Err(err)) => return Poll::Ready(Err(TransportTimeoutError::Other(err))),
        }

        match Pin::new(&mut this.timer).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => Poll::Ready(Err(TransportTimeoutError::Timeout)),
        }
    }
}

/// Error that can be produced by the `TransportTimeout` layer.
#[derive(Debug)]
pub enum TransportTimeoutError<TErr> {
    /// The transport timed out.
    Timeout,
    /// An error happened in the timer.
    TimerError(io::Error),
    /// Other kind of error.
    Other(TErr),
}

impl<TErr> fmt::Display for TransportTimeoutError<TErr>
where
    TErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportTimeoutError::Timeout => write!(f, "Timeout has been reached"),
            TransportTimeoutError::TimerError(err) => write!(f, "Error in the timer: {}", err),
            TransportTimeoutError::Other(err) => write!(f, "{}", err),
        }
    }
}

impl<TErr> error::Error for TransportTimeoutError<TErr>
where
    TErr: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            TransportTimeoutError::Timeout => None,
            TransportTimeoutError::TimerError(err) => Some(err),
            TransportTimeoutError::Other(err) => Some(err),
        }
    }
}
