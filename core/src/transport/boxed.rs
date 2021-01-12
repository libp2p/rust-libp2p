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

use crate::transport::{ListenerEvent, Transport, TransportError};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{error::Error, fmt, io, pin::Pin, sync::Arc};

/// Creates a new [`Boxed`] transport from the given transport.
pub fn boxed<T>(transport: T) -> Boxed<T::Output>
where
    T: Transport + Clone + Send + Sync + 'static,
    T::Error: Send + Sync,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
{
    Boxed {
        inner: Arc::new(transport) as Arc<_>,
    }
}

/// A `Boxed` transport is a `Transport` whose `Dial`, `Listener`
/// and `ListenerUpgrade` futures are `Box`ed and only the `Output`
/// and `Error` types are captured in type variables.
pub struct Boxed<O> {
    inner: Arc<dyn Abstract<O> + Send + Sync>,
}

type Dial<O> = Pin<Box<dyn Future<Output = io::Result<O>> + Send>>;
type Listener<O> = Pin<Box<dyn Stream<Item = io::Result<ListenerEvent<ListenerUpgrade<O>, io::Error>>> + Send>>;
type ListenerUpgrade<O> = Pin<Box<dyn Future<Output = io::Result<O>> + Send>>;

trait Abstract<O> {
    fn listen_on(&self, addr: Multiaddr) -> Result<Listener<O>, TransportError<io::Error>>;
    fn dial(&self, addr: Multiaddr) -> Result<Dial<O>, TransportError<io::Error>>;
    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr>;
}

impl<T, O> Abstract<O> for T
where
    T: Transport<Output = O> + Clone + 'static,
    T::Error: Send + Sync,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
{
    fn listen_on(&self, addr: Multiaddr) -> Result<Listener<O>, TransportError<io::Error>> {
        let listener = Transport::listen_on(self.clone(), addr).map_err(|e| e.map(box_err))?;
        let fut = listener.map_ok(|event|
            event.map(|upgrade| {
                let up = upgrade.map_err(box_err);
                Box::pin(up) as ListenerUpgrade<O>
            }).map_err(box_err)
        ).map_err(box_err);
        Ok(Box::pin(fut))
    }

    fn dial(&self, addr: Multiaddr) -> Result<Dial<O>, TransportError<io::Error>> {
        let fut = Transport::dial(self.clone(), addr)
            .map(|r| r.map_err(box_err))
            .map_err(|e| e.map(box_err))?;
        Ok(Box::pin(fut) as Dial<_>)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        Transport::address_translation(self, server, observed)
    }
}

impl<O> fmt::Debug for Boxed<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoxedTransport")
    }
}

impl<O> Clone for Boxed<O> {
    fn clone(&self) -> Self {
        Boxed {
            inner: self.inner.clone(),
        }
    }
}

impl<O> Transport for Boxed<O> {
    type Output = O;
    type Error = io::Error;
    type Listener = Listener<O>;
    type ListenerUpgrade = ListenerUpgrade<O>;
    type Dial = Dial<O>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        self.inner.listen_on(addr)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner.dial(addr)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(server, observed)
    }
}

fn box_err<E: Error + Send + Sync + 'static>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}
