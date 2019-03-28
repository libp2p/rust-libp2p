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

use crate::{InboundUpgrade, OutboundUpgrade, nodes::raw_swarm::ConnectedPoint};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{error, fmt};
use std::time::Duration;
use tokio_io::{AsyncRead, AsyncWrite};

pub mod and_then;
pub mod boxed;
pub mod choice;
pub mod dummy;
pub mod map;
pub mod map_err;
pub mod memory;
pub mod timeout;
pub mod upgrade;

pub use self::choice::OrTransport;
pub use self::memory::MemoryTransport;
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
/// [`with_upgrade`](Transport::with_upgrade) and optionally followed by further upgrades
/// through chaining calls to [`with_upgrade`](Transport::with_upgrade) and
/// [`and_then`](Transport::and_then). Thereby every upgrade yields a new [`Transport`]
/// whose connection setup incorporates all earlier upgrades followed by the new upgrade,
/// i.e. the order of the upgrades is significant.
///
/// > **Note**: The methods of this trait use `self` and not `&self` or `&mut self`. In other
/// >           words, listening or dialing consumes the transport object. This has been designed
/// >           so that you would implement this trait on `&Foo` or `&mut Foo` instead of directly
/// >           on `Foo`.
pub trait Transport {
    /// The result of a connection setup process, including protocol upgrades.
    ///
    /// Typically the output contains at least a handle to a data stream (i.e. a
    /// connection or a substream multiplexer on top of a connection) that
    /// provides APIs for sending and receiving data through the connection.
    type Output;

    /// An error that occurred during connection setup.
    type Error: error::Error;

    /// A stream of [`Output`](Transport::Output)s for inbound connections.
    ///
    /// An item should be produced whenever a connection is received at the lowest level of the
    /// transport stack. The item must be a [`ListenerUpgrade`](Transport::ListenerUpgrade) future
    /// that resolves to an [`Output`](Transport::Output) value once all protocol upgrades
    /// have been applied.
    type Listener: Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = Self::Error>;

    /// A pending [`Output`](Transport::Output) for an inbound connection,
    /// obtained from the [`Listener`](Transport::Listener) stream.
    ///
    /// After a connection has been accepted by the transport, it may need to go through
    /// asynchronous post-processing (i.e. protocol upgrade negotiations). Such
    /// post-processing should not block the `Listener` from producing the next
    /// connection, hence further connection setup proceeds asynchronously.
    /// Once a `ListenerUpgrade` future resolves it yields the [`Output`](Transport::Output)
    /// of the connection setup process.
    type ListenerUpgrade: Future<Item = Self::Output, Error = Self::Error>;

    /// A pending [`Output`](Transport::Output) for an outbound connection,
    /// obtained from [dialing](Transport::dial).
    type Dial: Future<Item = Self::Output, Error = Self::Error>;

    /// Listens on the given [`Multiaddr`], producing a stream of pending, inbound connections.
    ///
    /// > **Note**: The new [`Multiaddr`] that is returned alongside the connection stream
    /// > is the address that should be advertised to other nodes, as the given address
    /// > may be subject to changes such as an OS-assigned port number.
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>>
    where
        Self: Sized;

    /// Dials the given [`Multiaddr`], returning a future for a pending outbound connection.
    ///
    /// If [`TransportError::MultiaddrNotSupported`] is returned, it may be desirable to
    /// try an alternative [`Transport`], if available.
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>>
    where
        Self: Sized;

    /// Takes a [`Multiaddr`] that represents a listening address together with an
    /// an address observed by another node and tries to incoporate information
    /// from the observed address into the listening address, yielding an
    /// externally-visible address.
    ///
    /// In order to do so, `observed` must be an address that a remote node observes on an
    /// inbound connection from the local node. Each [`Transport`] implementation is only
    /// responsible for handling the protocols it supports and should only consider the
    /// prefix of `observed` necessary to perform the address translation
    /// (e.g. `/ip4/80.81.82.83`) but should otherwise preserve `server` as is. For example,
    /// if `server` is the address `/ip4/0.0.0.0/tcp/3000` and `observed` is the address
    /// `/ip4/80.81.82.83/tcp/29601`, then the address `/ip4/80.81.82.83/tcp/3000` should be
    /// returned.
    ///
    /// Returns `None` if the transport does not recognize a protocol, or if `server` and
    /// `observed` are unrelated addresses.
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr>;

    /// Turns this `Transport` into an abstract boxed transport.
    #[inline]
    fn boxed(self) -> boxed::Boxed<Self::Output, Self::Error>
    where Self: Sized + Clone + Send + Sync + 'static,
          Self::Dial: Send + 'static,
          Self::Listener: Send + 'static,
          Self::ListenerUpgrade: Send + 'static,
    {
        boxed::boxed(self)
    }

    /// Applies a function on the connections created by the transport.
    #[inline]
    fn map<F, O>(self, map: F) -> map::Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, ConnectedPoint) -> O + Clone
    {
        map::Map::new(self, map)
    }

    /// Applies a function on the errors generated by the futures of the transport.
    #[inline]
    fn map_err<F, TNewErr>(self, map_err: F) -> map_err::MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error) -> TNewErr + Clone
    {
        map_err::MapErr::new(self, map_err)
    }

    /// Builds a new transport that falls back to another transport when
    /// encountering errors on dialing or listening for connections.
    ///
    /// The returned transport will act like `self`, except that if `listen_on` or `dial`
    /// return an error then `other` will be tried.
    #[inline]
    fn or_transport<T>(self, other: T) -> OrTransport<Self, T>
    where
        Self: Sized,
    {
        OrTransport::new(self, other)
    }

    /// Wraps this transport inside an [`Upgrade`].
    ///
    /// Whenever an inbound or outbound connection is established by this
    /// transport, the upgrade is applied on the current state of the
    /// connection (which may have already gone through previous upgrades)
    /// as an [`upgrade::InboundUpgrade`] or [`upgrade::OutboundUpgrade`],
    /// respectively.
    #[inline]
    fn with_upgrade<U, O, E>(self, upgrade: U) -> Upgrade<Self, U>
    where
        Self: Sized,
        Self::Output: AsyncRead + AsyncWrite,
        U: InboundUpgrade<Self::Output, Output = O, Error = E>,
        U: OutboundUpgrade<Self::Output, Output = O, Error = E>
    {
        Upgrade::new(self, upgrade)
    }

    /// Applies a function producing an asynchronous result to every connection
    /// created by this transport.
    ///
    /// This function can be used for ad-hoc protocol upgrades on a transport or
    /// for processing or adapting the output of an earlier upgrade before
    /// applying the next upgrade.
    #[inline]
    fn and_then<C, F, O>(self, upgrade: C) -> and_then::AndThen<Self, C>
    where
        Self: Sized,
        C: FnOnce(Self::Output, ConnectedPoint) -> F + Clone,
        F: IntoFuture<Item = O>
    {
        and_then::AndThen::new(self, upgrade)
    }

    /// Adds a timeout to the connection setup (including upgrades) for all inbound
    /// and outbound connection attempts.
    #[inline]
    fn with_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::new(self, timeout)
    }

    /// Adds a timeout to the connection setup (including upgrades) for all outbound
    /// connection attempts.
    #[inline]
    fn with_outbound_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::with_outgoing_timeout(self, timeout)
    }

    /// Adds a timeout to the connection setup (including upgrades) for all inbound
    /// connection attempts.
    #[inline]
    fn with_inbound_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::with_ingoing_timeout(self, timeout)
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
    #[inline]
    pub fn map<TNewErr>(self, map: impl FnOnce(TErr) -> TNewErr) -> TransportError<TNewErr> {
        match self {
            TransportError::MultiaddrNotSupported(addr) => TransportError::MultiaddrNotSupported(addr),
            TransportError::Other(err) => TransportError::Other(map(err)),
        }
    }
}

impl<TErr> fmt::Display for TransportError<TErr>
where TErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::MultiaddrNotSupported(addr) => write!(f, "Multiaddr is not supported: {}", addr),
            TransportError::Other(err) => write!(f, "{}", err),
        }
    }
}

impl<TErr> error::Error for TransportError<TErr>
where TErr: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            TransportError::MultiaddrNotSupported(_) => None,
            TransportError::Other(err) => Some(err),
        }
    }
}
