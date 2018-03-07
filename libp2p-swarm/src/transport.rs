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

use bytes::Bytes;
use connection_reuse::ConnectionReuse;
use futures::{Async, Poll, stream, Stream};
use futures::future::{self, FromErr, Future, FutureResult, IntoFuture};
use multiaddr::Multiaddr;
use multistream_select;
use muxing::StreamMuxer;
use std::io::{Cursor, Error as IoError, ErrorKind as IoErrorKind, Read, Write};
use std::iter;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};

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
pub trait Transport<Config> {
	/// The raw connection to a peer.
	type RawConn: AsyncRead + AsyncWrite;

	/// The listener produces incoming connections.
	/// 
	/// An item should be produced whenever a connection is received at the lowest level of the
	/// transport stack. The item is a `Future` that is signalled once some pre-processing has
	/// taken place, and that connection has been upgraded to the wanted protocols.
	type Listener: Stream<Item = Self::ListenerUpgrade, Error = IoError>;

	/// After a connection has been received, we may need to do some asynchronous pre-processing
	/// on it (eg. an intermediary protocol negotiation). While this pre-processing takes place, we
	/// want to be able to continue polling on the listener.
	type ListenerUpgrade: Future<Item = (Self::RawConn, Multiaddr), Error = IoError>;

	/// A future which indicates that we are currently dialing to a peer.
	type Dial: IntoFuture<Item = (Self::RawConn, Multiaddr), Error = IoError>;

	/// Listen on the given multiaddr. Returns a stream of incoming connections, plus a modified
	/// version of the `Multiaddr`. This new `Multiaddr` is the one that that should be advertised
	/// to other nodes, instead of the one passed as parameter.
	///
	/// Returns the address back if it isn't supported.
	///
	/// > **Note**: The reason why we need to change the `Multiaddr` on success is to handle
	/// >             situations such as turning `/ip4/127.0.0.1/tcp/0` into
	/// >             `/ip4/127.0.0.1/tcp/<actual port>`.
	fn listen_on(self, addr: Multiaddr, config: Config) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)>
	where
		Self: Sized;

	/// Dial to the given multi-addr.
	///
	/// Returns either a future which may resolve to a connection, or gives back the multiaddress.
	fn dial(self, addr: Multiaddr, config: Config) -> Result<Self::Dial, (Self, Multiaddr)>
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

	/// Builds a new struct that implements `Transport` that contains both `self` and `other`.
	///
	/// The returned object will redirect its calls to `self`, except that if `listen_on` or `dial`
	/// return an error then `other` will be tried.
	#[inline]
	fn or_transport<T>(self, other: T) -> OrTransport<Self, T>
	where
		Self: Sized,
	{
		OrTransport(self, other)
	}

	/// Wraps this transport inside an upgrade. Whenever a connection that uses this transport
	/// is established, it is wrapped inside the upgrade.
	///
	/// > **Note**: The concept of an *upgrade* for example includes middlewares such *secio*
	/// >           (communication encryption), *multiplex*, but also a protocol handler.
	#[inline]
	fn with_upgrade<U>(self, upgrade: U) -> UpgradedNode<Self, U, Config>
	where
		Self: Sized,
		U: ConnectionUpgrade<Self::RawConn, Config>,
	{
		UpgradedNode {
			transports: self,
			upgrade: upgrade,
			config: Default::default(),
		}
	}

	/// Builds a dummy implementation of `MuxedTransport` that uses this transport.
	/// 
	/// The resulting object will not actually use muxing. This means that dialing the same node
	/// twice will result in two different connections instead of two substreams on the same
	/// connection.
	#[inline]
	fn with_dummy_muxing(self) -> DummyMuxing<Self>
		where Self: Sized
	{
		DummyMuxing { inner: self }
	}
}

/// Extension trait for `Transport`. Implemented on structs that provide a `Transport` on which
/// the dialed node can dial you back.
pub trait MuxedTransport<Conf>: Transport<Conf> {
	/// Future resolving to an incoming connection.
	type Incoming: Future<Item = (Self::RawConn, Multiaddr), Error = IoError>;

	/// Returns the next incoming substream opened by a node that we dialed ourselves.
	/// 
	/// > **Note**: Doesn't produce incoming substreams coming from addresses we are listening on.
	/// >			This only concerns nodes that we dialed with `dial()`.
	fn next_incoming(self, conf: Conf) -> Self::Incoming
		where Self: Sized;

	/// Returns a stream of incoming connections.
	#[inline]
	fn incoming(self, conf: Conf) -> stream::AndThen<
		stream::Repeat<(Self, Conf), IoError>,
		fn((Self, Conf)) -> Self::Incoming,
		Self::Incoming
	>
	where
		Self: Sized + Clone,
		Conf: Clone
	{
		stream::repeat((self, conf)).and_then(|(me, conf)| me.next_incoming(conf))
	}
}

/// Dummy implementation of `Transport` that just denies every single attempt.
#[derive(Debug, Copy, Clone)]
pub struct DeniedTransport;

impl<Conf> Transport<Conf> for DeniedTransport {
	// TODO: could use `!` for associated types once stable
	type RawConn = Cursor<Vec<u8>>;
	type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
	type ListenerUpgrade = Box<Future<Item = (Self::RawConn, Multiaddr), Error = IoError>>;
	type Dial = Box<Future<Item = (Self::RawConn, Multiaddr), Error = IoError>>;

	#[inline]
	fn listen_on(self, addr: Multiaddr, _: Conf) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
		Err((DeniedTransport, addr))
	}

	#[inline]
	fn dial(self, addr: Multiaddr, _: Conf) -> Result<Self::Dial, (Self, Multiaddr)> {
		Err((DeniedTransport, addr))
	}

	#[inline]
	fn nat_traversal(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
		None
	}
}

impl<Conf> MuxedTransport<Conf> for DeniedTransport {
	type Incoming = future::Empty<(Self::RawConn, Multiaddr), IoError>;

	#[inline]
	fn next_incoming(self, _: Conf) -> Self::Incoming {
		future::empty()
	}
}

/// Struct returned by `or_transport()`.
#[derive(Debug, Copy, Clone)]
pub struct OrTransport<A, B>(A, B);

impl<A, B, Conf> Transport<Conf> for OrTransport<A, B>
where
	A: Transport<Conf>,
	B: Transport<Conf>,
	Conf: Clone,
{
	type RawConn = EitherSocket<A::RawConn, B::RawConn>;
	type Listener = EitherListenStream<A::Listener, B::Listener>;
	type ListenerUpgrade = EitherListenUpgrade<A::ListenerUpgrade, B::ListenerUpgrade>;
	type Dial = EitherListenUpgrade<<A::Dial as IntoFuture>::Future, <B::Dial as IntoFuture>::Future>;

	fn listen_on(self, addr: Multiaddr, conf: Conf) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
		let (first, addr) = match self.0.listen_on(addr, conf.clone()) {
			Ok((connec, addr)) => return Ok((EitherListenStream::First(connec), addr)),
			Err(err) => err,
		};

		match self.1.listen_on(addr, conf) {
			Ok((connec, addr)) => Ok((EitherListenStream::Second(connec), addr)),
			Err((second, addr)) => Err((OrTransport(first, second), addr)),
		}
	}

	fn dial(self, addr: Multiaddr, conf: Conf) -> Result<Self::Dial, (Self, Multiaddr)> {
		let (first, addr) = match self.0.dial(addr, conf.clone()) {
			Ok(connec) => return Ok(EitherTransportFuture::First(connec.into_future())),
			Err(err) => err,
		};

		match self.1.dial(addr, conf) {
			Ok(connec) => Ok(EitherTransportFuture::Second(connec.into_future())),
			Err((second, addr)) => Err((OrTransport(first, second), addr)),
		}
	}

	#[inline]
	fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
		let first = self.0.nat_traversal(server, observed);
		if let Some(first) = first {
			return Some(first);
		}

		self.1.nat_traversal(server, observed)
	}
}

/// Implementation of `ConnectionUpgrade`. Convenient to use with small protocols.
#[derive(Debug)]
pub struct SimpleProtocol<F> {
	name: Bytes,
	// Note: we put the closure `F` in an `Arc` because Rust closures aren't automatically clonable
	// yet.
	upgrade: Arc<F>,
}

impl<F> SimpleProtocol<F> {
	/// Builds a `SimpleProtocol`.
	#[inline]
	pub fn new<N>(name: N, upgrade: F) -> SimpleProtocol<F>
	where
		N: Into<Bytes>,
	{
		SimpleProtocol {
			name: name.into(),
			upgrade: Arc::new(upgrade),
		}
	}
}

impl<F> Clone for SimpleProtocol<F> {
	#[inline]
	fn clone(&self) -> Self {
		SimpleProtocol {
			name: self.name.clone(),
			upgrade: self.upgrade.clone(),
		}
	}
}

impl<A, B, Conf> MuxedTransport<Conf> for OrTransport<A, B>
where
	A: MuxedTransport<Conf>,
	B: MuxedTransport<Conf>,
	A::Incoming: 'static, // TODO: meh :-/
	B::Incoming: 'static, // TODO: meh :-/
	Conf: Clone,
{
	type Incoming = Box<Future<Item = (EitherSocket<A::RawConn, B::RawConn>, Multiaddr), Error = IoError>>;

	#[inline]
	fn next_incoming(self, conf: Conf) -> Self::Incoming {
		let first = self.0.next_incoming(conf.clone()).map(|(out, addr)| (EitherSocket::First(out), addr));
		let second = self.1.next_incoming(conf).map(|(out, addr)| (EitherSocket::Second(out), addr));
		let future = first.select(second)
			.map(|(i, _)| i)
			.map_err(|(e, _)| e);
		Box::new(future) as Box<_>
	}
}

impl<C, F, O, Conf> ConnectionUpgrade<C, Conf> for SimpleProtocol<F>
where
	C: AsyncRead + AsyncWrite,
	F: Fn(C) -> O,
	O: IntoFuture<Error = IoError>,
{
	type NamesIter = iter::Once<(Bytes, ())>;
	type UpgradeIdentifier = ();

	#[inline]
	fn protocol_names(&self) -> Self::NamesIter {
		iter::once((self.name.clone(), ()))
	}

	type Output = O::Item;
	type Future = FromErr<O::Future, IoError>;

	#[inline]
	fn upgrade(self, socket: C, _: (), _: Endpoint, _: &Multiaddr, _: Conf) -> Self::Future {
		let upgrade = &self.upgrade;
		upgrade(socket).into_future().from_err()
	}
}

/// Implements `Stream` and dispatches all method calls to either `First` or `Second`.
#[derive(Debug, Copy, Clone)]
pub enum EitherListenStream<A, B> {
	First(A),
	Second(B),
}

impl<AStream, BStream, AInner, BInner> Stream for EitherListenStream<AStream, BStream>
where
	AStream: Stream<Item = AInner, Error = IoError>,
	BStream: Stream<Item = BInner, Error = IoError>,
{
	type Item = EitherListenUpgrade<AInner, BInner>;
	type Error = IoError;

	#[inline]
	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		match self {
			&mut EitherListenStream::First(ref mut a) => a.poll()
				.map(|i| i.map(|v| v.map(EitherListenUpgrade::First))),
			&mut EitherListenStream::Second(ref mut a) => a.poll()
				.map(|i| i.map(|v| v.map(EitherListenUpgrade::Second))),
		}
	}
}

/// Implements `Stream` and dispatches all method calls to either `First` or `Second`.
#[derive(Debug, Copy, Clone)]
pub enum EitherIncomingStream<A, B> {
	First(A),
	Second(B),
}

impl<A, B, Sa, Sb> Stream for EitherIncomingStream<A, B>
where
	A: Stream<Item = Sa, Error = IoError>,
	B: Stream<Item = Sb, Error = IoError>,
{
	type Item = EitherSocket<Sa, Sb>;
	type Error = IoError;

	#[inline]
	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		match self {
			&mut EitherIncomingStream::First(ref mut a) => {
				a.poll().map(|i| i.map(|v| v.map(EitherSocket::First)))
			}
			&mut EitherIncomingStream::Second(ref mut a) => {
				a.poll().map(|i| i.map(|v| v.map(EitherSocket::Second)))
			}
		}
	}
}

// TODO: This type is needed because of the lack of `impl Trait` in stable Rust.
//		 If Rust had impl Trait we could use the Either enum from the futures crate and add some
//		 modifiers to it. This custom enum is a combination of Either and these modifiers.
#[derive(Debug, Copy, Clone)]
pub enum EitherListenUpgrade<A, B> {
	First(A),
	Second(B),
}

impl<A, B, Ao, Bo> Future for EitherListenUpgrade<A, B>
where
	A: Future<Item = (Ao, Multiaddr), Error = IoError>,
	B: Future<Item = (Bo, Multiaddr), Error = IoError>,
{
	type Item = (EitherSocket<Ao, Bo>, Multiaddr);
	type Error = IoError;

	#[inline]
	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		match self {
			&mut EitherListenUpgrade::First(ref mut a) => {
				let (item, addr) = try_ready!(a.poll());
				Ok(Async::Ready((EitherSocket::First(item), addr)))
			}
			&mut EitherListenUpgrade::Second(ref mut b) => {
				let (item, addr) = try_ready!(b.poll());
				Ok(Async::Ready((EitherSocket::Second(item), addr)))
			}
		}
	}
}

/// Implements `Future` and redirects calls to either `First` or `Second`.
///
/// Additionally, the output will be wrapped inside a `EitherSocket`.
// TODO: This type is needed because of the lack of `impl Trait` in stable Rust.
//		 If Rust had impl Trait we could use the Either enum from the futures crate and add some
//		 modifiers to it. This custom enum is a combination of Either and these modifiers.
#[derive(Debug, Copy, Clone)]
pub enum EitherTransportFuture<A, B> {
	First(A),
	Second(B),
}

impl<A, B> Future for EitherTransportFuture<A, B>
where
	A: Future<Error = IoError>,
	B: Future<Error = IoError>,
{
	type Item = EitherSocket<A::Item, B::Item>;
	type Error = IoError;

	#[inline]
	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		match self {
			&mut EitherTransportFuture::First(ref mut a) => {
				let item = try_ready!(a.poll());
				Ok(Async::Ready(EitherSocket::First(item)))
			}
			&mut EitherTransportFuture::Second(ref mut b) => {
				let item = try_ready!(b.poll());
				Ok(Async::Ready(EitherSocket::Second(item)))
			}
		}
	}
}

/// Implements `AsyncRead` and `AsyncWrite` and dispatches all method calls to either `First` or
/// `Second`.
#[derive(Debug, Copy, Clone)]
pub enum EitherSocket<A, B> {
	First(A),
	Second(B),
}

impl<A, B> AsyncRead for EitherSocket<A, B>
where
	A: AsyncRead,
	B: AsyncRead,
{
	#[inline]
	unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
		match self {
			&EitherSocket::First(ref a) => a.prepare_uninitialized_buffer(buf),
			&EitherSocket::Second(ref b) => b.prepare_uninitialized_buffer(buf),
		}
	}
}

impl<A, B> Read for EitherSocket<A, B>
where
	A: Read,
	B: Read,
{
	#[inline]
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
		match self {
			&mut EitherSocket::First(ref mut a) => a.read(buf),
			&mut EitherSocket::Second(ref mut b) => b.read(buf),
		}
	}
}

impl<A, B> AsyncWrite for EitherSocket<A, B>
where
	A: AsyncWrite,
	B: AsyncWrite,
{
	#[inline]
	fn shutdown(&mut self) -> Poll<(), IoError> {
		match self {
			&mut EitherSocket::First(ref mut a) => a.shutdown(),
			&mut EitherSocket::Second(ref mut b) => b.shutdown(),
		}
	}
}

impl<A, B> Write for EitherSocket<A, B>
where
	A: Write,
	B: Write,
{
	#[inline]
	fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
		match self {
			&mut EitherSocket::First(ref mut a) => a.write(buf),
			&mut EitherSocket::Second(ref mut b) => b.write(buf),
		}
	}

	#[inline]
	fn flush(&mut self) -> Result<(), IoError> {
		match self {
			&mut EitherSocket::First(ref mut a) => a.flush(),
			&mut EitherSocket::Second(ref mut b) => b.flush(),
		}
	}
}

impl<A, B> StreamMuxer for EitherSocket<A, B>
where
	A: StreamMuxer,
	B: StreamMuxer,
{
	type Substream = EitherSocket<A::Substream, B::Substream>;
	type InboundSubstream = EitherTransportFuture<A::InboundSubstream, B::InboundSubstream>;
	type OutboundSubstream = EitherTransportFuture<A::OutboundSubstream, B::OutboundSubstream>;

	#[inline]
	fn inbound(self) -> Self::InboundSubstream {
		match self {
			EitherSocket::First(a) => EitherTransportFuture::First(a.inbound()),
			EitherSocket::Second(b) => EitherTransportFuture::Second(b.inbound()),
		}
	}

	#[inline]
	fn outbound(self) -> Self::OutboundSubstream {
		match self {
			EitherSocket::First(a) => EitherTransportFuture::First(a.outbound()),
			EitherSocket::Second(b) => EitherTransportFuture::Second(b.outbound()),
		}
	}
}

/// Implemented on structs that describe a possible upgrade to a connection between two peers.
///
/// The generic `C` is the type of the incoming connection before it is upgraded.
///
/// > **Note**: The `upgrade` method of this trait uses `self` and not `&self` or `&mut self`.
/// >           This has been designed so that you would implement this trait on `&Foo` or
/// >           `&mut Foo` instead of directly on `Foo`.
pub trait ConnectionUpgrade<C: AsyncRead + AsyncWrite, Conf> {
	/// Iterator returned by `protocol_names`.
	type NamesIter: Iterator<Item = (Bytes, Self::UpgradeIdentifier)>;
	/// Type that serves as an identifier for the protocol. This type only exists to be returned
	/// by the `NamesIter` and then be passed to `upgrade`.
	///
	/// This is only useful on implementations that dispatch between multiple possible upgrades.
	/// Any basic implementation will probably just use the `()` type.
	type UpgradeIdentifier;

	/// Returns the name of the protocols to advertise to the remote.
	fn protocol_names(&self) -> Self::NamesIter;

	/// Type of the stream that has been upgraded. Generally wraps around `C` and `Self`.
	///
	/// > **Note**: For upgrades that add an intermediary layer (such as `secio` or `multiplex`),
	/// >           this associated type must implement `AsyncRead + AsyncWrite`.
	type Output;
	/// Type of the future that will resolve to `Self::Output`.
	type Future: Future<Item = Self::Output, Error = IoError>;

	/// This method is called after protocol negotiation has been performed.
	///
	/// Because performing the upgrade may not be instantaneous (eg. it may require a handshake),
	/// this function returns a future instead of the direct output.
	fn upgrade(self, socket: C, id: Self::UpgradeIdentifier, ty: Endpoint,
			   remote_addr: &Multiaddr, conf: Conf) -> Self::Future;
}

/// Type of connection for the upgrade.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Endpoint {
	/// The socket comes from a dialer.
	Dialer,
	/// The socket comes from a listener.
	Listener,
}

/// Implementation of `ConnectionUpgrade` that always fails to negotiate.
#[derive(Debug, Copy, Clone)]
pub struct DeniedConnectionUpgrade;

impl<C, Conf> ConnectionUpgrade<C, Conf> for DeniedConnectionUpgrade
	where C: AsyncRead + AsyncWrite
{
	type NamesIter = iter::Empty<(Bytes, ())>;
	type UpgradeIdentifier = ();		// TODO: could use `!`
	type Output = ();		// TODO: could use `!`
	type Future = Box<Future<Item = (), Error = IoError>>;		// TODO: could use `!`

	#[inline]
	fn protocol_names(&self) -> Self::NamesIter {
		iter::empty()
	}

	#[inline]
	fn upgrade(self, _: C, _: Self::UpgradeIdentifier, _: Endpoint, _: &Multiaddr, _: Conf) -> Self::Future {
		unreachable!("the denied connection upgrade always fails to negotiate")
	}
}

/// Extension trait for `ConnectionUpgrade`. Automatically implemented on everything.
pub trait UpgradeExt {
	/// Builds a struct that will choose an upgrade between `self` and `other`, depending on what
	/// the remote supports.
    fn or_upgrade<T>(self, other: T) -> OrUpgrade<Self, T>
		where Self: Sized;
}

impl<T> UpgradeExt for T {
	#[inline]
    fn or_upgrade<U>(self, other: U) -> OrUpgrade<Self, U> {
        OrUpgrade(self, other)
    }
}

/// See `or_upgrade()`.
#[derive(Debug, Copy, Clone)]
pub struct OrUpgrade<A, B>(A, B);

impl<C, A, B, Conf> ConnectionUpgrade<C, Conf> for OrUpgrade<A, B>
where
	C: AsyncRead + AsyncWrite,
	A: ConnectionUpgrade<C, Conf>,
	B: ConnectionUpgrade<C, Conf>,
{
	type NamesIter = NamesIterChain<A::NamesIter, B::NamesIter>;
	type UpgradeIdentifier = EitherUpgradeIdentifier<A::UpgradeIdentifier, B::UpgradeIdentifier>;

	#[inline]
	fn protocol_names(&self) -> Self::NamesIter {
		NamesIterChain {
			first: self.0.protocol_names(),
			second: self.1.protocol_names(),
		}
	}

	type Output = EitherSocket<A::Output, B::Output>;
	type Future = EitherConnUpgrFuture<A::Future, B::Future>;

	#[inline]
	fn upgrade(self, socket: C, id: Self::UpgradeIdentifier, ty: Endpoint,
			   remote_addr: &Multiaddr, conf: Conf) -> Self::Future
	{
		match id {
			EitherUpgradeIdentifier::First(id) => {
				EitherConnUpgrFuture::First(self.0.upgrade(socket, id, ty, remote_addr, conf))
			}
			EitherUpgradeIdentifier::Second(id) => {
				EitherConnUpgrFuture::Second(self.1.upgrade(socket, id, ty, remote_addr, conf))
			}
		}
	}
}

/// Internal struct used by the `OrUpgrade` trait.
#[derive(Debug, Copy, Clone)]
pub enum EitherUpgradeIdentifier<A, B> {
	First(A),
	Second(B),
}

/// Implements `Future` and redirects calls to either `First` or `Second`.
///
/// Additionally, the output will be wrapped inside a `EitherSocket`.
///
// TODO: This type is needed because of the lack of `impl Trait` in stable Rust.
//		 If Rust had impl Trait we could use the Either enum from the futures crate and add some
//		 modifiers to it. This custom enum is a combination of Either and these modifiers.
#[derive(Debug, Copy, Clone)]
pub enum EitherConnUpgrFuture<A, B> {
	First(A),
	Second(B),
}

impl<A, B> Future for EitherConnUpgrFuture<A, B>
where
	A: Future<Error = IoError>,
	B: Future<Error = IoError>,
{
	type Item = EitherSocket<A::Item, B::Item>;
	type Error = IoError;

	#[inline]
	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		match self {
			&mut EitherConnUpgrFuture::First(ref mut a) => {
				let item = try_ready!(a.poll());
				Ok(Async::Ready(EitherSocket::First(item)))
			}
			&mut EitherConnUpgrFuture::Second(ref mut b) => {
				let item = try_ready!(b.poll());
				Ok(Async::Ready(EitherSocket::Second(item)))
			}
		}
	}
}

/// Internal type used by the `OrUpgrade` struct.
///
/// > **Note**: This type is needed because of the lack of `-> impl Trait` in Rust. It can be
/// >           removed eventually.
#[derive(Debug, Copy, Clone)]
pub struct NamesIterChain<A, B> {
	first: A,
	second: B,
}

impl<A, B, AId, BId> Iterator for NamesIterChain<A, B>
where
	A: Iterator<Item = (Bytes, AId)>,
	B: Iterator<Item = (Bytes, BId)>,
{
	type Item = (Bytes, EitherUpgradeIdentifier<AId, BId>);

	#[inline]
	fn next(&mut self) -> Option<Self::Item> {
		if let Some((name, id)) = self.first.next() {
			return Some((name, EitherUpgradeIdentifier::First(id)));
		}
		if let Some((name, id)) = self.second.next() {
			return Some((name, EitherUpgradeIdentifier::Second(id)));
		}
		None
	}

	#[inline]
	fn size_hint(&self) -> (usize, Option<usize>) {
		let (min1, max1) = self.first.size_hint();
		let (min2, max2) = self.second.size_hint();
		let max = match (max1, max2) {
			(Some(max1), Some(max2)) => max1.checked_add(max2),
			_ => None,
		};
		(min1.saturating_add(min2), max)
	}
}

/// Implementation of the `ConnectionUpgrade` that negotiates the `/plaintext/1.0.0` protocol and
/// simply passes communications through without doing anything more.
///
/// > **Note**: Generally used as an alternative to `secio` if a security layer is not desirable.
// TODO: move `PlainTextConfig` to a separate crate?
#[derive(Debug, Copy, Clone)]
pub struct PlainTextConfig;

impl<C, Conf> ConnectionUpgrade<C, Conf> for PlainTextConfig
where
	C: AsyncRead + AsyncWrite,
{
	type Output = C;
	type Future = FutureResult<C, IoError>;
	type UpgradeIdentifier = ();
	type NamesIter = iter::Once<(Bytes, ())>;

	#[inline]
	fn upgrade(self, i: C, _: (), _: Endpoint, _: &Multiaddr, _: Conf) -> Self::Future {
		future::ok(i)
	}

	#[inline]
	fn protocol_names(&self) -> Self::NamesIter {
		iter::once((Bytes::from("/plaintext/1.0.0"), ()))
	}
}

/// Dummy implementation of `MuxedTransport` that uses an inner `Transport`.
#[derive(Debug, Copy, Clone)]
pub struct DummyMuxing<T> {
	inner: T,
}

impl<T, Conf> MuxedTransport<Conf> for DummyMuxing<T>
	where T: Transport<Conf>
{
	type Incoming = future::Empty<(T::RawConn, Multiaddr), IoError>;

	fn next_incoming(self, _: Conf) -> Self::Incoming
		where Self: Sized
	{
		future::empty()
	}
}

impl<T, Conf> Transport<Conf> for DummyMuxing<T>
	where T: Transport<Conf>
{
	type RawConn = T::RawConn;
	type Listener = T::Listener;
	type ListenerUpgrade = T::ListenerUpgrade;
	type Dial = T::Dial;

	#[inline]
	fn listen_on(self, addr: Multiaddr, conf: Conf) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)>
	where
		Self: Sized
	{
		self.inner.listen_on(addr, conf).map_err(|(inner, addr)| {
			(DummyMuxing { inner }, addr)
		})
	}

	#[inline]
	fn dial(self, addr: Multiaddr, conf: Conf) -> Result<Self::Dial, (Self, Multiaddr)>
	where
		Self: Sized
	{
		self.inner.dial(addr, conf).map_err(|(inner, addr)| {
			(DummyMuxing { inner }, addr)
		})
	}

	#[inline]
	fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
		self.inner.nat_traversal(server, observed)
	}
}

/// Implements the `Transport` trait. Dials or listens, then upgrades any dialed or received
/// connection.
///
/// See the `Transport::with_upgrade` method.
#[derive(Debug)]
pub struct UpgradedNode<T, C, Conf> {
	transports: T,
	upgrade: C,
	config: ::std::marker::PhantomData<Conf>,
}

impl<T: Clone, C: Clone, Conf> Clone for UpgradedNode<T, C, Conf> {
	fn clone(&self) -> Self {
		UpgradedNode {
			transports: self.transports.clone(),
			upgrade: self.upgrade.clone(),
			config: self.config,
		}
	}
}

impl<'a, Trans: 'a, Upgrade: 'a, Conf: 'a> UpgradedNode<Trans, Upgrade, Conf>
where
	Trans: Transport<Conf>,
	Upgrade: ConnectionUpgrade<Trans::RawConn, Conf>,
	Conf: Clone,
{
	/// Turns this upgraded node into a `ConnectionReuse`. If the `Output` implements the
	/// `StreamMuxer` trait, the returned object will implement `Transport` and `MuxedTransport`.
	#[inline]
	pub fn into_connection_reuse(self) -> ConnectionReuse<Trans, Upgrade, Conf> {
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
		conf: Conf,
	) -> Result<Box<Future<Item = (C::Output, Multiaddr), Error = IoError> + 'a>, (Self, Multiaddr)> {
		let upgrade = self.upgrade;

		let dialed_fut = match self.transports.dial(addr.clone(), conf.clone()) {
			Ok(f) => f.into_future(),
			Err((trans, addr)) => {
				let builder = UpgradedNode {
					transports: trans,
					upgrade: upgrade,
					config: Default::default(),
				};

				return Err((builder, addr));
			}
		};

		let future = dialed_fut
            // Try to negotiate the protocol.
            .and_then(move |(connection, client_addr)| {
                let iter = upgrade.protocol_names()
                    .map(|(name, id)| (name, <Bytes as PartialEq>::eq, id));
                let negotiated = multistream_select::dialer_select_proto(connection, iter)
                    .map_err(|err| IoError::new(IoErrorKind::Other, err));
                negotiated.map(|(upgrade_id, conn)| (upgrade_id, conn, upgrade, client_addr))
            })
            .and_then(move |(upgrade_id, connection, upgrade, client_addr)| {
                let f = upgrade.upgrade(connection, upgrade_id, Endpoint::Dialer, &client_addr, conf);
                f.map(|v| (v, client_addr))
            });

		Ok(Box::new(future))
	}

	/// If the underlying transport is a `MuxedTransport`, then after calling `dial` we may receive
	/// substreams opened by the dialed nodes.
	/// 
	/// This function returns the next incoming substream. You are strongly encouraged to call it
	/// if you have a muxed transport.
	pub fn next_incoming(self, conf: Conf) -> Box<Future<Item = (Upgrade::Output, Multiaddr), Error = IoError> + 'a>
		where Trans: MuxedTransport<Conf>,
			  Upgrade::NamesIter: Clone, // TODO: not elegant
			  Upgrade: Clone,
	{
		let upgrade = self.upgrade;

		let future = self.transports.next_incoming(conf.clone())
            // Try to negotiate the protocol.
            .and_then(move |(connection, addr)| {
                let iter = upgrade.protocol_names()
                    .map::<_, fn(_) -> _>(|(name, id)| (name, <Bytes as PartialEq>::eq, id));
                let negotiated = multistream_select::listener_select_proto(connection, iter)
                    .map_err(|err| IoError::new(IoErrorKind::Other, err));
                negotiated.map(|(upgrade_id, conn)| (upgrade_id, conn, upgrade, addr))
            })
            .and_then(|(upgrade_id, connection, upgrade, addr)| {
                upgrade.upgrade(connection, upgrade_id, Endpoint::Dialer, &addr, conf)
					.map(|u| (u, addr))
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
		conf: Conf,
	) -> Result<
		(Box<Stream<Item = Box<Future<Item = (Upgrade::Output, Multiaddr), Error = IoError> + 'a>, Error = IoError> + 'a>, Multiaddr),
		(Self, Multiaddr),
	>
	where
		Upgrade::NamesIter: Clone, // TODO: not elegant
		Upgrade: Clone,
	{
		let upgrade = self.upgrade;

		let (listening_stream, new_addr) = match self.transports.listen_on(addr, conf.clone()) {
			Ok((l, new_addr)) => (l, new_addr),
			Err((trans, addr)) => {
				let builder = UpgradedNode {
					transports: trans,
					upgrade: upgrade,
					config: Default::default(),
				};

				return Err((builder, addr));
			}
		};

		// Try to negotiate the protocol.
		// Note that failing to negotiate a protocol will never produce a future with an error.
		// Instead the `stream` will produce `Ok(Err(...))`.
		// `stream` can only produce an `Err` if `listening_stream` produces an `Err`.
		let stream = listening_stream
			.zip(stream::repeat(conf))
			.map(move |((connection, client_addr), conf)| {
				let upgrade = upgrade.clone();
				let connection = connection
					// Try to negotiate the protocol
					.and_then(move |(connection, remote_addr)| {
						let iter = upgrade.protocol_names()
							.map::<_, fn(_) -> _>(|(n, t)| (n, <Bytes as PartialEq>::eq, t));
						multistream_select::listener_select_proto(connection, iter)
							.map_err(|err| IoError::new(IoErrorKind::Other, err))
							.and_then(move |(upgrade_id, connection)| {
								let fut = upgrade.upgrade(connection, upgrade_id, Endpoint::Listener,
									&remote_addr, conf);
								fut.map(move |c| (c, remote_addr))
							})
							.into_future()
					});

				Box::new(connection) as Box<_>
			});

		Ok((Box::new(stream), new_addr))
	}
}

impl<Trans: 'static, Upgrade: 'static, Conf: 'static> Transport<Conf> for UpgradedNode<Trans, Upgrade, Conf>
where
	Conf: Clone,
	Trans: Transport<Conf>,
	Upgrade: ConnectionUpgrade<Trans::RawConn, Conf>,
	Upgrade::Output: AsyncRead + AsyncWrite,
	Upgrade::NamesIter: Clone, // TODO: not elegant
	Upgrade: Clone,
{
	type RawConn = Upgrade::Output;
	type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = IoError>>;
	type ListenerUpgrade = Box<Future<Item = Upgrade::Output, Error = IoError>>;
	type Dial = Box<Future<Item = Upgrade::Output, Error = IoError>>;

	#[inline]
	fn listen_on(self, addr: Multiaddr, conf: Conf) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
		UpgradedNode::listen_on(self, addr, conf)
	}

	#[inline]
	fn dial(self, addr: Multiaddr, conf: Conf) -> Result<Self::Dial, (Self, Multiaddr)> {
		UpgradedNode::dial(self, addr, conf)
	}

	#[inline]
	fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
		self.transports.nat_traversal(server, observed)
	}
}

impl<Trans: 'static, Upgrade: 'static, Conf: 'static> MuxedTransport<Conf> for UpgradedNode<Trans, Upgrade, Conf>
where
	Conf: Clone,
	Trans: MuxedTransport<Conf>,
	Upgrade: ConnectionUpgrade<Trans::RawConn, Conf>,
	Upgrade::Output: AsyncRead + AsyncWrite,
	Upgrade::NamesIter: Clone, // TODO: not elegant
	Upgrade: Clone,
{
	type Incoming = Box<Future<Item = (Upgrade::Output, Multiaddr), Error = IoError>>;

	#[inline]
	fn next_incoming(self, conf: Conf) -> Self::Incoming {
		self.next_incoming(conf)
	}
}
