// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Connection-oriented communication channels.
//!
//! The main entity of this module is the [`Transport`] trait, which provides an
//! interface for establishing connections with other nodes, thereby negotiating
//! any desired protocols. The rest of the module defines combinators for
//! modifying a transport through composition with other transports or protocol upgrades.

use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{
    error::Error,
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

pub mod and_then;
pub mod choice;
pub mod dummy;
pub mod map;
pub mod map_err;
pub mod memory;
pub mod timeout;
pub mod upgrade;

mod boxed;
mod optional;

use crate::ConnectedPoint;

pub use self::boxed::Boxed;
pub use self::choice::OrTransport;
pub use self::memory::MemoryTransport;
pub use self::optional::OptionalTransport;
pub use self::upgrade::Upgrade;

/// A transport provides connection-oriented communication between two peers
/// through ordered streams of data (i.e. connections).
///
/// Connections are established either by [listening](Transport::listen_on)
/// or [dialing](Transport::dial) on a [`Transport`]. A peer that
/// obtains a connection by listening is often referred to as the *listener* and the
/// peer that initiated the connection through dialing as the *dialer*, in
/// contrast to the traditional roles of *server* and *client*.
///
/// Most transports also provide a form of reliable delivery on the established
/// connections but the precise semantics of these guarantees depend on the
/// specific transport.
///
/// This trait is implemented for concrete connection-oriented transport protocols
/// like TCP or Unix Domain Sockets, but also on wrappers that add additional
/// functionality to the dialing or listening process (e.g. name resolution via
/// the DNS).
///
/// Additional protocols can be layered on top of the connections established
/// by a [`Transport`] through an upgrade mechanism that is initiated via
/// [`upgrade`](Transport::upgrade).
///
/// Note for implementors: Futures returned by [`Transport::dial`] should only
/// do work once polled for the first time. E.g. in the case of TCP, connecting
/// to the remote should not happen immediately on [`Transport::dial`] but only
/// once the returned [`Future`] is polled. The caller of [`Transport::dial`]
/// may call the method multiple times with a set of addresses, racing a subset
/// of the returned dials to success concurrently.
pub trait Transport {
    /// The result of a connection setup process, including protocol upgrades.
    ///
    /// Typically the output contains at least a handle to a data stream (i.e. a
    /// connection or a substream multiplexer on top of a connection) that
    /// provides APIs for sending and receiving data through the connection.
    type Output;

    /// An error that occurred during connection setup.
    type Error: Error;

    /// A pending [`Output`](Transport::Output) for an inbound connection,
    /// obtained from the [`Transport`] stream.
    ///
    /// After a connection has been accepted by the transport, it may need to go through
    /// asynchronous post-processing (i.e. protocol upgrade negotiations). Such
    /// post-processing should not block the `Listener` from producing the next
    /// connection, hence further connection setup proceeds asynchronously.
    /// Once a `ListenerUpgrade` future resolves it yields the [`Output`](Transport::Output)
    /// of the connection setup process.
    type ListenerUpgrade: Future<Output = Result<Self::Output, Self::Error>>;

    /// A pending [`Output`](Transport::Output) for an outbound connection,
    /// obtained from [dialing](Transport::dial).
    type Dial: Future<Output = Result<Self::Output, Self::Error>>;

    /// Listens on the given [`Multiaddr`] for inbound connections.
    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>>;

    /// Remove a listener.
    ///
    /// Return `true` if there was a listener with this Id, `false`
    /// otherwise.
    fn remove_listener(&mut self, id: ListenerId) -> bool;

    /// Dials the given [`Multiaddr`], returning a future for a pending outbound connection.
    ///
    /// If [`TransportError::MultiaddrNotSupported`] is returned, it may be desirable to
    /// try an alternative [`Transport`], if available.
    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>>;

    /// As [`Transport::dial`] but has the local node act as a listener on the outgoing connection.
    ///
    /// This option is needed for NAT and firewall hole punching.
    ///
    /// See [`ConnectedPoint::Dialer`](crate::connection::ConnectedPoint::Dialer) for related option.
    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>>;

    /// Poll for [`TransportEvent`]s.
    ///
    /// A [`TransportEvent::Incoming`] should be produced whenever a connection is received at the lowest
    /// level of the transport stack. The item must be a [`ListenerUpgrade`](Transport::ListenerUpgrade)
    /// future that resolves to an [`Output`](Transport::Output) value once all protocol upgrades have
    /// been applied.
    ///
    /// Transports are expected to produce [`TransportEvent::Incoming`] events only for
    /// listen addresses which have previously been announced via
    /// a [`TransportEvent::NewAddress`] event and which have not been invalidated by
    /// an [`TransportEvent::AddressExpired`] event yet.
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>>;

    /// Performs a transport-specific mapping of an address `observed` by a remote onto a
    /// local `listen` address to yield an address for the local node that may be reachable
    /// for other peers.
    ///
    /// This is relevant for transports where Network Address Translation (NAT) can occur
    /// so that e.g. the peer is observed at a different IP than the IP of the local
    /// listening address. See also [`address_translation`][crate::address_translation].
    ///
    /// Within [`libp2p::Swarm`](<https://docs.rs/libp2p/latest/libp2p/struct.Swarm.html>) this is
    /// used when extending the listening addresses of the local peer with external addresses
    /// observed by remote peers.
    /// On transports where this is not relevant (i.e. no NATs are present) `None` should be
    /// returned for the sake of de-duplication.
    ///
    /// Note: if the listen or observed address is not a valid address of this transport,
    /// `None` should be returned as well.
    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr>;

    /// Boxes the transport, including custom transport errors.
    fn boxed(self) -> boxed::Boxed<Self::Output>
    where
        Self: Sized + Send + Unpin + 'static,
        Self::Dial: Send + 'static,
        Self::ListenerUpgrade: Send + 'static,
        Self::Error: Send + Sync,
    {
        boxed::boxed(self)
    }

    /// Applies a function on the connections created by the transport.
    fn map<F, O>(self, f: F) -> map::Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, ConnectedPoint) -> O,
    {
        map::Map::new(self, f)
    }

    /// Applies a function on the errors generated by the futures of the transport.
    fn map_err<F, E>(self, f: F) -> map_err::MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error) -> E,
    {
        map_err::MapErr::new(self, f)
    }

    /// Adds a fallback transport that is used when encountering errors
    /// while establishing inbound or outbound connections.
    ///
    /// The returned transport will act like `self`, except that if `listen_on` or `dial`
    /// return an error then `other` will be tried.
    fn or_transport<U>(self, other: U) -> OrTransport<Self, U>
    where
        Self: Sized,
        U: Transport,
        <U as Transport>::Error: 'static,
    {
        OrTransport::new(self, other)
    }

    /// Applies a function producing an asynchronous result to every connection
    /// created by this transport.
    ///
    /// This function can be used for ad-hoc protocol upgrades or
    /// for processing or adapting the output for following configurations.
    ///
    /// For the high-level transport upgrade procedure, see [`Transport::upgrade`].
    fn and_then<C, F, O>(self, f: C) -> and_then::AndThen<Self, C>
    where
        Self: Sized,
        C: FnOnce(Self::Output, ConnectedPoint) -> F,
        F: TryFuture<Ok = O>,
        <F as TryFuture>::Error: Error + 'static,
    {
        and_then::AndThen::new(self, f)
    }

    /// Begins a series of protocol upgrades via an
    /// [`upgrade::Builder`](upgrade::Builder).
    fn upgrade(self, version: upgrade::Version) -> upgrade::Builder<Self>
    where
        Self: Sized,
        Self::Error: 'static,
    {
        upgrade::Builder::new(self, version)
    }
}

/// The ID of a single listener.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

impl ListenerId {
    /// Creates a new `ListenerId`.
    pub fn new() -> Self {
        ListenerId(rand::random())
    }
}

impl Default for ListenerId {
    fn default() -> Self {
        Self::new()
    }
}

/// Event produced by [`Transport`]s.
pub enum TransportEvent<TUpgr, TErr> {
    /// A new address is being listened on.
    NewAddress {
        /// The listener that is listening on the new address.
        listener_id: ListenerId,
        /// The new address that is being listened on.
        listen_addr: Multiaddr,
    },
    /// An address is no longer being listened on.
    AddressExpired {
        /// The listener that is no longer listening on the address.
        listener_id: ListenerId,
        /// The new address that is being listened on.
        listen_addr: Multiaddr,
    },
    /// A connection is incoming on one of the listeners.
    Incoming {
        /// The listener that produced the upgrade.
        listener_id: ListenerId,
        /// The produced upgrade.
        upgrade: TUpgr,
        /// Local connection address.
        local_addr: Multiaddr,
        /// Address used to send back data to the incoming client.
        send_back_addr: Multiaddr,
    },
    /// A listener closed.
    ListenerClosed {
        /// The ID of the listener that closed.
        listener_id: ListenerId,
        /// Reason for the closure. Contains `Ok(())` if the stream produced `None`, or `Err`
        /// if the stream produced an error.
        reason: Result<(), TErr>,
    },
    /// A listener errored.
    ///
    /// The listener will continue to be polled for new events and the event
    /// is for informational purposes only.
    ListenerError {
        /// The ID of the listener that errored.
        listener_id: ListenerId,
        /// The error value.
        error: TErr,
    },
}

impl<TUpgr, TErr> TransportEvent<TUpgr, TErr> {
    /// In case this [`TransportEvent`] is an upgrade, apply the given function
    /// to the upgrade and produce another transport event based the the function's result.
    pub fn map_upgrade<U>(self, map: impl FnOnce(TUpgr) -> U) -> TransportEvent<U, TErr> {
        match self {
            TransportEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr,
            } => TransportEvent::Incoming {
                listener_id,
                upgrade: map(upgrade),
                local_addr,
                send_back_addr,
            },
            TransportEvent::NewAddress {
                listen_addr,
                listener_id,
            } => TransportEvent::NewAddress {
                listen_addr,
                listener_id,
            },
            TransportEvent::AddressExpired {
                listen_addr,
                listener_id,
            } => TransportEvent::AddressExpired {
                listen_addr,
                listener_id,
            },
            TransportEvent::ListenerError { listener_id, error } => {
                TransportEvent::ListenerError { listener_id, error }
            }
            TransportEvent::ListenerClosed {
                listener_id,
                reason,
            } => TransportEvent::ListenerClosed {
                listener_id,
                reason,
            },
        }
    }

    /// In case this [`TransportEvent`] is an [`ListenerError`](TransportEvent::ListenerError),
    /// or [`ListenerClosed`](TransportEvent::ListenerClosed) apply the given function to the
    /// error and produce another transport event based on the function's result.
    pub fn map_err<E>(self, map_err: impl FnOnce(TErr) -> E) -> TransportEvent<TUpgr, E> {
        match self {
            TransportEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr,
            } => TransportEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr,
            },
            TransportEvent::NewAddress {
                listen_addr,
                listener_id,
            } => TransportEvent::NewAddress {
                listen_addr,
                listener_id,
            },
            TransportEvent::AddressExpired {
                listen_addr,
                listener_id,
            } => TransportEvent::AddressExpired {
                listen_addr,
                listener_id,
            },
            TransportEvent::ListenerError { listener_id, error } => TransportEvent::ListenerError {
                listener_id,
                error: map_err(error),
            },
            TransportEvent::ListenerClosed {
                listener_id,
                reason,
            } => TransportEvent::ListenerClosed {
                listener_id,
                reason: reason.map_err(map_err),
            },
        }
    }

    /// Returns `true` if this is an [`Incoming`](TransportEvent::Incoming) transport event.
    pub fn is_upgrade(&self) -> bool {
        matches!(self, TransportEvent::Incoming { .. })
    }

    /// Try to turn this transport event into the upgrade parts of the
    /// incoming connection.
    ///
    /// Returns `None` if the event is not actually an incoming connection,
    /// otherwise the upgrade and the remote address.
    pub fn into_incoming(self) -> Option<(TUpgr, Multiaddr)> {
        if let TransportEvent::Incoming {
            upgrade,
            send_back_addr,
            ..
        } = self
        {
            Some((upgrade, send_back_addr))
        } else {
            None
        }
    }

    /// Returns `true` if this is a [`TransportEvent::NewAddress`].
    pub fn is_new_address(&self) -> bool {
        matches!(self, TransportEvent::NewAddress { .. })
    }

    /// Try to turn this transport event into the new `Multiaddr`.
    ///
    /// Returns `None` if the event is not actually a [`TransportEvent::NewAddress`],
    /// otherwise the address.
    pub fn into_new_address(self) -> Option<Multiaddr> {
        if let TransportEvent::NewAddress { listen_addr, .. } = self {
            Some(listen_addr)
        } else {
            None
        }
    }

    /// Returns `true` if this is an [`TransportEvent::AddressExpired`].
    pub fn is_address_expired(&self) -> bool {
        matches!(self, TransportEvent::AddressExpired { .. })
    }

    /// Try to turn this transport event into the expire `Multiaddr`.
    ///
    /// Returns `None` if the event is not actually a [`TransportEvent::AddressExpired`],
    /// otherwise the address.
    pub fn into_address_expired(self) -> Option<Multiaddr> {
        if let TransportEvent::AddressExpired { listen_addr, .. } = self {
            Some(listen_addr)
        } else {
            None
        }
    }

    /// Returns `true` if this is an [`TransportEvent::ListenerError`] transport event.
    pub fn is_listener_error(&self) -> bool {
        matches!(self, TransportEvent::ListenerError { .. })
    }

    /// Try to turn this transport event into the listener error.
    ///
    /// Returns `None` if the event is not actually a [`TransportEvent::ListenerError`]`,
    /// otherwise the error.
    pub fn into_listener_error(self) -> Option<TErr> {
        if let TransportEvent::ListenerError { error, .. } = self {
            Some(error)
        } else {
            None
        }
    }
}

impl<TUpgr, TErr: fmt::Debug> fmt::Debug for TransportEvent<TUpgr, TErr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            TransportEvent::NewAddress {
                listener_id,
                listen_addr,
            } => f
                .debug_struct("TransportEvent::NewAddress")
                .field("listener_id", listener_id)
                .field("listen_addr", listen_addr)
                .finish(),
            TransportEvent::AddressExpired {
                listener_id,
                listen_addr,
            } => f
                .debug_struct("TransportEvent::AddressExpired")
                .field("listener_id", listener_id)
                .field("listen_addr", listen_addr)
                .finish(),
            TransportEvent::Incoming {
                listener_id,
                local_addr,
                ..
            } => f
                .debug_struct("TransportEvent::Incoming")
                .field("listener_id", listener_id)
                .field("local_addr", local_addr)
                .finish(),
            TransportEvent::ListenerClosed {
                listener_id,
                reason,
            } => f
                .debug_struct("TransportEvent::Closed")
                .field("listener_id", listener_id)
                .field("reason", reason)
                .finish(),
            TransportEvent::ListenerError { listener_id, error } => f
                .debug_struct("TransportEvent::ListenerError")
                .field("listener_id", listener_id)
                .field("error", error)
                .finish(),
        }
    }
}

/// An error during [dialing][Transport::dial] or [listening][Transport::listen_on]
/// on a [`Transport`].
#[derive(Debug, Clone)]
pub enum TransportError<TErr> {
    /// The [`Multiaddr`] passed as parameter is not supported.
    ///
    /// Contains back the same address.
    MultiaddrNotSupported(Multiaddr),

    /// Any other error that a [`Transport`] may produce.
    Other(TErr),
}

impl<TErr> TransportError<TErr> {
    /// Applies a function to the the error in [`TransportError::Other`].
    pub fn map<TNewErr>(self, map: impl FnOnce(TErr) -> TNewErr) -> TransportError<TNewErr> {
        match self {
            TransportError::MultiaddrNotSupported(addr) => {
                TransportError::MultiaddrNotSupported(addr)
            }
            TransportError::Other(err) => TransportError::Other(map(err)),
        }
    }
}

impl<TErr> fmt::Display for TransportError<TErr>
where
    TErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::MultiaddrNotSupported(addr) => {
                write!(f, "Multiaddr is not supported: {}", addr)
            }
            TransportError::Other(_) => Ok(()),
        }
    }
}

impl<TErr> Error for TransportError<TErr>
where
    TErr: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TransportError::MultiaddrNotSupported(_) => None,
            TransportError::Other(err) => Some(err),
        }
    }
}
