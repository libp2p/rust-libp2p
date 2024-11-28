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

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use either::Either;
use futures::future;
use multiaddr::Multiaddr;

use crate::{
    either::EitherFuture,
    transport::{DialOpts, ListenerId, Transport, TransportError, TransportEvent},
};

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
    type Output = future::Either<A::Output, B::Output>;
    type Error = Either<A::Error, B::Error>;
    type ListenerUpgrade = EitherFuture<A::ListenerUpgrade, B::ListenerUpgrade>;
    type Dial = EitherFuture<A::Dial, B::Dial>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        tracing::trace!(
            address=%addr,
            "Attempting to listen on address using {}",
            std::any::type_name::<A>()
        );
        let addr = match self.0.listen_on(id, addr) {
            Err(TransportError::MultiaddrNotSupported(addr)) => {
                tracing::debug!(
                    address=%addr,
                    "Failed to listen on address using {}",
                    std::any::type_name::<A>()
                );
                addr
            }
            res => return res.map_err(|err| err.map(Either::Left)),
        };

        tracing::trace!(
            address=%addr,
            "Attempting to listen on address using {}",
            std::any::type_name::<B>()
        );
        let addr = match self.1.listen_on(id, addr) {
            Err(TransportError::MultiaddrNotSupported(addr)) => {
                tracing::debug!(
                    address=%addr,
                    "Failed to listen on address using {}",
                    std::any::type_name::<B>()
                );
                addr
            }
            res => return res.map_err(|err| err.map(Either::Right)),
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.0.remove_listener(id) || self.1.remove_listener(id)
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        tracing::trace!(
            address=%addr,
            "Attempting to dial using {}",
            std::any::type_name::<A>()
        );
        let addr = match self.0.dial(addr, opts) {
            Ok(connec) => return Ok(EitherFuture::First(connec)),
            Err(TransportError::MultiaddrNotSupported(addr)) => {
                tracing::debug!(
                    address=%addr,
                    "Failed to dial using {}",
                    std::any::type_name::<B>(),
                );
                addr
            }
            Err(TransportError::Other(err)) => {
                return Err(TransportError::Other(Either::Left(err)))
            }
        };

        tracing::trace!(
            address=%addr,
            "Attempting to dial {}",
            std::any::type_name::<A>()
        );
        let addr = match self.1.dial(addr, opts) {
            Ok(connec) => return Ok(EitherFuture::Second(connec)),
            Err(TransportError::MultiaddrNotSupported(addr)) => {
                tracing::debug!(
                    address=%addr,
                    "Failed to dial using {}",
                    std::any::type_name::<B>(),
                );
                addr
            }
            Err(TransportError::Other(err)) => {
                return Err(TransportError::Other(Either::Right(err)))
            }
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        match this.0.poll(cx) {
            Poll::Ready(ev) => {
                return Poll::Ready(ev.map_upgrade(EitherFuture::First).map_err(Either::Left))
            }
            Poll::Pending => {}
        }
        match this.1.poll(cx) {
            Poll::Ready(ev) => {
                return Poll::Ready(ev.map_upgrade(EitherFuture::Second).map_err(Either::Right))
            }
            Poll::Pending => {}
        }
        Poll::Pending
    }
}
