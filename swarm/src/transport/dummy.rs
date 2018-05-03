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

use futures::future;
use multiaddr::Multiaddr;
use std::io::Error as IoError;
use transport::{MuxedTransport, Transport};

/// Dummy implementation of `MuxedTransport` that uses an inner `Transport`.
#[derive(Debug, Copy, Clone)]
pub struct DummyMuxing<T> {
    inner: T,
}

impl<T> DummyMuxing<T> {
    pub fn new(transport: T) -> DummyMuxing<T> {
        DummyMuxing { inner: transport }
    }
}

impl<T> MuxedTransport for DummyMuxing<T>
where
    T: Transport,
{
    type Incoming = future::Empty<Self::IncomingUpgrade, IoError>;
    type IncomingUpgrade = future::Empty<(T::RawConn, Multiaddr), IoError>;

    fn next_incoming(self) -> Self::Incoming
    where
        Self: Sized,
    {
        future::empty()
    }
}

impl<T> Transport for DummyMuxing<T>
where
    T: Transport,
{
    type RawConn = T::RawConn;
    type Listener = T::Listener;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = T::Dial;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)>
    where
        Self: Sized,
    {
        self.inner
            .listen_on(addr)
            .map_err(|(inner, addr)| (DummyMuxing { inner }, addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)>
    where
        Self: Sized,
    {
        self.inner
            .dial(addr)
            .map_err(|(inner, addr)| (DummyMuxing { inner }, addr))
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}
