// Copyright 2018 Parity Technologies (UK) Ltd.
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
use futures::future::{self, FutureResult};
use futures::stream::FuturesUnordered;
use futures::{stream, task, Async, Future, Poll, Stream};
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use parking_lot::Mutex;
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Read, Write};
use std::ops::{Deref, DerefMut};
use std::sync::{atomic::AtomicUsize, atomic::Ordering, Arc};
use std::collections::hash_map::Entry;
use tokio_io::{AsyncRead, AsyncWrite};
use transport::{MuxedTransport, Transport, UpgradedNode};
use upgrade::ConnectionUpgrade;

use std::clone::Clone;

/// Allows reusing the same muxed connection multiple times.
///
/// Can be created from an `UpgradedNode` through the `From` trait.
///
/// Implements the `Transport` trait.
pub struct ConnectionReuse<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone,
{
    /// Struct shared between most of the `ConnectionReuse` infrastructure.
    shared: Arc<Shared<T, C>>,
}

#[derive(Default)]
struct Counter {
    inner: AtomicUsize
}

impl Counter {
    pub fn next(&self) -> usize {
        self.inner.fetch_add(1, Ordering::Relaxed)
    }
}

enum PeerState<M>
where
    M: StreamMuxer + Clone,
{
    /// Connection is active and can be used to open substreams.
    Active {
        /// The muxer to open new substreams.
        muxer: M,
        /// Unique identifier for this connection in the `ConnectionReuse`.
        connection_id: usize,
        /// Number of open substreams.
        num_substreams: usize,
        /// Id of the listener that created this connection, or `None` if it was opened by a
        /// dialer.
        listener_id: Option<usize>,
    },

    /// Connection is pending.
    // TODO: stronger Future type
    Pending {
        /// Future that produces the muxer.
        future: Box<Future<Item = (M, Multiaddr), Error = IoError>>,
        /// All the tasks to notify when `future` resolves.
        notify: FnvHashMap<usize, task::Task>,
        /// Unique identifier for this connection in the `ConnectionReuse`.
        connection_id: usize,
    },

    /// An earlier connection attempt errored.
    Errored(IoError),
}

/// Struct shared between most of the `ConnectionReuse` infrastructure.
// #[derive(Clone)]
struct Shared<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    /// Underlying transport and connection upgrade, used when we need to dial or listen.
    transport: UpgradedNode<T, C>,

    /// All the connections that were opened, whether successful and/or active or not.
    // TODO: this will grow forever
    connections: Mutex<FnvHashMap<Multiaddr, PeerState<C::Output>>>,

    /// Tasks to notify when one or more new elements were added to `connections`.
    notify_on_new_connec: Mutex<FnvHashMap<usize, task::Task>>,

    /// Counter giving us the next connection_id
    connection_counter: Counter,

    /// Counter giving us the next listener_id
    listener_counter: Counter,
}

impl<T, C> Shared<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    /// notify all `Tasks` in `self.notify_on_new_connec` that a new connection was established
    /// consume the tasks in that process
    fn new_con_notify(&self) {
        let mut notifiers = self.notify_on_new_connec.lock();
        for to_notify in notifiers.drain() {
            to_notify.1.notify();
        }
    }

    /// Insert a new connection, returns Some<Value> if an entry was present, notify listeners
    pub fn insert_connection(
        &self,
        addr: Multiaddr,
        state: PeerState<C::Output>,
    ) -> Option<PeerState<C::Output>> {
        let r = self.connections.lock().insert(addr, state);
        self.new_con_notify();
        r
    }

    /// Removes one substream from an active connection. Closes the connection if necessary.
    pub fn remove_substream(&self, connec_id: usize, addr: &Multiaddr) {
        self.connections.lock().retain(|_, connec| {
            if let PeerState::Active {
                connection_id,
                ref mut num_substreams,
                ..
            } = connec
            {
                if *connection_id == connec_id {
                    *num_substreams -= 1;
                    if *num_substreams == 0 {
                        trace!(
                            "All substreams to {} closed ; closing main connection",
                            addr
                        );
                        return false;
                    }
                }
            }
            true
        });
    }
}

impl<T, C> Shared<T, C>
where
    T: Transport + Clone,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
    UpgradedNode<T, C>: Transport<Output = C::Output> + Clone,
    <UpgradedNode<T, C> as Transport>::Dial: 'static,
    <UpgradedNode<T, C> as Transport>::MultiaddrFuture: 'static,
{
    fn poll_outbound(&self,
        addr: Multiaddr,
        reset: bool) -> Poll<(<C::Output as StreamMuxer>::OutboundSubstream, usize), IoError> {

        let mut conns = self.connections.lock();

        if reset {
            conns.remove(&addr);
        }

        match conns.entry(addr.clone()) {
            Entry::Vacant(e) => {
                // Empty: dial, keep in mind, which task wanted to be notified,
                // then return with `NotReady`
                e.insert(match self.transport.clone().dial(addr.clone()) {
                    Ok(future) => {
                        trace!("Opened new connection to {:?}", addr);
                        let future = future.and_then(|(out, addr)| addr.map(move |a| (out, a)));
                        let future = Box::new(future);
                        let connection_id = self.connection_counter.next();

                        // make sure we are woken up once this connects
                        let mut notify : FnvHashMap<usize, task::Task> = Default::default();
                        notify.insert(TASK_ID.with(|&t| t), task::current());

                        PeerState::Pending {
                            future,
                            notify,
                            connection_id,
                        }
                    }
                    Err(_) => {
                        trace!(
                            "Failed to open connection to {:?}, multiaddr not supported",
                            addr
                        );
                        let err =
                            IoError::new(IoErrorKind::ConnectionRefused, "multiaddr not supported");
                        PeerState::Errored(err)
                    }
                });
                self.new_con_notify();

                Ok(Async::NotReady)
            }
            Entry::Occupied(mut e) => {
                let (replace_with, return_with) = match e.get_mut() {
                    PeerState::Errored(err) => {
                        // Error: return it
                        let io_err = IoError::new(err.kind(), err.to_string());
                        return Err(io_err);
                    }
                    PeerState::Active {
                        muxer,
                        connection_id,
                        ref mut num_substreams,
                        ..
                    } => {
                        // perfect, let's reuse, update the stream and 
                        // return the new ReuseDialOut
                        trace!(
                            "Using existing connection to {} to open outbound substream",
                            addr
                        );
                        // mutating the param in place, nothing to be done
                        *num_substreams += 1;
                        return Ok(Async::Ready( (muxer.clone().outbound(), connection_id.clone())));
                    }
                    PeerState::Pending {
                        ref mut future,
                        ref mut notify,
                        connection_id,
                    } => {
                        // was pending, let's check if that has changed
                        match future.poll() {
                            Ok(Async::NotReady) => {
                                // no it still isn't ready, keep the current task
                                // informed but return with NotReady
                                notify.insert(TASK_ID.with(|&t| t), task::current());
                                return Ok(Async::NotReady);
                            }
                            Err(err) => {
                                // well, looks like connecting failed, replace the
                                // entry then return the Error
                                trace!("Failed new connection to {}: {:?}", addr, err);
                                let io_err = IoError::new(err.kind(), err.to_string());
                                (PeerState::Errored(io_err), Err(err))
                            }
                            Ok(Async::Ready((muxer, client_addr))) => {
                                trace!(
                                    "Successful new connection to {} ({})",
                                    addr,
                                    client_addr
                                );
                                for task in notify {
                                    task.1.notify();
                                }
                                let first_outbound = muxer.clone().outbound();

                                // our connection was upgraded, replace it.
                                (PeerState::Active {
                                    muxer,
                                    connection_id: connection_id.clone(),
                                    num_substreams: 1,
                                    listener_id: None
                                }, Ok(Async::Ready( (first_outbound, *connection_id))))
                            }
                        }
                    }
                };

                e.insert(replace_with);

                return_with
            }
        }
    }

    /// Polls the incoming substreams on all the incoming connections that match the `listener`.
    ///
    /// Returns `Ready(None)` if no connection is matching the `listener`. Returns `NotReady` if
    /// one or more connections are matching the `listener` but they are not ready.
    fn poll_incoming(
        &self,
        listener: Option<usize>,
    ) -> Poll<Option<(<C::Output as StreamMuxer>::Substream, usize, Multiaddr)>, IoError> {
        // Keys of the elements in `connections` to remove afterwards.
        let mut to_remove = Vec::new();
        // Substream to return, if any found.
        let mut ret_value = None;
        let mut found_one = false;
        let mut conn = self.connections.lock();

        for (addr, state) in conn.iter_mut() {
            let res = {
                if let PeerState::Active {
                    ref mut muxer,
                    ref mut num_substreams,
                    connection_id,
                    listener_id,
                } = state
                {
                    if *listener_id == listener {
                        continue;
                    }
                    found_one = true;

                    match muxer.clone().inbound().poll() {
                        Ok(Async::Ready(Some(inner))) => {
                            trace!("New incoming substream from {}", addr);
                            *num_substreams += 1;
                            Ok((inner, connection_id.clone(), addr.clone()))
                        }
                        Ok(Async::Ready(None)) => {
                            // The muxer isn't capable of opening any inbound stream anymore, so
                            // we close the connection entirely.
                            trace!("Removing existing connection to {} as it cannot open inbound anymore", addr);
                            to_remove.push(addr.clone());
                            continue;
                        }
                        Ok(Async::NotReady) => continue,
                        Err(err) => {
                            // If an error happens while opening an inbound stream, we close the
                            // connection entirely.
                            trace!(
                                "Error while opening inbound substream to {}: {:?}",
                                addr,
                                err
                            );
                            to_remove.push(addr.clone());
                            Err(err)
                        }
                    }
                } else {
                    if listener.is_none() {
                        if let PeerState::Pending { ref mut notify, .. } = state {
                            notify.insert(TASK_ID.with(|&t| t), task::current());
                        }
                    }
                    continue;
                }
            };

            ret_value = Some(res);
            break;
        }

        for to_remove in to_remove {
            conn.remove(&to_remove);
        }

        match ret_value {
            Some(Ok(val)) => Ok(Async::Ready(Some(val))),
            Some(Err(err)) => Err(err),
            None => {
                if found_one {
                    Ok(Async::NotReady)
                } else {
                    Ok(Async::Ready(None))
                }
            }
        }
    }
}

impl<T, C> From<UpgradedNode<T, C>> for ConnectionReuse<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone,
{
    #[inline]
    fn from(node: UpgradedNode<T, C>) -> ConnectionReuse<T, C> {
        ConnectionReuse {
            shared: Arc::new(Shared {
                transport: node,
                connections: Default::default(),
                notify_on_new_connec: Default::default(),
                connection_counter: Default::default(),
                listener_counter: Default::default(),
            }),
        }
    }
}

impl<T, C> Transport for ConnectionReuse<T, C>
where
    T: Transport + Clone + 'static,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone + 'static, // TODO: 'static :(
    C::Output: StreamMuxer + Clone,
    <C::Output as StreamMuxer>::Substream: Clone,
    <C::Output as StreamMuxer>::OutboundSubstream: Clone,
    C::MultiaddrFuture: Future<Item = Multiaddr, Error = IoError>,
    C::NamesIter: Clone,
    UpgradedNode<T, C>: Clone,
{
    type Output = ConnectionReuseSubstream<T, C>;
    type MultiaddrFuture = future::FutureResult<Multiaddr, IoError>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = FutureResult<(Self::Output, Self::MultiaddrFuture), IoError>;
    type Dial = ConnectionReuseDial<T, C>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let (listener, new_addr) = match self.shared.transport.clone().listen_on(addr.clone()) {
            Ok((l, a)) => (l, a),
            Err((_, addr)) => {
                return Err((
                    ConnectionReuse {
                        shared: self.shared,
                    },
                    addr,
                ));
            }
        };

        let listener = listener
            .map(|upgr| {
                upgr.and_then(|(out, addr)| {
                    trace!("Waiting for remote's address as listener");
                    addr.map(move |addr| (out, addr))
                })
            })
            .fuse();

        let listener_id = self.shared.listener_counter.next();

        let listener = ConnectionReuseListener {
            shared: self.shared,
            listener,
            listener_id,
            current_upgrades: FuturesUnordered::new(),
        };

        Ok((Box::new(listener) as Box<_>, new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        // If an earlier attempt to dial this multiaddress failed, we clear the error. Otherwise
        // the returned `Future` will immediately produce the error.
        let must_clear = match self.shared.connections.lock().get(&addr) {
            Some(&PeerState::Errored(ref err)) => {
                trace!(
                    "Clearing existing connection to {} which errored earlier: {:?}",
                    addr,
                    err
                );
                true
            }
            _ => false,
        };
        if must_clear {
            self.shared.connections.lock().remove(&addr);
        }

        Ok(ConnectionReuseDial {
            outbound: None,
            shared: self.shared,
            addr,
        })
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.shared
            .transport
            .transport()
            .nat_traversal(server, observed)
    }
}

impl<T, C> MuxedTransport for ConnectionReuse<T, C>
where
    T: Transport + Clone + 'static, // TODO: 'static :(
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone + 'static, // TODO: 'static :(
    C::Output: StreamMuxer + Clone,
    <C::Output as StreamMuxer>::Substream: Clone,
    <C::Output as StreamMuxer>::OutboundSubstream: Clone,
    C::MultiaddrFuture: Future<Item = Multiaddr, Error = IoError>,
    C::NamesIter: Clone,
    UpgradedNode<T, C>: Clone,
{
    type Incoming = ConnectionReuseIncoming<T, C>;
    type IncomingUpgrade =
        future::FutureResult<(ConnectionReuseSubstream<T, C>, Self::MultiaddrFuture), IoError>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        ConnectionReuseIncoming {
            shared: self.shared,
        }
    }
}

static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(0);
// `TASK_ID` is used internally to uniquely identify each task.
task_local!{
    static TASK_ID: usize = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
}

/// Implementation of `Future` for dialing a node.
pub struct ConnectionReuseDial<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    /// The future that will construct the substream, the connection id the muxer comes from, and
    /// the `Future` of the client's multiaddr.
    /// If `None`, we need to grab a new outbound substream from the muxer.
    outbound: Option<ConnectionReuseDialOut<T, C>>,

    // Shared between the whole connection reuse mechanism.
    shared: Arc<Shared<T, C>>,

    // The address we're trying to dial.
    addr: Multiaddr,
}

struct ConnectionReuseDialOut<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    /// The pending outbound substream.
    stream: <C::Output as StreamMuxer>::OutboundSubstream,
    /// Id of the connection that was used to create the substream.
    connection_id: usize,
    /// Address of the remote.
    client_addr: Multiaddr,
}

impl<T, C> Future for ConnectionReuseDial<T, C>
where
    T: Transport + Clone,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
    <C::Output as StreamMuxer>::Substream: Clone,
    <C::Output as StreamMuxer>::OutboundSubstream: Clone,
    UpgradedNode<T, C>: Transport<Output = C::Output> + Clone,
    <UpgradedNode<T, C> as Transport>::Dial: 'static,
    <UpgradedNode<T, C> as Transport>::MultiaddrFuture: 'static,
{
    type Item = (
        ConnectionReuseSubstream<T, C>,
        FutureResult<Multiaddr, IoError>,
    );
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let should_kill_existing_muxer = match self.outbound.take() {
                Some(mut outbound) => {
                    match outbound.stream.poll() {
                        Ok(Async::Ready(Some(inner))) => {
                            trace!("Opened new outgoing substream to {}", self.addr);
                            let shared = self.shared.clone();
                            return Ok(Async::Ready((
                                ConnectionReuseSubstream {
                                    connection_id: outbound.connection_id,
                                    inner,
                                    shared,
                                    addr: outbound.client_addr.clone(),
                                },
                                future::ok(outbound.client_addr),
                            )));
                        }
                        Ok(Async::NotReady) => return Ok(Async::NotReady),
                        Ok(Async::Ready(None)) => {
                            // The muxer can no longer produce outgoing substreams.
                            // Let's reopen a connection.
                            trace!("Closing existing connection to {} ; can't produce outgoing substreams", self.addr);
                            true
                        }
                        Err(err) => {
                            // If we get an error while opening a substream, we decide to ignore it
                            // and open a new muxer.
                            // If opening the muxer produces an error, *then* we will return it.
                            debug!(
                                "Error while opening outgoing substream to {}: {:?}",
                                self.addr, err
                            );
                            true
                        }
                    }
                }
                _ => false,
            };

            match self.shared.poll_outbound(self.addr.clone(), should_kill_existing_muxer) {
                Ok(Async::Ready((stream, connection_id))) => {
                    self.outbound = Some(ConnectionReuseDialOut {
                        stream: stream.clone(),
                        connection_id,
                        client_addr: self.addr.clone(),
                    })
                }
                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady)
                }
                Err(err) => {
                    return Err(err)
                }
            }
        }
    }
}

impl<T, C> Drop for ConnectionReuseDial<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    fn drop(&mut self) {
        if let Some(outbound) = self.outbound.take() {
            self.shared.remove_substream(outbound.connection_id, &outbound.client_addr);
        }
    }
}

/// Implementation of `Stream` for the connections incoming from listening on a specific address.
pub struct ConnectionReuseListener<T, C, L>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    /// The main listener.
    listener: stream::Fuse<L>,
    /// Identifier for this listener. Used to determine which connections were opened by it.
    listener_id: usize,
    /// Opened connections that need to be upgraded.
    current_upgrades: FuturesUnordered<Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>>,

    /// Shared between the whole connection reuse mechanism.
    shared: Arc<Shared<T, C>>,
}

impl<T, C, L, Lu> Stream for ConnectionReuseListener<T, C, L>
where
    T: Transport + Clone,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
    <C::Output as StreamMuxer>::Substream: Clone,
    <C::Output as StreamMuxer>::OutboundSubstream: Clone,
    UpgradedNode<T, C>: Transport<Output = C::Output> + Clone,
    <UpgradedNode<T, C> as Transport>::Dial: 'static,
    <UpgradedNode<T, C> as Transport>::MultiaddrFuture: 'static,
    L: Stream<Item = Lu, Error = IoError>,
    Lu: Future<Item = (C::Output, Multiaddr), Error = IoError> + 'static,
{
    type Item = FutureResult<
        (
            ConnectionReuseSubstream<T, C>,
            FutureResult<Multiaddr, IoError>,
        ),
        IoError,
    >;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Check for any incoming connection on the listening socket.
        // Note that since `self.listener` is a `Fuse`, it's not a problem to continue polling even
        // after it is finished or after it error'ed.
        loop {
            match self.listener.poll() {
                Ok(Async::Ready(Some(upgrade))) => {
                    trace!("New incoming connection");
                    self.current_upgrades.push(Box::new(upgrade));
                }
                Ok(Async::NotReady) => break,
                Ok(Async::Ready(None)) => {
                    debug!("Listener has been closed");
                    break;
                }
                Err(err) => {
                    debug!("Error while polling listener: {:?}", err);
                    return Err(err);
                }
            };
        }

        // Process the connections being upgraded.
        loop {
            let (muxer, client_addr) = match self.current_upgrades.poll() {
                Ok(Async::Ready(Some((muxer, client_addr)))) => (muxer, client_addr),
                Ok(Async::Ready(None)) | Ok(Async::NotReady) => break,
                Err(err) => {
                    // Insert the rest of the pending upgrades, but not the current one.
                    debug!("Error while upgrading listener connection: {:?}", err);
                    return Ok(Async::Ready(Some(future::err(err))));
                }
            };

            // Successfully upgraded a new incoming connection.
            trace!("New multiplexed connection from {}", client_addr);
            let connection_id = self.shared.connection_counter.next();

            self.shared.insert_connection(
                client_addr.clone(),
                PeerState::Active {
                    muxer,
                    connection_id,
                    listener_id: Some(self.listener_id),
                    num_substreams: 1,
                },
            );
        }

        // Poll all the incoming connections on all the connections we opened.
        match self.shared.poll_incoming(Some(self.listener_id)) {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => {
                if self.listener.is_done() && self.current_upgrades.is_empty() {
                    Ok(Async::Ready(None))
                } else {
                    Ok(Async::NotReady)
                }
            }
            Ok(Async::Ready(Some((inner, connection_id, addr)))) => {
                Ok(Async::Ready(Some(future::ok((
                    ConnectionReuseSubstream {
                        inner: inner.clone(),
                        shared: self.shared.clone(),
                        connection_id: connection_id.clone(),
                        addr: addr.clone(),
                    },
                    future::ok(addr.clone()),
                )))))
            }
            Err(err) => Ok(Async::Ready(Some(future::err(IoError::new(
                err.kind(),
                err.to_string(),
            ))))),
        }
    }
}

/// Implementation of `Future` that yields the next incoming substream from a dialed connection.
pub struct ConnectionReuseIncoming<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    // Shared between the whole connection reuse system.
    shared: Arc<Shared<T, C>>,
}

impl<T, C> Future for ConnectionReuseIncoming<T, C>
where
    T: Transport + Clone,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
    <C::Output as StreamMuxer>::Substream: Clone,
    <C::Output as StreamMuxer>::OutboundSubstream: Clone,
    UpgradedNode<T, C>: Transport<Output = C::Output> + Clone,
    <UpgradedNode<T, C> as Transport>::Dial: 'static,
    <UpgradedNode<T, C> as Transport>::MultiaddrFuture: 'static,
{
    type Item = future::FutureResult<
        (
            ConnectionReuseSubstream<T, C>,
            future::FutureResult<Multiaddr, IoError>,
        ),
        IoError,
    >;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.shared.poll_incoming(None) {
            Ok(Async::Ready(Some((inner, connection_id, addr)))) => Ok(Async::Ready(future::ok((
                ConnectionReuseSubstream {
                    inner,
                    shared: self.shared.clone(),
                    connection_id,
                    addr: addr.clone(),
                },
                future::ok(addr),
            )))),
            Ok(Async::Ready(None)) | Ok(Async::NotReady) => {
                self.shared
                    .notify_on_new_connec
                    .lock()
                    .insert(TASK_ID.with(|&v| v), task::current());
                Ok(Async::NotReady)
            }
            Err(err) => Err(err),
        }
    }
}

/// Wraps around the `Substream`.
pub struct ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    inner: <C::Output as StreamMuxer>::Substream,
    shared: Arc<Shared<T, C>>,
    /// Id this connection was created from.
    connection_id: usize,
    /// Address of the remote.
    addr: Multiaddr,
}

impl<T, C> Deref for ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    type Target = <C::Output as StreamMuxer>::Substream;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T, C> DerefMut for ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T, C> Read for ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.inner.read(buf)
    }
}

impl<T, C> AsyncRead for ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
}

impl<T, C> Write for ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.inner.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        self.inner.flush()
    }
}

impl<T, C> AsyncWrite for ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        self.inner.shutdown()
    }
}

impl<T, C> Drop for ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    fn drop(&mut self) {
        self.shared.remove_substream(self.connection_id, &self.addr);
    }
}
