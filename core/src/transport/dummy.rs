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

use crate::transport::{Dialer, Transport, TransportError, ListenerEvent};
use crate::Multiaddr;
use futures::{prelude::*, task::Context, task::Poll};
use std::{fmt, io, marker::PhantomData, pin::Pin};

type TError = io::Error;
type TDial<TOut> = futures::future::Pending<Result<TOut, TError>>;
type TListenerUpgrade<TOut> = futures::future::Pending<Result<TOut, TError>>;

/// Implementation of `Transport` that doesn't support any multiaddr.
///
/// Useful for testing purposes, or as a fallback implementation when no protocol is available.
#[derive(Default)]
pub struct DummyTransport<TOut = DummyStream>(DummyDialer<TOut>);

impl<TOut> DummyTransport<TOut> {
    /// Builds a new `DummyTransport`.
    pub fn new() -> Self {
        DummyTransport(DummyDialer::new())
    }
}

impl<TOut> Clone for DummyTransport<TOut> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<TOut> fmt::Debug for DummyTransport<TOut> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DummyTransport")
    }
}

impl<TOut: Unpin> Transport for DummyTransport<TOut> {
    type Output = TOut;
    type Error = TError;
    type Dial = TDial<TOut>;
    type Dialer = DummyDialer<TOut>;
    type ListenerUpgrade = TListenerUpgrade<TOut>;
    type Listener = DummyListener<TOut>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Self::Dialer), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dialer(&self) -> Self::Dialer {
        self.0.clone()
    }
}

/// Implementation of `Dialer` that doesn't support any multiaddr.
///
/// Useful for testing purposes, or as a fallback implementation when no protocol is available.
pub struct DummyDialer<TOut>(PhantomData<TOut>);

impl<TOut> Dialer for DummyDialer<TOut> {
    type Output = TOut;
    type Error = TError;
    type Dial = TDial<TOut>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }
}

impl<TOut> DummyDialer<TOut> {
    /// Builds a new `DummyDialer`.
    pub fn new() -> Self {
        DummyDialer(PhantomData)
    }
}

impl<TOut> Default for DummyDialer<TOut> {
    fn default() -> Self {
        DummyDialer::new()
    }
}

impl<TOut> fmt::Debug for DummyDialer<TOut> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DummyTransport")
    }
}

impl<TOut> Clone for DummyDialer<TOut> {
    fn clone(&self) -> Self {
        DummyDialer(PhantomData)
    }
}

/// Implementation of `Dialer` that doesn't support any multiaddr.
///
/// Useful for testing purposes, or as a fallback implementation when no protocol is available.
#[derive(Clone, Default)]
pub struct DummyListener<TOut>(DummyDialer<TOut>);

impl<TOut> Stream for DummyListener<TOut> {
    type Item = Result<ListenerEvent<TListenerUpgrade<TOut>, TError>, TError>;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context) -> Poll<Option<Self::Item>> {
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
    fn poll_read(self: Pin<&mut Self>, _: &mut Context<'_>, _: &mut [u8])
        -> Poll<Result<usize, io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }
}

impl AsyncWrite for DummyStream {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8])
        -> Poll<Result<usize, io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>)
        -> Poll<Result<(), io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>)
        -> Poll<Result<(), io::Error>>
    {
        Poll::Ready(Err(io::ErrorKind::Other.into()))
    }
}
