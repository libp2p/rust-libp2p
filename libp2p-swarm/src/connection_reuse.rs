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
//! will dial the given multiaddress. Dialed node can also spontaneously open new substreams with
//! us. In order to handle these new substreams you should use the `next_incoming` method of the
//! `MuxedTransport` trait.
//! TODO: this raises several questions ^
//!
//! TODO: this whole code is a dummy and should be rewritten after the design has been properly
//!       figured out.

use futures::future::{self, IntoFuture, FutureResult};
use futures::{stream, Async, Future, Poll, Stream, task};
use futures::stream::Fuse as StreamFuse;
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use parking_lot::Mutex;
use smallvec::SmallVec;
use std::io::Error as IoError;
use std::sync::Arc;
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
	shared: Arc<Mutex<Shared<C::Output>>>,
}

struct Shared<O> {
	// List of futures to dialed connections.
	incoming: Vec<Box<Stream<Item = (O, Multiaddr), Error = future::SharedError<Mutex<Option<IoError>>>>>>,
	// Tasks to signal when an element is added to `incoming`. Only used when `incoming` is empty.
	to_signal: Vec<task::Task>,
}

impl<T, C> From<UpgradedNode<T, C>> for ConnectionReuse<T, C>
where
	T: Transport,
	C: ConnectionUpgrade<T::RawConn>,
{
	#[inline]
	fn from(node: UpgradedNode<T, C>) -> ConnectionReuse<T, C> {
		ConnectionReuse {
			inner: node,
			shared: Arc::new(Mutex::new(Shared {
				incoming: Vec::new(),
				to_signal: Vec::new(),
			})),
		}
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
	type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = IoError>>;
	type ListenerUpgrade = FutureResult<Self::RawConn, IoError>;
	type Dial = Box<Future<Item = Self::RawConn, Error = IoError>>;

	fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
		let (listener, new_addr) = match self.inner.listen_on(addr.clone()) {
			Ok((l, a)) => (l, a),
			Err((inner, addr)) => {
				return Err((ConnectionReuse { inner: inner, shared: self.shared }, addr));
			}
		};

		let listener = ConnectionReuseListener {
			listener: listener.fuse(),
			current_upgrades: Vec::new(),
			connections: Vec::new(),
		};

		Ok((Box::new(listener) as Box<_>, new_addr))
	}

	fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
		let dial = match self.inner.dial(addr.clone()) {
			Ok(l) => l,
			Err((inner, addr)) => {
				return Err((ConnectionReuse { inner: inner, shared: self.shared }, addr));
			}
		};

		let dial = dial
			.map_err::<fn(IoError) -> Mutex<Option<IoError>>, _>(|err| Mutex::new(Some(err)))
			.shared();

		let ingoing = dial.clone()
			.map(|muxer| stream::repeat(muxer))
			.flatten_stream()
			.map(move |muxer| ((&*muxer).clone(), addr.clone()));

		let mut lock = self.shared.lock();
		lock.incoming.push(Box::new(ingoing) as Box<_>);
		for task in lock.to_signal.drain(..) { task.notify(); }
		drop(lock);

		let future = dial
			.map_err(|err| err.lock().take().expect("error can only be extracted once"))
			.and_then(|dial| (&*dial).clone().outbound());
		Ok(Box::new(future) as Box<_>)
	}

	#[inline]
	fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
		self.inner.transport().nat_traversal(server, observed)
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
	type Incoming = Box<Future<Item = (<C::Output as StreamMuxer>::Substream, Multiaddr), Error = IoError>>;

	#[inline]
	fn next_incoming(self) -> Self::Incoming {
		let future = ConnectionReuseIncoming { shared: self.shared.clone() }
			.and_then(|(out, addr)| {
				out.inbound().map(|o| (o, addr))
			});
		Box::new(future) as Box<_>
	}
}

/// Implementation of `Stream<Item = (impl AsyncRead + AsyncWrite, Multiaddr)` for the
/// `ConnectionReuse` struct.
pub struct ConnectionReuseListener<S, F, M>
where
	S: Stream<Item = (F, Multiaddr), Error = IoError>,
	F: Future<Item = M, Error = IoError>,
	M: StreamMuxer,
{
	listener: StreamFuse<S>,
	current_upgrades: Vec<(F, Multiaddr)>,
	connections: Vec<(M, <M as StreamMuxer>::InboundSubstream, Multiaddr)>,
}

impl<S, F, M> Stream for ConnectionReuseListener<S, F, M>
where S: Stream<Item = (F, Multiaddr), Error = IoError>,
	  F: Future<Item = M, Error = IoError>,
	  M: StreamMuxer + Clone + 'static // TODO: 'static :(
{
	type Item = (FutureResult<M::Substream, IoError>, Multiaddr);
	type Error = IoError;

	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		match self.listener.poll() {
			Ok(Async::Ready(Some((upgrade, client_addr)))) => {
				self.current_upgrades.push((upgrade, client_addr));
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
		let mut upgrades_to_drop: SmallVec<[_; 8]> = SmallVec::new();
		let mut early_ret = None;

		for (index, &mut (ref mut current_upgrade, ref mut client_addr)) in
			self.current_upgrades.iter_mut().enumerate()
		{
			match current_upgrade.poll() {
				Ok(Async::Ready(muxer)) => {
					let next_incoming = muxer.clone().inbound();
					self.connections.push((muxer, next_incoming, client_addr.clone()));
					upgrades_to_drop.push(index);
				},
				Ok(Async::NotReady) => {},
				Err(err) => {
					upgrades_to_drop.push(index);
					early_ret = Some(Async::Ready(Some((Err(err).into_future(), client_addr.clone()))));
				},
			}
		}

		for &index in upgrades_to_drop.iter().rev() {
			self.current_upgrades.swap_remove(index);
		}

		if let Some(early_ret) = early_ret {
			return Ok(early_ret);
		}

		// We reuse `upgrades_to_drop`.
		upgrades_to_drop.clear();
		let mut connections_to_drop = upgrades_to_drop;

		for (index, &mut (ref mut muxer, ref mut next_incoming, ref client_addr)) in
			self.connections.iter_mut().enumerate()
		{
			match next_incoming.poll() {
				Ok(Async::Ready(incoming)) => {
					let mut new_next = muxer.clone().inbound();
					*next_incoming = new_next;
					return Ok(Async::Ready(Some((Ok(incoming).into_future(), client_addr.clone()))));
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

/// Implementation of `Future<Item = (impl AsyncRead + AsyncWrite, Multiaddr)` for the
/// `ConnectionReuse` struct.
pub struct ConnectionReuseIncoming<O> {
	shared: Arc<Mutex<Shared<O>>>,
}

impl<O> Future for ConnectionReuseIncoming<O>
	where O: Clone
{
	type Item = (O, Multiaddr);
	type Error = IoError;

	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		let mut lock = self.shared.lock();

		let mut to_remove = SmallVec::<[_; 8]>::new();
		let mut ret_value = None;

		for (offset, future) in lock.incoming.iter_mut().enumerate() {
			match future.poll() {
				Ok(Async::Ready(Some((value, addr)))) => {
					ret_value = Some((value.clone(), addr));
					break;
				},
				Ok(Async::Ready(None)) => {
					to_remove.push(offset);
				},
				Ok(Async::NotReady) => {},
				Err(_) => {
					to_remove.push(offset);
				},
			}
		}

		for offset in to_remove.into_iter().rev() {
			lock.incoming.swap_remove(offset);
		}

		if let Some(ret_value) = ret_value {
			Ok(Async::Ready(ret_value))
		} else {
			lock.to_signal.push(task::current());
			Ok(Async::NotReady)
		}
	}
}
