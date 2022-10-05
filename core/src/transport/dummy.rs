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

use crate::transport::{ListenerId, Transport, TransportError, TransportEvent};
use crate::Multiaddr;
use futures::{prelude::*, task::Context, task::Poll};
use std::{fmt, io, marker::PhantomData, pin::Pin};

/// Implementation of `Transport` that doesn't support any multiaddr.
///
/// Useful for testing purposes, or as a fallback implementation when no protocol is available.
pub struct DummyTransport<TOut = DummyStream>(PhantomData<TOut>);

impl<TOut> DummyTransport<TOut> {
    /// Builds a new `DummyTransport`.
    pub fn new() -> Self {
        DummyTransport(PhantomData)
    }
}

impl<TOut> Default for DummyTransport<TOut> {
    fn default() -> Self {
        DummyTransport::new()
    }
}

impl<TOut> fmt::Debug for DummyTransport<TOut> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DummyTransport")
    }
}

impl<TOut> Clone for DummyTransport<TOut> {
    fn clone(&self) -> Self {
        DummyTransport(PhantomData)
    }
}

impl<TOut> Transport for DummyTransport<TOut> {
    type Output = TOut;
    type Error = io::Error;
    type ListenerUpgrade = futures::future::Pending<Result<Self::Output, io::Error>>;
    type Dial = futures::future::Pending<Result<Self::Output, io::Error>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn address_translation(&self, _server: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }

    fn poll(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }
}

/// Implementation of `AsyncRead` and `AsyncWrite`. Not meant to be instanciated.
pub struct DummyStream(());

impl fmt::Debug for DummyStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DummyStream")
    }
}

impl AsyncRead for DummyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }
}

impl AsyncWrite for DummyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }
}
