// Copyright 2019 Parity Technologies (UK) Ltd.
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
    pin::Pin,
    task::{Context, Poll},
};

use multiaddr::Multiaddr;

use crate::transport::{DialOpts, ListenerId, Transport, TransportError, TransportEvent};

/// Transport that is possibly disabled.
///
/// An `OptionalTransport<T>` is a wrapper around an `Option<T>`. If it is disabled (read: contains
/// `None`), then any attempt to dial or listen will return `MultiaddrNotSupported`. If it is
/// enabled (read: contains `Some`), then dialing and listening will be handled by the inner
/// transport.
#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct OptionalTransport<T>(#[pin] Option<T>);

impl<T> OptionalTransport<T> {
    /// Builds an `OptionalTransport` with the given transport in an enabled
    /// state.
    pub fn some(inner: T) -> OptionalTransport<T> {
        OptionalTransport(Some(inner))
    }

    /// Builds a disabled `OptionalTransport`.
    pub fn none() -> OptionalTransport<T> {
        OptionalTransport(None)
    }
}

impl<T> From<T> for OptionalTransport<T> {
    fn from(inner: T) -> Self {
        OptionalTransport(Some(inner))
    }
}

impl<T> Transport for OptionalTransport<T>
where
    T: Transport,
{
    type Output = T::Output;
    type Error = T::Error;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = T::Dial;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        if let Some(inner) = self.0.as_mut() {
            inner.listen_on(id, addr)
        } else {
            Err(TransportError::MultiaddrNotSupported(addr))
        }
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(inner) = self.0.as_mut() {
            inner.remove_listener(id)
        } else {
            false
        }
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        if let Some(inner) = self.0.as_mut() {
            inner.dial(addr, opts)
        } else {
            Err(TransportError::MultiaddrNotSupported(addr))
        }
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        if let Some(inner) = self.project().0.as_pin_mut() {
            inner.poll(cx)
        } else {
            Poll::Pending
        }
    }
}
