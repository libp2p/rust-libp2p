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

//! Handles entering a connection with a peer.
//!
//! The two main elements of this module are the `Transport` and `ConnectionUpgrade` traits.
//! `Transport` is implemented on objects that allow dialing and listening. `ConnectionUpgrade` is
//! implemented on objects that make it possible to upgrade a connection (for example by adding an
//! encryption middleware to the connection).
//!
//! Thanks to the `Transport::or_transport`, `Transport::with_upgrade` and
//! `UpgradedNode::or_upgrade` methods, you can combine multiple transports and/or upgrades
//! together in a complex chain of protocols negotiation.

use futures::prelude::*;
use multiaddr::Multiaddr;
use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

pub mod and_then;
pub mod choice;
pub mod denied;
pub mod dummy;
pub mod interruptible;
pub mod map;
pub mod map_err;
pub mod memory;
pub mod muxed;
pub mod upgrade;

pub use self::choice::OrTransport;
pub use self::denied::DeniedTransport;
pub use self::dummy::DummyMuxing;
pub use self::memory::connector;
pub use self::muxed::MuxedTransport;
pub use self::upgrade::UpgradedNode;

/// A transport is an object that can be used to produce connections by listening or dialing a
/// peer.
///
/// This trait is implemented on concrete transports (eg. TCP, UDP, etc.), but also on wrappers
/// around them.
///
/// > **Note**: The methods of this trait use `self` and not `&self` or `&mut self`. In other
/// >           words, listening or dialing consumes the transport object. This has been designed
/// >           so that you would implement this trait on `&Foo` or `&mut Foo` instead of directly
/// >           on `Foo`.
pub trait Transport {
    /// The raw connection to a peer.
    type Output;

    /// The listener produces incoming connections.
    ///
    /// An item should be produced whenever a connection is received at the lowest level of the
    /// transport stack. The item is a `Future` that is signalled once some pre-processing has
    /// taken place, and that connection has been upgraded to the wanted protocols.
    type Listener: Stream<Item = Self::ListenerUpgrade, Error = IoError>;

    /// Future that produces the multiaddress of the remote.
    type MultiaddrFuture: Future<Item = Multiaddr, Error = IoError>;

    /// After a connection has been received, we may need to do some asynchronous pre-processing
    /// on it (eg. an intermediary protocol negotiation). While this pre-processing takes place, we
    /// want to be able to continue polling on the listener.
    // TODO: we could move the `MultiaddrFuture` to the `Listener` trait
    type ListenerUpgrade: Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>;

    /// A future which indicates that we are currently dialing to a peer.
    type Dial: Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>;

    /// Listen on the given multiaddr. Returns a stream of incoming connections, plus a modified
    /// version of the `Multiaddr`. This new `Multiaddr` is the one that that should be advertised
    /// to other nodes, instead of the one passed as parameter.
    ///
    /// Returns the address back if it isn't supported.
    ///
    /// > **Note**: The reason why we need to change the `Multiaddr` on success is to handle
    /// >             situations such as turning `/ip4/127.0.0.1/tcp/0` into
    /// >             `/ip4/127.0.0.1/tcp/<actual port>`.
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)>
    where
        Self: Sized;

    /// Dial to the given multi-addr.
    ///
    /// Returns either a future which may resolve to a connection, or gives back the multiaddress.
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)>
    where
        Self: Sized;

    /// Takes a multiaddress we're listening on (`server`), and tries to convert it to an
    /// externally-visible multiaddress. In order to do so, we pass an `observed` address which
    /// a remote node observes for one of our dialers.
    ///
    /// For example, if `server` is `/ip4/0.0.0.0/tcp/3000` and `observed` is
    /// `/ip4/80.81.82.83/tcp/29601`, then we should return `/ip4/80.81.82.83/tcp/3000`. Each
    /// implementation of `Transport` is only responsible for handling the protocols it supports.
    ///
    /// Returns `None` if nothing can be determined. This happens if this trait implementation
    /// doesn't recognize the protocols, or if `server` and `observed` are related.
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr>;

    /// Applies a function on the output of the `Transport`.
    #[inline]
    fn map<F, O>(self, map: F) -> map::Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Endpoint) -> O + Clone + 'static,        // TODO: 'static :-/
    {
        map::Map::new(self, map)
    }

    /// Applies a function on the errors generated by the futures of the `Transport`.
    #[inline]
    fn map_err<F>(self, map_err: F) -> map_err::MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(IoError) -> IoError + Clone + 'static,        // TODO: 'static :-/
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
    fn with_upgrade<U>(self, upgrade: U) -> UpgradedNode<Self, U>
    where
        Self: Sized,
        Self::Output: AsyncRead + AsyncWrite,
        U: ConnectionUpgrade<Self::Output, Self::MultiaddrFuture>,
    {
        UpgradedNode::new(self, upgrade)
    }

    /// Wraps this transport inside an upgrade. Whenever a connection that uses this transport
    /// is established, it is wrapped inside the upgrade.
    ///
    /// > **Note**: The concept of an *upgrade* for example includes middlewares such *secio*
    /// >           (communication encryption), *multiplex*, but also a protocol handler.
    #[inline]
    fn and_then<C, F, O, Maf>(self, upgrade: C) -> and_then::AndThen<Self, C>
    where
        Self: Sized,
        C: FnOnce(Self::Output, Endpoint, Self::MultiaddrFuture) -> F + Clone + 'static,
        F: Future<Item = (O, Maf), Error = IoError> + 'static,
        Maf: Future<Item = Multiaddr, Error = IoError> + 'static,
    {
        and_then::and_then(self, upgrade)
    }

    /// Builds a dummy implementation of `MuxedTransport` that uses this transport.
    ///
    /// The resulting object will not actually use muxing. This means that dialing the same node
    /// twice will result in two different connections instead of two substreams on the same
    /// connection.
    #[inline]
    fn with_dummy_muxing(self) -> DummyMuxing<Self>
    where
        Self: Sized,
    {
        DummyMuxing::new(self)
    }

    /// Wraps around the `Transport` and makes it interruptible.
    #[inline]
    fn interruptible(self) -> (interruptible::Interruptible<Self>, interruptible::Interrupt)
    where
        Self: Sized,
    {
        interruptible::Interruptible::new(self)
    }
}
