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
//!
//! When called on a `ConnectionReuse`, the `dial` method will try to use a connection that has
//! already been opened earlier, and open an outgoing substream on it. If none is available, it
//! will dial the given multiaddress. Dialed node can also spontaneously open new substreams with
//! us. In order to handle these new substreams you should use the `next_incoming` method of the
//! `MuxedTransport` trait.

use fnv::FnvHashMap;
use futures::future::{self, IntoFuture, FutureResult};
use futures::{Async, Future, Poll, Stream};
use futures::stream::Fuse as StreamFuse;
use futures::sync::mpsc;
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use parking_lot::Mutex;
use std::io::Error as IoError;
use std::mem;
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
	C::Output: StreamMuxer,
{
	// Underlying transport and connection upgrade for when we need to dial or listen.
	inner: UpgradedNode<T, C>,

	// Struct shared between most of the types that are part of the `ConnectionReuse`
	// infrastructure.
	shared: Arc<Mutex<Shared<C::Output>>>,
}

struct Shared<M> where M: StreamMuxer {
	// List of active muxers.
	active_connections: FnvHashMap<Multiaddr, M>,

	// List of pending inbound substreams from dialed nodes.
	// Only add to this list elements received through `add_to_next_rx`.
	next_incoming: Vec<(M, M::InboundSubstream, Multiaddr)>,

	// New elements are not directly added to `next_incoming`. Instead they are sent to this
	// channel. This is done so that we can wake up tasks whenever a next inbound stream arrives.
	add_to_next_rx: mpsc::UnboundedReceiver<(M, M::InboundSubstream, Multiaddr)>,

	// Other part of `add_to_next_rx`.
	add_to_next_tx: mpsc::UnboundedSender<(M, M::InboundSubstream, Multiaddr)>,
}

impl<T, C> From<UpgradedNode<T, C>> for ConnectionReuse<T, C>
where
	T: Transport,
	C: ConnectionUpgrade<T::RawConn>,
	C::Output: StreamMuxer,
{
	#[inline]
	fn from(node: UpgradedNode<T, C>) -> ConnectionReuse<T, C> {
		let (tx, rx) = mpsc::unbounded();

		ConnectionReuse {
			inner: node,
			shared: Arc::new(Mutex::new(Shared {
				active_connections: Default::default(),
				next_incoming: Vec::new(),
				add_to_next_rx: rx,
				add_to_next_tx: tx,
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
			shared: self.shared.clone(),
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

		// If we already have an active connection, use it!
		if let Some(connec) = self.shared.lock().active_connections.get(&addr).map(|c| c.clone()) {
			let future = connec.outbound();
			return Ok(Box::new(future) as Box<_>);
		}

		// TODO: handle if we're already in the middle in dialing that same node?

		let shared = self.shared.clone();
		let dial = dial
			.into_future()
			.and_then(move |connec| {
				// Always replace the active connection because we are the most recent.
				let mut lock = shared.lock();
				lock.active_connections.insert(addr.clone(), connec.clone());
				// TODO: doesn't need locking ; the sender could be extracted
				let _ = lock.add_to_next_tx
					.unbounded_send((connec.clone(), connec.clone().inbound(), addr));
				connec.outbound()
			});

		Ok(Box::new(dial) as Box<_>)
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
	type Incoming = ConnectionReuseIncoming<C::Output>;
	type IncomingUpgrade = future::FutureResult<<C::Output as StreamMuxer>::Substream, IoError>;

	#[inline]
	fn next_incoming(self) -> Self::Incoming {
		ConnectionReuseIncoming { shared: self.shared.clone() }
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
	shared: Arc<Mutex<Shared<M>>>,
}

impl<S, F, M> Stream for ConnectionReuseListener<S, F, M>
where S: Stream<Item = (F, Multiaddr), Error = IoError>,
	  F: Future<Item = M, Error = IoError>,
	  M: StreamMuxer + Clone + 'static // TODO: 'static :(
{
	type Item = (FutureResult<M::Substream, IoError>, Multiaddr);
	type Error = IoError;

	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		// Check for any incoming connection on the socket.
		match self.listener.poll() {
			Ok(Async::Ready(Some((upgrade, client_addr)))) => {
				self.current_upgrades.push((upgrade, client_addr));
			}
			Ok(Async::NotReady) => {},
			Ok(Async::Ready(None)) => {
				if self.connections.is_empty() && self.current_upgrades.is_empty() {
					return Ok(Async::Ready(None));
				}
			}
			Err(err) => {
				if self.connections.is_empty() && self.current_upgrades.is_empty() {
					return Err(err);
				}
			}
		};

		// Check whether any upgrade (to a muxer) on an incoming connection is ready.
		// We extract everything at the start, then insert back the elements that we still want at
		// the next iteration.
        for n in (0 .. self.current_upgrades.len()).rev() {
            let (mut current_upgrade, client_addr) = self.current_upgrades.swap_remove(n);
			match current_upgrade.poll() {
				Ok(Async::Ready(muxer)) => {
					let next_incoming = muxer.clone().inbound();
					self.connections.push((muxer.clone(), next_incoming, client_addr.clone()));
					// We overwrite any current active connection to that multiaddr because we
					// are the freshest possible connection.
					self.shared.lock().active_connections.insert(client_addr, muxer);
				},
				Ok(Async::NotReady) => {
					self.current_upgrades.push((current_upgrade, client_addr));
				},
				Err(err) => {
					// Insert the rest of the pending upgrades, but not the current one.
					return Ok(Async::Ready(Some((future::err(err), client_addr))));
				},
			}	
		}

		// Check whether any incoming substream is ready.
		// We extract everything at the start, then insert back the elements that we still want at
		// the next iteration.
        for n in (0 .. self.connections.len()).rev() {
            let (muxer, mut next_incoming, client_addr) = self.connections.swap_remove(n);
			match next_incoming.poll() {
				Ok(Async::Ready(incoming)) => {
					let mut new_next = muxer.clone().inbound();
					self.connections.push((muxer, new_next, client_addr.clone()));
					return Ok(Async::Ready(Some((Ok(incoming).into_future(), client_addr))));
				}
				Ok(Async::NotReady) => {
					self.connections.push((muxer, next_incoming, client_addr));
				}
				Err(err) => {
					// Insert the rest of the pending connections, but not the current one.
					return Ok(Async::Ready(Some((future::err(err), client_addr))));
				}
			}
		}

		Ok(Async::NotReady)
	}
}

/// Implementation of `Future<Item = (impl AsyncRead + AsyncWrite, Multiaddr)` for the
/// `ConnectionReuse` struct.
pub struct ConnectionReuseIncoming<M>
	where M: StreamMuxer
{
	shared: Arc<Mutex<Shared<M>>>,
}

impl<M> Future for ConnectionReuseIncoming<M>
	where M: Clone + StreamMuxer,
{
	type Item = (future::FutureResult<M::Substream, IoError>, Multiaddr);
	type Error = IoError;

	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		let mut lock = self.shared.lock();

		loop {
			match lock.add_to_next_rx.poll() {
				Ok(Async::Ready(Some(elem))) => {
					lock.next_incoming.push(elem);
				},
				Ok(Async::NotReady) => break,
				Ok(Async::Ready(None)) | Err(_) => {
					// The sender and receiver are both in the same struct, therefore the link can
					// never break.
					unreachable!()
				},
			}
		}

		let mut next_incoming = mem::replace(&mut lock.next_incoming, Vec::new());
		while let Some((muxer, mut future, addr)) = next_incoming.pop() {
			match future.poll() {
				Ok(Async::Ready(value)) => {
					let next = muxer.clone().inbound();
					lock.next_incoming.push((muxer, next, addr.clone()));
					lock.next_incoming.extend(next_incoming);
					return Ok(Async::Ready((future::ok(value), addr)));
				},
				Ok(Async::NotReady) => {
					lock.next_incoming.push((muxer, future, addr));
				},
				Err(_) => {},
			}
		}

		Ok(Async::NotReady)
	}
}
