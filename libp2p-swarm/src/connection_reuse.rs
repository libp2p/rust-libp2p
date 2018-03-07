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
use futures::future::{self, FutureResult, IntoFuture};
use futures::{Async, Future, Poll, Stream};
use futures::stream::Fuse as StreamFuse;
use futures::sync::mpsc;
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use parking_lot::Mutex;
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
    C::Output: StreamMuxer,
{
    // Underlying transport and connection upgrade for when we need to dial or listen.
    inner: UpgradedNode<T, C>,

    // Struct shared between most of the `ConnectionReuse` infrastructure.
    shared: Arc<Mutex<Shared<C::Output>>>,
}

struct Shared<M>
where
    M: StreamMuxer,
{
    // List of active muxers.
    active_connections: FnvHashMap<Multiaddr, M>,

    // List of pending inbound substreams from dialed nodes.
    // Only add to this list elements received through `add_to_next_rx`.
    next_incoming: Vec<(M, M::InboundSubstream, Multiaddr)>,

    // New elements are not directly added to `next_incoming`. Instead they are sent to this
    // channel. This is done so that we can wake up tasks whenever a new element is added.
    add_to_next_rx: mpsc::UnboundedReceiver<(M, M::InboundSubstream, Multiaddr)>,

    // Other side of `add_to_next_rx`.
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
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = FutureResult<(Self::RawConn, Multiaddr), IoError>;
    type Dial = Box<Future<Item = (Self::RawConn, Multiaddr), Error = IoError>>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let (listener, new_addr) = match self.inner.listen_on(addr.clone()) {
            Ok((l, a)) => (l, a),
            Err((inner, addr)) => {
                return Err((
                    ConnectionReuse {
                        inner: inner,
                        shared: self.shared,
                    },
                    addr,
                ));
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
        // If we already have an active connection, use it!
        if let Some(connec) = self.shared
            .lock()
            .active_connections
            .get(&addr)
            .map(|c| c.clone())
        {
            let future = connec.outbound().map(|s| (s, addr));
            return Ok(Box::new(future) as Box<_>);
        }

        // TODO: handle if we're already in the middle in dialing that same node?
        // TODO: try dialing again if the existing connection has dropped

        let dial = match self.inner.dial(addr) {
            Ok(l) => l,
            Err((inner, addr)) => {
                return Err((
                    ConnectionReuse {
                        inner: inner,
                        shared: self.shared,
                    },
                    addr,
                ));
            }
        };

        let shared = self.shared.clone();
        let dial = dial.into_future().and_then(move |(connec, addr)| {
            // Always replace the active connection because we are the most recent.
            let mut lock = shared.lock();
            lock.active_connections.insert(addr.clone(), connec.clone());
            // TODO: doesn't need locking ; the sender could be extracted
            let _ = lock.add_to_next_tx.unbounded_send((
                connec.clone(),
                connec.clone().inbound(),
                addr.clone(),
            ));
            connec.outbound().map(|s| (s, addr))
        });

        Ok(Box::new(dial) as Box<_>)
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
    type Incoming = ConnectionReuseIncoming<C::Output>;
    type IncomingUpgrade =
        future::FutureResult<(<C::Output as StreamMuxer>::Substream, Multiaddr), IoError>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        ConnectionReuseIncoming {
            shared: self.shared.clone(),
        }
    }
}

/// Implementation of `Stream` for the connections incoming from listening on a specific address.
pub struct ConnectionReuseListener<S, F, M>
where
    S: Stream<Item = F, Error = IoError>,
    F: Future<Item = (M, Multiaddr), Error = IoError>,
    M: StreamMuxer,
{
    // The main listener. `S` is from the underlying transport.
    listener: StreamFuse<S>,
    current_upgrades: Vec<F>,
    connections: Vec<(M, <M as StreamMuxer>::InboundSubstream, Multiaddr)>,

    // Shared between the whole connection reuse mechanism.
    shared: Arc<Mutex<Shared<M>>>,
}

impl<S, F, M> Stream for ConnectionReuseListener<S, F, M>
where
    S: Stream<Item = F, Error = IoError>,
    F: Future<Item = (M, Multiaddr), Error = IoError>,
    M: StreamMuxer + Clone + 'static, // TODO: 'static :(
{
    type Item = FutureResult<(M::Substream, Multiaddr), IoError>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Check for any incoming connection on the listening socket.
        // Note that since `self.listener` is a `Fuse`, it's not a problem to continue polling even
        // after it is finished or after it error'ed.
        match self.listener.poll() {
            Ok(Async::Ready(Some(upgrade))) => {
                self.current_upgrades.push(upgrade);
            }
            Ok(Async::NotReady) => {}
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
        for n in (0..self.current_upgrades.len()).rev() {
            let mut current_upgrade = self.current_upgrades.swap_remove(n);
            match current_upgrade.poll() {
                Ok(Async::Ready((muxer, client_addr))) => {
                    let next_incoming = muxer.clone().inbound();
                    self.connections
                        .push((muxer.clone(), next_incoming, client_addr.clone()));
                    // We overwrite any current active connection to that multiaddr because we
                    // are the freshest possible connection.
                    self.shared
                        .lock()
                        .active_connections
                        .insert(client_addr, muxer);
                }
                Ok(Async::NotReady) => {
                    self.current_upgrades.push(current_upgrade);
                }
                Err(err) => {
                    // Insert the rest of the pending upgrades, but not the current one.
                    return Ok(Async::Ready(Some(future::err(err))));
                }
            }
        }

        // Check whether any incoming substream is ready.
        for n in (0..self.connections.len()).rev() {
            let (muxer, mut next_incoming, client_addr) = self.connections.swap_remove(n);
            match next_incoming.poll() {
                Ok(Async::Ready(incoming)) => {
                    // A new substream is ready.
                    let mut new_next = muxer.clone().inbound();
                    self.connections
                        .push((muxer, new_next, client_addr.clone()));
                    return Ok(Async::Ready(Some(
                        Ok((incoming, client_addr)).into_future(),
                    )));
                }
                Ok(Async::NotReady) => {
                    self.connections.push((muxer, next_incoming, client_addr));
                }
                Err(err) => {
                    // Insert the rest of the pending connections, but not the current one.
                    return Ok(Async::Ready(Some(future::err(err))));
                }
            }
        }

        // Nothing is ready, return `NotReady`.
        Ok(Async::NotReady)
    }
}

/// Implementation of `Future` that yields the next incoming substream from a dialed connection.
pub struct ConnectionReuseIncoming<M>
where
    M: StreamMuxer,
{
    // Shared between the whole connection reuse system.
    shared: Arc<Mutex<Shared<M>>>,
}

impl<M> Future for ConnectionReuseIncoming<M>
where
    M: Clone + StreamMuxer,
{
    type Item = future::FutureResult<(M::Substream, Multiaddr), IoError>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut lock = self.shared.lock();

        // Try to get any new muxer from `add_to_next_rx`.
        // We push the new muxers to a channel instead of adding them to `next_incoming`, so that
        // tasks are notified when something is pushed.
        loop {
            match lock.add_to_next_rx.poll() {
                Ok(Async::Ready(Some(elem))) => {
                    lock.next_incoming.push(elem);
                }
                Ok(Async::NotReady) => break,
                Ok(Async::Ready(None)) | Err(_) => unreachable!(
                    "the sender and receiver are both in the same struct, therefore \
                     the link can never break"
                ),
            }
        }

        // Check whether any incoming substream is ready.
        for n in (0..lock.next_incoming.len()).rev() {
            let (muxer, mut future, addr) = lock.next_incoming.swap_remove(n);
            match future.poll() {
                Ok(Async::Ready(value)) => {
                    // A substream is ready ; push back the muxer for the next time this function
                    // is called, then return.
                    let next = muxer.clone().inbound();
                    lock.next_incoming.push((muxer, next, addr.clone()));
                    return Ok(Async::Ready(future::ok((value, addr))));
                }
                Ok(Async::NotReady) => {
                    lock.next_incoming.push((muxer, future, addr));
                }
                Err(_) => {
                    // In case of error, we just not push back the element, which drops it.
                }
            }
        }

        // Nothing is ready.
        Ok(Async::NotReady)
    }
}
