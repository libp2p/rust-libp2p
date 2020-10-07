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

use crate::transport::{Dialer, ListenerEvent, Transport, TransportError};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{error, fmt, pin::Pin, sync::Arc};

/// See the `Transport::boxed` method.
pub fn boxed<T>(transport: T) -> Boxed<T::Output, T::Error>
where
    T: Transport + Clone + Send + Sync + 'static,
    T::Dial: Send + 'static,
    T::Dialer: Clone + Send + Sync + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
{
    Boxed {
        inner: Arc::new(transport) as Arc<_>,
    }
}

/// See the `Transport::boxed` method.
pub struct Boxed<O, E> {
    inner: Arc<dyn Abstract<O, E> + Send + Sync>,
}

type Dial<O, E> = Pin<Box<dyn Future<Output = Result<O, E>> + Send>>;
type Listener<O, E> = Pin<Box<dyn Stream<Item = Result<ListenerEvent<ListenerUpgrade<O, E>, E>, E>> + Send>>;
type ListenerUpgrade<O, E> = Pin<Box<dyn Future<Output = Result<O, E>> + Send>>;

trait Abstract<O, E> {
    fn listen_on(&self, addr: Multiaddr) -> Result<(Listener<O, E>, BoxedDialer<O, E>), TransportError<E>>;
    fn dialer(&self) -> BoxedDialer<O, E>;
}

trait AbstractDialer<O, E> {
    fn dial(&self, addr: Multiaddr) -> Result<Dial<O, E>, TransportError<E>>;
}

impl<T, O, E> Abstract<O, E> for T
where
    T: Transport<Output = O, Error = E> + Clone + 'static,
    E: error::Error,
    T::Dial: Send + 'static,
    T::Dialer: Clone + Send + Sync + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
{
    fn listen_on(&self, addr: Multiaddr) -> Result<(Listener<O, E>, BoxedDialer<O, E>), TransportError<E>> {
        let (listener, dialer) = Transport::listen_on(self.clone(), addr)?;
        let fut = listener.map_ok(|event| event.map(|upgrade| {
            Box::pin(upgrade) as ListenerUpgrade<O, E>
        }));
        Ok((Box::pin(fut), BoxedDialer::new(dialer)))
    }

    fn dialer(&self) -> BoxedDialer<O, E> {
        BoxedDialer::new(Transport::dialer(self))
    }
}

impl<D, O, E> AbstractDialer<O, E> for D
where
    D: Dialer<Output = O, Error = E> + Clone + Send + 'static,
    E: error::Error,
    D::Dial: Send + 'static,
{
    fn dial(&self, addr: Multiaddr) -> Result<Dial<O, E>, TransportError<E>> {
        let fut = Dialer::dial(self.clone(), addr)?;
        Ok(Box::pin(fut) as Dial<_, _>)
    }
}

impl<O, E> fmt::Debug for Boxed<O, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoxedTransport")
    }
}

impl<O, E> Clone for Boxed<O, E> {
    fn clone(&self) -> Self {
        Boxed {
            inner: self.inner.clone(),
        }
    }
}

impl<O, E> Transport for Boxed<O, E>
where E: error::Error,
{
    type Output = O;
    type Error = E;
    type Dial = Dial<O, E>;
    type Dialer = BoxedDialer<O, E>;
    type Listener = Listener<O, E>;
    type ListenerUpgrade = ListenerUpgrade<O, E>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Self::Dialer), TransportError<Self::Error>> {
        self.inner.listen_on(addr)
    }

    fn dialer(&self) -> Self::Dialer {
        self.inner.dialer()
    }
}

/// See the `Transport::boxed` method.
pub struct BoxedDialer<O, E> {
    inner: Arc<dyn AbstractDialer<O, E> + Send + Sync>,
}

impl<O, E> BoxedDialer<O, E> {
    fn new<D: AbstractDialer<O, E> + Send + Sync + 'static>(dialer: D) -> Self {
        BoxedDialer {
            inner: Arc::new(dialer),
        }
    }
}

impl<O, E> fmt::Debug for BoxedDialer<O, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoxedDialer")
    }
}

impl<O, E> Clone for BoxedDialer<O, E> {
    fn clone(&self) -> Self {
        BoxedDialer {
            inner: self.inner.clone(),
        }
    }
}

impl<O, E> Dialer for BoxedDialer<O, E>
where E: error::Error,
{
    type Output = O;
    type Error = E;
    type Dial = Dial<O, E>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner.dial(addr)
    }
}
