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

use connection_reuse::ConnectionReuse;
use futures::prelude::*;
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};
use transport::{MuxedTransport, Transport};
use upgrade::{apply, ConnectionUpgrade, Endpoint};

/// Implements the `Transport` trait. Dials or listens, then upgrades any dialed or received
/// connection.
///
/// See the `Transport::with_upgrade` method.
#[derive(Debug, Clone)]
pub struct UpgradedNode<T, C> {
    transports: T,
    upgrade: C,
}

impl<T, C> UpgradedNode<T, C> {
    pub fn new(transports: T, upgrade: C) -> UpgradedNode<T, C> {
        UpgradedNode {
            transports,
            upgrade,
        }
    }
}

impl<'a, T, C> UpgradedNode<T, C>
where
    T: Transport + 'a,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output> + 'a,
{
    /// Turns this upgraded node into a `ConnectionReuse`. If the `Output` implements the
    /// `StreamMuxer` trait, the returned object will implement `Transport` and `MuxedTransport`.
    #[inline]
    pub fn into_connection_reuse(self) -> ConnectionReuse<T, C>
    where
        C::Output: StreamMuxer,
    {
        From::from(self)
    }

    /// Returns a reference to the inner `Transport`.
    #[inline]
    pub fn transport(&self) -> &T {
        &self.transports
    }

    /// Tries to dial on the `Multiaddr` using the transport that was passed to `new`, then upgrade
    /// the connection.
    ///
    /// Note that this does the same as `Transport::dial`, but with less restrictions on the trait
    /// requirements.
    #[inline]
    pub fn dial(
        self,
        addr: Multiaddr,
    ) -> Result<Box<Future<Item = (C::Output, Multiaddr), Error = IoError> + 'a>, (Self, Multiaddr)>
    where
        C::NamesIter: Clone, // TODO: not elegant
    {
        let upgrade = self.upgrade;

        let dialed_fut = match self.transports.dial(addr.clone()) {
            Ok(f) => f.into_future(),
            Err((trans, addr)) => {
                let builder = UpgradedNode {
                    transports: trans,
                    upgrade: upgrade,
                };

                return Err((builder, addr));
            }
        };

        let future = dialed_fut
            // Try to negotiate the protocol.
            .and_then(move |(connection, client_addr)| {
                apply(connection, upgrade, Endpoint::Dialer, client_addr)
            });

        Ok(Box::new(future))
    }

    /// If the underlying transport is a `MuxedTransport`, then after calling `dial` we may receive
    /// substreams opened by the dialed nodes.
    ///
    /// This function returns the next incoming substream. You are strongly encouraged to call it
    /// if you have a muxed transport.
    pub fn next_incoming(
        self,
    ) -> Box<
        Future<
                Item = Box<Future<Item = (C::Output, Multiaddr), Error = IoError> + 'a>,
                Error = IoError,
            >
            + 'a,
    >
    where
        T: MuxedTransport,
        C::NamesIter: Clone, // TODO: not elegant
        C: Clone,
    {
        let upgrade = self.upgrade;

        let future = self.transports.next_incoming().map(|future| {
            // Try to negotiate the protocol.
            let future = future.and_then(move |(connection, client_addr)| {
                apply(connection, upgrade, Endpoint::Listener, client_addr)
            });

            Box::new(future) as Box<Future<Item = _, Error = _>>
        });

        Box::new(future) as Box<_>
    }

    /// Start listening on the multiaddr using the transport that was passed to `new`.
    /// Then whenever a connection is opened, it is upgraded.
    ///
    /// Note that this does the same as `Transport::listen_on`, but with less restrictions on the
    /// trait requirements.
    #[inline]
    pub fn listen_on(
        self,
        addr: Multiaddr,
    ) -> Result<
        (
            Box<
                Stream<
                        Item = Box<Future<Item = (C::Output, Multiaddr), Error = IoError> + 'a>,
                        Error = IoError,
                    >
                    + 'a,
            >,
            Multiaddr,
        ),
        (Self, Multiaddr),
    >
    where
        C::NamesIter: Clone, // TODO: not elegant
        C: Clone,
    {
        let upgrade = self.upgrade;

        let (listening_stream, new_addr) = match self.transports.listen_on(addr) {
            Ok((l, new_addr)) => (l, new_addr),
            Err((trans, addr)) => {
                let builder = UpgradedNode {
                    transports: trans,
                    upgrade: upgrade,
                };

                return Err((builder, addr));
            }
        };

        // Try to negotiate the protocol.
        // Note that failing to negotiate a protocol will never produce a future with an error.
        // Instead the `stream` will produce `Ok(Err(...))`.
        // `stream` can only produce an `Err` if `listening_stream` produces an `Err`.
        let stream = listening_stream.map(move |connection| {
            let upgrade = upgrade.clone();
            let connection = connection
                // Try to negotiate the protocol.
                .and_then(move |(connection, client_addr)| {
                    apply(connection, upgrade, Endpoint::Listener, client_addr)
                });

            Box::new(connection) as Box<_>
        });

        Ok((Box::new(stream), new_addr))
    }
}

impl<T, C> Transport for UpgradedNode<T, C>
where
    T: Transport + 'static,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output> + 'static,
    C::NamesIter: Clone, // TODO: not elegant
    C: Clone,
{
    type Output = C::Output;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>;
    type Dial = Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        self.listen_on(addr)
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        self.dial(addr)
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transports.nat_traversal(server, observed)
    }
}

impl<T, C> MuxedTransport for UpgradedNode<T, C>
where
    T: MuxedTransport + 'static,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output> + 'static,
    C::NamesIter: Clone, // TODO: not elegant
    C: Clone,
{
    type Incoming = Box<Future<Item = Self::IncomingUpgrade, Error = IoError>>;
    type IncomingUpgrade = Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        self.next_incoming()
    }
}
