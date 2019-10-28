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

use crate::transport::{Transport, TransportError, ListenerEvent};
use crate::Multiaddr;
use std::{fmt, io, marker::PhantomData};

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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
    type Listener = futures::stream::Empty<ListenerEvent<Self::ListenerUpgrade>, io::Error>;
    type ListenerUpgrade = futures::future::Empty<Self::Output, io::Error>;
    type Dial = futures::future::Empty<Self::Output, io::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }
}

/// Implementation of `Read` and `Write`. Not meant to be instanciated.
pub struct DummyStream(());

impl fmt::Debug for DummyStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DummyStream")
    }
}

impl io::Read for DummyStream {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::Other.into())
    }
}

impl io::Write for DummyStream {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::Other.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::Other.into())
    }
}

impl tokio_io::AsyncRead for DummyStream {
    unsafe fn prepare_uninitialized_buffer(&self, _: &mut [u8]) -> bool {
        false
    }
}

impl tokio_io::AsyncWrite for DummyStream {
    fn shutdown(&mut self) -> futures::Poll<(), io::Error> {
        Err(io::ErrorKind::Other.into())
    }
}
