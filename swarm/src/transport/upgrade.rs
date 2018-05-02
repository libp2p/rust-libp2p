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

use bytes::Bytes;
use connection_reuse::ConnectionReuse;
use futures::prelude::*;
use multiaddr::Multiaddr;
use multistream_select;
use muxing::StreamMuxer;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use tokio_io::{AsyncRead, AsyncWrite};
use transport::{MuxedTransport, Transport};
use upgrade::{ConnectionUpgrade, Endpoint};

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
    C: ConnectionUpgrade<T::RawConn> + 'a,
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
                let iter = upgrade.protocol_names()
                    .map(|(name, id)| (name, <Bytes as PartialEq>::eq, id));
                debug!(target: "libp2p-swarm", "Starting protocol negotiation (dialer)");
                let negotiated = multistream_select::dialer_select_proto(connection, iter)
                    .map_err(|err| IoError::new(IoErrorKind::Other, err));
                negotiated.map(|(upgrade_id, conn)| (upgrade_id, conn, upgrade, client_addr))
            })
            .then(|negotiated| {
                match negotiated {
                    Ok((_, _, _, ref client_addr)) => {
                        debug!(target: "libp2p-swarm", "Successfully negotiated protocol \
                               upgrade with {}", client_addr)
                    },
                    Err(ref err) => {
                        debug!(target: "libp2p-swarm", "Error while negotiated protocol \
                               upgrade: {:?}", err)
                    },
                };
                negotiated
            })
            .and_then(move |(upgrade_id, connection, upgrade, client_addr)| {
                let f = upgrade.upgrade(connection, upgrade_id, Endpoint::Dialer, &client_addr);
                debug!(target: "libp2p-swarm", "Trying to apply negotiated protocol with {}",
                       client_addr);
                f.map(|v| (v, client_addr))
            })
            .then(|val| {
                match val {
                    Ok(_) => debug!(target: "libp2p-swarm", "Successfully applied negotiated \
                                                             protocol"),
                    Err(_) => debug!(target: "libp2p-swarm", "Failed to apply negotiated protocol"),
                }
                val
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
            let future = future
                .and_then(move |(connection, addr)| {
                    let iter = upgrade
                        .protocol_names()
                        .map::<_, fn(_) -> _>(|(name, id)| (name, <Bytes as PartialEq>::eq, id));
                    debug!(target: "libp2p-swarm", "Starting protocol negotiation (incoming)");
                    let negotiated = multistream_select::listener_select_proto(connection, iter)
                        .map_err(|err| IoError::new(IoErrorKind::Other, err));
                    negotiated.map(move |(upgrade_id, conn)| (upgrade_id, conn, upgrade, addr))
                })
                .then(|negotiated| {
                    match negotiated {
                        Ok((_, _, _, ref client_addr)) => {
                            debug!(target: "libp2p-swarm", "Successfully negotiated protocol \
                                upgrade with {}", client_addr)
                        }
                        Err(ref err) => {
                            debug!(target: "libp2p-swarm", "Error while negotiated protocol \
                                upgrade: {:?}", err)
                        }
                    };
                    negotiated
                })
                .and_then(move |(upgrade_id, connection, upgrade, addr)| {
                    let upg = upgrade.upgrade(connection, upgrade_id, Endpoint::Listener, &addr);
                    debug!(target: "libp2p-swarm", "Trying to apply negotiated protocol with {}",
                           addr);
                    upg.map(|u| (u, addr))
                })
                .then(|val| {
                    match val {
                        Ok(_) => debug!(target: "libp2p-swarm", "Successfully applied negotiated \
                                                                 protocol"),
                        Err(_) => debug!(target: "libp2p-swarm", "Failed to apply negotiated \
                                                                  protocol"),
                    }
                    val
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
                    // Try to negotiate the protocol
                    .and_then(move |(connection, remote_addr)| {
                        let iter = upgrade.protocol_names()
                            .map::<_, fn(_) -> _>(|(n, t)| (n, <Bytes as PartialEq>::eq, t));
                        let remote_addr2 = remote_addr.clone();
                        debug!(target: "libp2p-swarm", "Starting protocol negotiation (listener)");
                        multistream_select::listener_select_proto(connection, iter)
                            .map_err(|err| IoError::new(IoErrorKind::Other, err))
                            .then(move |negotiated| {
                                match negotiated {
                                    Ok(_) => {
                                        debug!(target: "libp2p-swarm", "Successfully negotiated \
                                               protocol upgrade with {}", remote_addr2)
                                    },
                                    Err(ref err) => {
                                        debug!(target: "libp2p-swarm", "Error while negotiated \
                                               protocol upgrade: {:?}", err)
                                    },
                                };
                                negotiated
                            })
                            .and_then(move |(upgrade_id, connection)| {
                                let fut = upgrade.upgrade(
                                    connection,
                                    upgrade_id,
                                    Endpoint::Listener,
                                    &remote_addr,
                                );
                                fut.map(move |c| (c, remote_addr))
                            })
                            .into_future()
                    });

            Box::new(connection) as Box<_>
        });

        Ok((Box::new(stream), new_addr))
    }
}

impl<T, C> Transport for UpgradedNode<T, C>
where
    T: Transport + 'static,
    C: ConnectionUpgrade<T::RawConn> + 'static,
    C::Output: AsyncRead + AsyncWrite,
    C::NamesIter: Clone, // TODO: not elegant
    C: Clone,
{
    type RawConn = C::Output;
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
    C: ConnectionUpgrade<T::RawConn> + 'static,
    C::Output: AsyncRead + AsyncWrite,
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
