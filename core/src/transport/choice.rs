// Copyright 2017 Parity Technologies (UK) Ltd.
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

use crate::either::{EitherError, EitherFuture, EitherOutput};
use crate::transport::{ListenerId, Transport, TransportError, TransportEvent};
use multiaddr::Multiaddr;
use std::{pin::Pin, task::Context, task::Poll};

/// Struct returned by `or_transport()`.
#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct OrTransport<A, B>(#[pin] A, #[pin] B);

impl<A, B> OrTransport<A, B> {
    pub fn new(a: A, b: B) -> OrTransport<A, B> {
        OrTransport(a, b)
    }
}

impl<A, B> Transport for OrTransport<A, B>
where
    B: Transport,
    A: Transport,
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = EitherError<A::Error, B::Error>;
    type ListenerUpgrade = EitherFuture<A::ListenerUpgrade, B::ListenerUpgrade>;
    type Dial = EitherFuture<A::Dial, B::Dial>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let addr = match self.0.listen_on(addr) {
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            res => return res.map_err(|err| err.map(EitherError::A)),
        };

        let addr = match self.1.listen_on(addr) {
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            res => return res.map_err(|err| err.map(EitherError::B)),
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.0.remove_listener(id) || self.1.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let addr = match self.0.dial(addr) {
            Ok(connec) => return Ok(EitherFuture::First(connec)),
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            Err(TransportError::Other(err)) => {
                return Err(TransportError::Other(EitherError::A(err)))
            }
        };

        let addr = match self.1.dial(addr) {
            Ok(connec) => return Ok(EitherFuture::Second(connec)),
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            Err(TransportError::Other(err)) => {
                return Err(TransportError::Other(EitherError::B(err)))
            }
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let addr = match self.0.dial_as_listener(addr) {
            Ok(connec) => return Ok(EitherFuture::First(connec)),
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            Err(TransportError::Other(err)) => {
                return Err(TransportError::Other(EitherError::A(err)))
            }
        };

        let addr = match self.1.dial_as_listener(addr) {
            Ok(connec) => return Ok(EitherFuture::Second(connec)),
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            Err(TransportError::Other(err)) => {
                return Err(TransportError::Other(EitherError::B(err)))
            }
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        if let Some(addr) = self.0.address_translation(server, observed) {
            Some(addr)
        } else {
            self.1.address_translation(server, observed)
        }
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        match this.0.poll(cx) {
            Poll::Ready(ev) => {
                return Poll::Ready(ev.map_upgrade(EitherFuture::First).map_err(EitherError::A))
            }
            Poll::Pending => {}
        }
        match this.1.poll(cx) {
            Poll::Ready(ev) => {
                return Poll::Ready(ev.map_upgrade(EitherFuture::Second).map_err(EitherError::B))
            }
            Poll::Pending => {}
        }
        Poll::Pending
    }
}
