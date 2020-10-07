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

use crate::either::{EitherListenStream, EitherOutput, EitherError, EitherFuture};
use crate::transport::{Dialer, Transport, TransportError};
use multiaddr::Multiaddr;

/// Struct returned by `or_transport()`.
#[derive(Debug, Copy, Clone)]
pub struct OrTransport<A, B>(A, B);

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
    type Dial = EitherFuture<A::Dial, B::Dial>;
    type Dialer = OrDialer<A::Dialer, B::Dialer>;
    type Listener = EitherListenStream<A::Listener, B::Listener>;
    type ListenerUpgrade = EitherFuture<A::ListenerUpgrade, B::ListenerUpgrade>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Self::Dialer), TransportError<Self::Error>> {
        let addr = match self.0.listen_on(addr) {
            Ok((listener, dialer)) => {
                let dialer = OrDialer(Some(dialer), None);
                let listener = EitherListenStream::First(listener);
                return Ok((listener, dialer));
            }
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            Err(TransportError::Other(err)) => return Err(TransportError::Other(EitherError::A(err))),
        };

        let addr = match self.1.listen_on(addr) {
            Ok((listener, dialer)) => {
                let dialer = OrDialer(None, Some(dialer));
                let listener = EitherListenStream::Second(listener);
                return Ok((listener, dialer));
            }
            Err(TransportError::MultiaddrNotSupported(addr)) => addr,
            Err(TransportError::Other(err)) => return Err(TransportError::Other(EitherError::B(err))),
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dialer(&self) -> Self::Dialer {
        OrDialer(Some(self.0.dialer()), Some(self.1.dialer()))
    }
}

#[derive(Debug, Copy, Clone)]
pub struct OrDialer<A, B>(Option<A>, Option<B>);

impl<A, B> Dialer for OrDialer<A, B>
where
    B: Dialer,
    A: Dialer,
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = EitherError<A::Error, B::Error>;
    type Dial = EitherFuture<A::Dial, B::Dial>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let addr = if let Some(dialer) = self.0 {
            match dialer.dial(addr) {
                Ok(connec) => return Ok(EitherFuture::First(connec)),
                Err(TransportError::MultiaddrNotSupported(addr)) => addr,
                Err(TransportError::Other(err)) => return Err(TransportError::Other(EitherError::A(err))),
            }
        } else {
            addr
        };

        let addr = if let Some(dialer) = self.1 {
            match dialer.dial(addr) {
                Ok(connec) => return Ok(EitherFuture::Second(connec)),
                Err(TransportError::MultiaddrNotSupported(addr)) => addr,
                Err(TransportError::Other(err)) => return Err(TransportError::Other(EitherError::B(err))),
            }
        } else {
            addr
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }
}
