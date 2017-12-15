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

//! Contains the `ConnectionReuse` struct. Stores open muxed connections to nodes so that dialing
//! a node reuses the same connection instead of opening a new one.
//!
//! A `ConnectionReuse` can only be created from an `UpgradedNode` whose `ConnectionUpgrade`
//! yields as `StreamMuxer`.
//!
//! # Behaviour
//!
//! The API exposed by the `ConnectionReuse` struct consists in the `Transport` trait
//! implementation, with the `dial` and `listen_on` methods.
//!
//! When called on a `ConnectionReuse`, the `listen_on` method will listen on the given
//! multiaddress (by using the underlying `Transport`), then will apply a `flat_map` on the
//! incoming connections so that we actually listen to the incoming substreams of each connection.
//! TODO: design issue ; we need a way to handle the substreams that are opened by remotes on
//!       connections opened by us
//!
//! When called on a `ConnectionReuse`, the `dial` method will try to use a connection that has
//! already been opened earlier, and open an outgoing substream on it. If none is available, it
//! will dial the given multiaddress.
//! TODO: this raises several questions ^
//!
//! TODO: this whole code is a dummy and should be rewritten after the design has been properly
//!       figured out.

use futures::{Async, Future, Poll, Stream};
use futures::stream::Fuse as StreamFuse;
use futures::stream;
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use smallvec::SmallVec;
use std::io::Error as IoError;
use transport::{ConnectionUpgrade, MuxedTransport, Transport, UpgradedNode};

/// Allows reusing the same muxed connection multiple times.
///
/// Can be created from an `UpgradedNode` through the `From` trait.
///
/// Implements the `Transport` trait.
#[derive(Clone)]
pub struct ConnectionReuse<T, C>
where
	T: Transport,
	C: ConnectionUpgrade<T::RawConn>,
{
	// Underlying transport and connection upgrade for when we need to dial or listen.
	inner: UpgradedNode<T, C>,
}

impl<T, C> From<UpgradedNode<T, C>> for ConnectionReuse<T, C>
where
	T: Transport,
	C: ConnectionUpgrade<T::RawConn>,
{
	#[inline]
	fn from(node: UpgradedNode<T, C>) -> ConnectionReuse<T, C> {
		ConnectionReuse { inner: node }
	}
}

impl<T, C> Transport for ConnectionReuse<T, C>
where
	T: Transport + 'static,                     // TODO: 'static :(
	C: ConnectionUpgrade<T::RawConn> + 'static, // TODO: 'static :(
	C: Clone,
	C::Output: StreamMuxer + Clone,
	C::NamesIter: Clone, // TODO: not elegant
{
	type RawConn = <C::Output as StreamMuxer>::Substream;
	type Listener = ConnectionReuseListener<
		Box<Stream<Item = (C::Output, Multiaddr), Error = IoError>>,
		C::Output,
	>;
	type Dial = Box<Future<Item = Self::RawConn, Error = IoError>>;

	fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
		let (listener, new_addr) = match self.inner.listen_on(addr.clone()) {
			Ok((l, a)) => (l, a),
			Err((inner, addr)) => {
				return Err((ConnectionReuse { inner: inner }, addr));
			}
		};

		let listener = ConnectionReuseListener {
			listener: listener.fuse(),
			connections: Vec::new(),
		};

		Ok((listener, new_addr))
	}

	fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
		let dial = match self.inner.dial(addr) {
			Ok(l) => l,
			Err((inner, addr)) => {
				return Err((ConnectionReuse { inner: inner }, addr));
			}
		};

		let future = dial.and_then(|dial| dial.outbound());
		Ok(Box::new(future) as Box<_>)
	}
}

impl<T, C> MuxedTransport for ConnectionReuse<T, C>
where
	T: Transport + 'static,                     // TODO: 'static :(
	C: ConnectionUpgrade<T::RawConn> + 'static, // TODO: 'static :(
	C: Clone,
	C::Output: StreamMuxer + Clone,
	C::NamesIter: Clone, // TODO: not elegant
{
	type Incoming = stream::AndThen<
		stream::Repeat<C::Output, IoError>,
		fn(C::Output)
			-> <<C as ConnectionUpgrade<T::RawConn>>::Output as StreamMuxer>::InboundSubstream,
		<<C as ConnectionUpgrade<T::RawConn>>::Output as StreamMuxer>::InboundSubstream,
	>;
	type Outgoing =
		<<C as ConnectionUpgrade<T::RawConn>>::Output as StreamMuxer>::OutboundSubstream;
	type DialAndListen = Box<Future<Item = (Self::Incoming, Self::Outgoing), Error = IoError>>;

	fn dial_and_listen(self, addr: Multiaddr) -> Result<Self::DialAndListen, (Self, Multiaddr)> {
		self.inner
			.dial(addr)
			.map_err(|(inner, addr)| (ConnectionReuse { inner: inner }, addr))
			.map(|fut| {
				fut.map(|muxer| {
					(
						stream::repeat(muxer.clone()).and_then(StreamMuxer::inbound as fn(_) -> _),
						muxer.outbound(),
					)
				})
			})
			.map(|fut| Box::new(fut) as _)
	}
}

/// Implementation of `Stream<Item = (impl AsyncRead + AsyncWrite, Multiaddr)` for the
/// `ConnectionReuse` struct.
pub struct ConnectionReuseListener<S, M>
where
	S: Stream<Item = (M, Multiaddr), Error = IoError>,
	M: StreamMuxer,
{
	listener: StreamFuse<S>,
	connections: Vec<(M, <M as StreamMuxer>::InboundSubstream, Multiaddr)>,
}

impl<S, M> Stream for ConnectionReuseListener<S, M>
where
	S: Stream<Item = (M, Multiaddr), Error = IoError>,
	M: StreamMuxer + Clone + 'static, // TODO: 'static :(
{
	type Item = (M::Substream, Multiaddr);
	type Error = IoError;

	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		match self.listener.poll() {
			Ok(Async::Ready(Some((upgrade, client_addr)))) => {
				let next_incoming = upgrade.clone().inbound();
				self.connections.push((upgrade, next_incoming, client_addr));
			}
			Ok(Async::NotReady) => (),
			Ok(Async::Ready(None)) => {
				if self.connections.is_empty() {
					return Ok(Async::Ready(None));
				}
			}
			Err(err) => {
				if self.connections.is_empty() {
					return Err(err);
				}
			}
		};

		// Most of the time, this array will contain 0 or 1 elements, but sometimes it may contain
		// more and we don't want to panic if that happens. With 8 elements, we can be pretty
		// confident that this is never going to spill into a `Vec`.
		let mut connections_to_drop: SmallVec<[_; 8]> = SmallVec::new();

		for (index, &mut (ref mut muxer, ref mut next_incoming, ref client_addr)) in
			self.connections.iter_mut().enumerate()
		{
			match next_incoming.poll() {
				Ok(Async::Ready(incoming)) => {
					let mut new_next = muxer.clone().inbound();
					*next_incoming = new_next;
					return Ok(Async::Ready(Some((incoming, client_addr.clone()))));
				}
				Ok(Async::NotReady) => {}
				Err(_) => {
					connections_to_drop.push(index);
				}
			}
		}

		for &index in connections_to_drop.iter().rev() {
			self.connections.swap_remove(index);
		}

		Ok(Async::NotReady)
	}
}
