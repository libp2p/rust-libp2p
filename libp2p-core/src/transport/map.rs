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

use crate::{
    connection::{ConnectedPoint, Endpoint},
    transport::{Transport, TransportError, TransportEvent},
};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{pin::Pin, task::Context, task::Poll};

use super::ListenerId;

/// See `Transport::map`.
#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct Map<T, F> {
    #[pin]
    transport: T,
    fun: F,
}

impl<T, F> Map<T, F> {
    pub(crate) fn new(transport: T, fun: F) -> Self {
        Map { transport, fun }
    }

    pub fn inner(&self) -> &T {
        &self.transport
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.transport
    }
}

impl<T, F, D> Transport for Map<T, F>
where
    T: Transport,
    F: FnOnce(T::Output, ConnectedPoint) -> D + Clone,
{
    type Output = D;
    type Error = T::Error;
    type ListenerUpgrade = MapFuture<T::ListenerUpgrade, F>;
    type Dial = MapFuture<T::Dial, F>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        self.transport.listen_on(addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.transport.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let future = self.transport.dial(addr.clone())?;
        let p = ConnectedPoint::Dialer {
            address: addr,
            role_override: Endpoint::Dialer,
        };
        Ok(MapFuture {
            inner: future,
            args: Some((self.fun.clone(), p)),
        })
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let future = self.transport.dial_as_listener(addr.clone())?;
        let p = ConnectedPoint::Dialer {
            address: addr,
            role_override: Endpoint::Listener,
        };
        Ok(MapFuture {
            inner: future,
            args: Some((self.fun.clone(), p)),
        })
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.address_translation(server, observed)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        match this.transport.poll(cx) {
            Poll::Ready(TransportEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr,
            }) => {
                let point = ConnectedPoint::Listener {
                    local_addr: local_addr.clone(),
                    send_back_addr: send_back_addr.clone(),
                };
                Poll::Ready(TransportEvent::Incoming {
                    listener_id,
                    upgrade: MapFuture {
                        inner: upgrade,
                        args: Some((this.fun.clone(), point)),
                    },
                    local_addr,
                    send_back_addr,
                })
            }
            Poll::Ready(other) => {
                let mapped = other.map_upgrade(|_upgrade| unreachable!("case already matched"));
                Poll::Ready(mapped)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Custom `Future` to avoid boxing.
///
/// Applies a function to the inner future's result.
#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapFuture<T, F> {
    #[pin]
    inner: T,
    args: Option<(F, ConnectedPoint)>,
}

impl<T, A, F, B> Future for MapFuture<T, F>
where
    T: TryFuture<Ok = A>,
    F: FnOnce(A, ConnectedPoint) -> B,
{
    type Output = Result<B, T::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let item = match TryFuture::try_poll(this.inner, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(v)) => v,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        let (f, a) = this.args.take().expect("MapFuture has already finished.");
        Poll::Ready(Ok(f(item, a)))
    }
}
