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

use futures::{future, prelude::*, sync::oneshot};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use transport::{MuxedTransport, Transport};
use Multiaddr;

/// See `Transport::interruptible`.
#[derive(Debug, Clone)]
pub struct Interruptible<T> {
    transport: T,
    rx: future::Shared<oneshot::Receiver<()>>,
}

impl<T> Interruptible<T> {
    /// Internal function that builds an `Interruptible`.
    #[inline]
    pub(crate) fn new(transport: T) -> (Interruptible<T>, Interrupt) {
        let (_tx, rx) = oneshot::channel();
        let transport = Interruptible { transport, rx: rx.shared() };
        let int = Interrupt { _tx };
        (transport, int)
    }
}

impl<T> Transport for Interruptible<T>
where
    T: Transport,
{
    type Output = T::Output;
    type MultiaddrFuture = T::MultiaddrFuture;
    type Listener = T::Listener;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = InterruptibleDial<T::Dial>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        match self.transport.listen_on(addr) {
            Ok(val) => Ok(val),
            Err((transport, addr)) => Err((Interruptible { transport, rx: self.rx }, addr)),
        }
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match self.transport.dial(addr) {
            Ok(future) => {
                Ok(InterruptibleDial {
                    inner: future,
                    rx: self.rx,
                })
            }
            Err((transport, addr)) => Err((Interruptible { transport, rx: self.rx }, addr)),
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

impl<T> MuxedTransport for Interruptible<T>
where
    T: MuxedTransport,
{
    type Incoming = T::Incoming;
    type IncomingUpgrade = T::IncomingUpgrade;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        self.transport.next_incoming()
    }
}

/// Dropping this object interrupts the dialing of the corresponding `Interruptible`.
pub struct Interrupt {
    _tx: oneshot::Sender<()>,
}

pub struct InterruptibleDial<F> {
    inner: F,
    rx: future::Shared<oneshot::Receiver<()>>,
}

impl<F> Future for InterruptibleDial<F>
    where F: Future<Error = IoError>
{
    type Item = F::Item;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.rx.poll() {
            Ok(Async::Ready(_)) | Err(_) => {
                return Err(IoError::new(IoErrorKind::ConnectionAborted, "connection interrupted"));
            },
            Ok(Async::NotReady) => (),
        };

        self.inner.poll()
    }
}
