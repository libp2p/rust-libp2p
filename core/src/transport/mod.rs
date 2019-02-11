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

//! Handles entering a connection with a peer.
//!
//! The main element of this module is the `Transport` trait. It is implemented on objects that
//! allow dialing and listening.
//!
//! The rest of the module holds combinators that allow tweaking an implementation of `Transport`,
//! combine multiple transports together, or combine a transport with an upgrade.

use crate::{InboundUpgrade, OutboundUpgrade, nodes::raw_swarm::ConnectedPoint};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{error, fmt};
use std::time::Duration;
use tokio_io::{AsyncRead, AsyncWrite};

pub mod and_then;
pub mod boxed;
pub mod choice;
pub mod map;
pub mod map_err;
pub mod memory;
pub mod timeout;
pub mod upgrade;

pub use self::choice::OrTransport;
pub use self::memory::connector;
pub use self::upgrade::Upgrade;

/// A transport is an object that can be used to produce connections by listening or dialing a
/// peer.
///
/// This trait is implemented on concrete transports (e.g. TCP, UDP, etc.), but also on wrappers
/// around them.
///
/// > **Note**: The methods of this trait use `self` and not `&self` or `&mut self`. In other
/// >           words, listening or dialing consumes the transport object. This has been designed
/// >           so that you would implement this trait on `&Foo` or `&mut Foo` instead of directly
/// >           on `Foo`.
pub trait Transport {
    /// The raw connection to a peer.
    type Output;
    /// Error that can happen when dialing or listening.
    type Error: error::Error;

    /// The listener produces incoming connections.
    ///
    /// An item should be produced whenever a connection is received at the lowest level of the
    /// transport stack. The item is a `Future` that is signalled once some pre-processing has
    /// taken place, and that connection has been upgraded to the wanted protocols.
    type Listener: Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = Self::Error>;

    /// After a connection has been received, we may need to do some asynchronous pre-processing
    /// on it (e.g. an intermediary protocol negotiation). While this pre-processing takes place,
    /// we want to be able to continue polling on the listener.
    type ListenerUpgrade: Future<Item = Self::Output, Error = Self::Error>;

    /// A future which indicates that we are currently dialing to a peer.
    type Dial: Future<Item = Self::Output, Error = Self::Error>;

    /// Listen on the given multiaddr. Returns a stream of incoming connections, plus a modified
    /// version of the `Multiaddr`. This new `Multiaddr` is the one that that should be advertised
    /// to other nodes, instead of the one passed as parameter.
    ///
    /// > **Note**: The reason why we need to change the `Multiaddr` on success is to handle
    /// >             situations such as turning `/ip4/127.0.0.1/tcp/0` into
    /// >             `/ip4/127.0.0.1/tcp/<actual port>`.
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>>
    where
        Self: Sized;

    /// Dial the given multi-addr.
    ///
    /// Returns either a future which may resolve to a connection.
    ///
    /// If `MultiaddrNotSupported` is returned, then caller can try another implementation of
    /// `Transport` if there is any. If instead an error is returned, then we assume that there is
    /// no point in trying another `Transport`.
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>>
    where
        Self: Sized;

    /// Takes a multiaddress we're listening on (`server`), and tries to convert it to an
    /// externally-visible multiaddress. In order to do so, we pass an `observed` address which
    /// a remote node observes for one of our dialers.
    ///
    /// For example, if `server` is `/ip4/0.0.0.0/tcp/3000` and `observed` is
    /// `/ip4/80.81.82.83/tcp/29601`, then we should return `/ip4/80.81.82.83/tcp/3000`.
    ///
    /// Each implementation of `Transport` is only responsible for handling the protocols it
    /// supports and should only consider the prefix of `observed` necessary to perform the
    /// address translation (e.g. `/ip4/80.81.82.83`) but should otherwise preserve `server`
    /// as is.
    ///
    /// Returns `None` if nothing can be determined. This happens if this trait implementation
    /// doesn't recognize the protocols, or if `server` and `observed` are not related.
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

    /// Applies a function on the output of the `Transport`.
    #[inline]
    fn map<F, O>(self, map: F) -> map::Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, ConnectedPoint) -> O + Clone
    {
        map::Map::new(self, map)
    }

    /// Applies a function on the errors generated by the futures of the `Transport`.
    #[inline]
    fn map_err<F, TNewErr>(self, map_err: F) -> map_err::MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error) -> TNewErr + Clone
    {
        map_err::MapErr::new(self, map_err)
    }

    /// Builds a new struct that implements `Transport` that contains both `self` and `other`.
    ///
    /// The returned object will redirect its calls to `self`, except that if `listen_on` or `dial`
    /// return an error then `other` will be tried.
    #[inline]
    fn or_transport<T>(self, other: T) -> OrTransport<Self, T>
    where
        Self: Sized,
    {
        OrTransport::new(self, other)
    }

    /// Wraps this transport inside an upgrade. Whenever a connection that uses this transport
    /// is established, it is wrapped inside the upgrade.
    ///
    /// > **Note**: The concept of an *upgrade* for example includes middlewares such *secio*
    /// >           (communication encryption), *multiplex*, but also a protocol handler.
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

    /// Wraps this transport inside an upgrade. Whenever a connection that uses this transport
    /// is established, it is wrapped inside the upgrade.
    ///
    /// > **Note**: The concept of an *upgrade* for example includes middlewares such *secio*
    /// >           (communication encryption), *multiplex*, but also a protocol handler.
    #[inline]
    fn and_then<C, F, O>(self, upgrade: C) -> and_then::AndThen<Self, C>
    where
        Self: Sized,
        C: FnOnce(Self::Output, ConnectedPoint) -> F + Clone,
        F: IntoFuture<Item = O>
    {
        and_then::AndThen::new(self, upgrade)
    }

    /// Adds a timeout to the connection and upgrade steps for all the sockets created by
    /// the transport.
    #[inline]
    fn with_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::new(self, timeout)
    }

    /// Adds a timeout to the connection and upgrade steps for all the outgoing sockets created
    /// by the transport.
    #[inline]
    fn with_outbound_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::with_outgoing_timeout(self, timeout)
    }

    /// Adds a timeout to the connection and upgrade steps for all the incoming sockets created
    /// by the transport.
    #[inline]
    fn with_inbound_timeout(self, timeout: Duration) -> timeout::TransportTimeout<Self>
    where
        Self: Sized,
    {
        timeout::TransportTimeout::with_ingoing_timeout(self, timeout)
    }
}

/// Error that can happen when dialing or listening.
#[derive(Debug, Clone)]
pub enum TransportError<TErr> {
    /// The `Multiaddr` passed as parameter is not supported.
    ///
    /// Contains back the same address.
    MultiaddrNotSupported(Multiaddr),

    /// Any other error that the `Transport` may produce.
    Other(TErr),
}

impl<TErr> TransportError<TErr> {
    /// Applies a map to the `Other` variant.
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
