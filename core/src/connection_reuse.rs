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
use futures::future::{self, Either, FutureResult};
use futures::{Async, Future, Poll, Stream};
use futures::stream::FuturesUnordered;
use futures::sync::mpsc;
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use parking_lot::Mutex;
use std::io::{self, Error as IoError};
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};
use transport::{MuxedTransport, Transport, UpgradedNode};
use upgrade::ConnectionUpgrade;

/// Allows reusing the same muxed connection multiple times.
///
/// Can be created from an `UpgradedNode` through the `From` trait.
///
/// Implements the `Transport` trait.
#[derive(Clone)]
pub struct ConnectionReuse<T, C>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
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
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
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
    T: Transport + 'static, // TODO: 'static :(
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone + 'static, // TODO: 'static :(
    C::Output: StreamMuxer + Clone,
    C::MultiaddrFuture: Future<Item = Multiaddr, Error = IoError>,
    C::NamesIter: Clone, // TODO: not elegant
{
    type Output = <C::Output as StreamMuxer>::Substream;
    type MultiaddrFuture = future::FutureResult<Multiaddr, IoError>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = FutureResult<(Self::Output, Self::MultiaddrFuture), IoError>;
    type Dial = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>>;

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

        let listener = listener
            .fuse()
            .map(|upgr| {
                upgr.and_then(|(out, addr)| {
                    addr.map(move |addr| (out, addr))
                })
            });

        let listener = ConnectionReuseListener {
            shared: self.shared.clone(),
            listener: listener,
            current_upgrades: FuturesUnordered::new(),
            connections: Vec::new(),
        };

        Ok((Box::new(listener) as Box<_>, new_addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        // If we already have an active connection, use it!
        let substream = if let Some(muxer) = self.shared
            .lock()
            .active_connections
            .get(&addr)
            .map(|muxer| muxer.clone())
        {
            let a = addr.clone();
            Either::A(muxer.outbound().map(move |s| s.map(move |s| (s, future::ok(a)))))
        } else {
            Either::B(future::ok(None))
        };

        let shared = self.shared.clone();
        let inner = self.inner;
        let future = substream.and_then(move |outbound| {
            if let Some(o) = outbound {
                debug!("Using existing multiplexed connection to {}", addr);
                return Either::A(future::ok(o));
            }
            // The previous stream muxer did not yield a new substream => start new dial
            debug!("No existing connection to {}; dialing", addr);
            match inner.dial(addr.clone()) {
                Ok(dial) => {
                    let future = dial
                    .and_then(move |(muxer, addr_fut)| {
                        trace!("Waiting for remote's address");
                        addr_fut.map(move |addr| (muxer, addr))
                    })
                    .and_then(move |(muxer, addr)| {
                        muxer.clone().outbound().and_then(move |substream| {
                            if let Some(s) = substream {
                                // Replace the active connection because we are the most recent.
                                let mut lock = shared.lock();
                                lock.active_connections.insert(addr.clone(), muxer.clone());
                                // TODO: doesn't need locking ; the sender could be extracted
                                let _ = lock.add_to_next_tx.unbounded_send((
                                    muxer.clone(),
                                    muxer.inbound(),
                                    addr.clone(),
                                ));
                                Ok((s, future::ok(addr)))
                            } else {
                                error!("failed to dial to {}", addr);
                                shared.lock().active_connections.remove(&addr);
                                Err(io::Error::new(io::ErrorKind::Other, "dial failed"))
                            }
                        })
                    });
                    Either::B(Either::A(future))
                }
                Err(_) => {
                    let e = io::Error::new(io::ErrorKind::Other, "transport rejected dial");
                    Either::B(Either::B(future::err(e)))
                }
            }
        });

        Ok(Box::new(future) as Box<_>)
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.transport().nat_traversal(server, observed)
    }
}

impl<T, C> MuxedTransport for ConnectionReuse<T, C>
where
    T: Transport + 'static, // TODO: 'static :(
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone + 'static, // TODO: 'static :(
    C::Output: StreamMuxer + Clone,
    C::MultiaddrFuture: Future<Item = Multiaddr, Error = IoError>,
    C::NamesIter: Clone, // TODO: not elegant
{
    type Incoming = ConnectionReuseIncoming<C::Output>;
    type IncomingUpgrade =
        future::FutureResult<(<C::Output as StreamMuxer>::Substream, Self::MultiaddrFuture), IoError>;

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
    M: StreamMuxer,
{
    // The main listener. `S` is from the underlying transport.
    listener: S,
    current_upgrades: FuturesUnordered<F>,
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
    type Item = FutureResult<(M::Substream, FutureResult<Multiaddr, IoError>), IoError>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Check for any incoming connection on the listening socket.
        // Note that since `self.listener` is a `Fuse`, it's not a problem to continue polling even
        // after it is finished or after it error'ed.
        loop {
            match self.listener.poll() {
                Ok(Async::Ready(Some(upgrade))) => {
                    self.current_upgrades.push(upgrade);
                }
                Ok(Async::NotReady) => break,
                Ok(Async::Ready(None)) => {
                    debug!("listener has been closed");
                    if self.connections.is_empty() && self.current_upgrades.is_empty() {
                        return Ok(Async::Ready(None));
                    }
                    break
                }
                Err(err) => {
                    debug!("error while polling listener: {:?}", err);
                    if self.connections.is_empty() && self.current_upgrades.is_empty() {
                        return Err(err);
                    }
                    break
                }
            }
        }

        loop {
            match self.current_upgrades.poll() {
                Ok(Async::Ready(Some((muxer, client_addr)))) => {
                    let next_incoming = muxer.clone().inbound();
                    self.connections
                        .push((muxer.clone(), next_incoming, client_addr.clone()));
                }
                Err(err) => {
                    debug!("error while upgrading listener connection: {:?}", err);
                    return Ok(Async::Ready(Some(future::err(err))));
                }
                _ => break,
            }
        }

        // Check whether any incoming substream is ready.
        for n in (0..self.connections.len()).rev() {
            let (muxer, mut next_incoming, client_addr) = self.connections.swap_remove(n);
            match next_incoming.poll() {
                Ok(Async::Ready(None)) => {
                    // stream muxer gave us a `None` => connection should be considered closed
                    debug!("no more inbound substreams on {}", client_addr);
                    self.shared.lock().active_connections.remove(&client_addr);
                }
                Ok(Async::Ready(Some(incoming))) => {
                    // We overwrite any current active connection to that multiaddr because we
                    // are the freshest possible connection.
                    self.shared
                        .lock()
                        .active_connections
                        .insert(client_addr.clone(), muxer.clone());
                    // A new substream is ready.
                    let mut new_next = muxer.clone().inbound();
                    self.connections
                        .push((muxer, new_next, client_addr.clone()));
                    return Ok(Async::Ready(Some(
                        future::ok((incoming, future::ok(client_addr))),
                    )));
                }
                Ok(Async::NotReady) => {
                    self.connections.push((muxer, next_incoming, client_addr));
                }
                Err(err) => {
                    debug!("error while upgrading the multiplexed incoming connection: {:?}", err);
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
    type Item = future::FutureResult<(M::Substream, future::FutureResult<Multiaddr, IoError>), IoError>;
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
                Ok(Async::Ready(None)) => {
                    debug!("no inbound substream for {}", addr);
                    lock.active_connections.remove(&addr);
                }
                Ok(Async::Ready(Some(value))) => {
                    // A substream is ready ; push back the muxer for the next time this function
                    // is called, then return.
                    debug!("New incoming substream");
                    let next = muxer.clone().inbound();
                    lock.next_incoming.push((muxer, next, addr.clone()));
                    return Ok(Async::Ready(future::ok((value, future::ok(addr)))));
                }
                Ok(Async::NotReady) => {
                    lock.next_incoming.push((muxer, future, addr));
                }
                Err(err) => {
                    // In case of error, we just not push back the element, which drops it.
                    debug!("ConnectionReuse incoming: one of the \
                           multiplexed substreams produced an error: {:?}",
                           err);
                }
            }
        }

        // Nothing is ready.
        Ok(Async::NotReady)
    }
}
