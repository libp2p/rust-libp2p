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
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::io::{self, Cursor};
use transport::MuxedTransport;
use transport::Transport;

/// Dummy implementation of `Transport` that just denies every single attempt.
#[derive(Debug, Copy, Clone)]
pub struct DeniedTransport;

impl Transport for DeniedTransport {
    // TODO: could use `!` for associated types once stable
    type Output = Cursor<Vec<u8>>;
    type MultiaddrFuture = Box<Future<Item = Multiaddr, Error = io::Error>>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = io::Error>>;
    type ListenerUpgrade = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = io::Error>>;
    type Dial = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = io::Error>>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        Err((DeniedTransport, addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        Err((DeniedTransport, addr))
    }

    #[inline]
    fn nat_traversal(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

impl MuxedTransport for DeniedTransport {
    type Incoming = future::Empty<Self::IncomingUpgrade, io::Error>;
    type IncomingUpgrade = future::Empty<(Self::Output, Self::MultiaddrFuture), io::Error>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        future::empty()
    }
}
