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
use std::collections::hash_map::Entry;
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Read, Write};
use std::ops::{Deref, DerefMut};
use std::sync::{atomic::AtomicUsize, atomic::AtomicBool, atomic::Ordering, Arc};
use tokio_io::{AsyncRead, AsyncWrite};
use transport::{MuxedTransport, Transport, UpgradedNode};
use upgrade::ConnectionUpgrade;
use std::clone::Clone;

/// Allows reusing the same muxed connection multiple times.
///
/// Can be created from an `UpgradedNode` through the `From` trait.
///
/// Implements the `Transport` trait
#[derive(Clone)]
pub struct ConnectionReuse<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone,
{
    /// Struct shared between most of the `ConnectionReuse` infrastructure.
    manager: Arc<ConnectionsManager<T, C>>,
}

/// Keeps an internal counter, for every new number issued
/// will increase its internal state.
#[derive(Default)]
struct Counter {
    inner: AtomicUsize,
}

impl Counter {
    /// Returns the next counter, increasing the internal state
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
        /// Id of the listener that created this connection, or `None` if it was opened by a
        /// dialer.
        listener_id: Option<usize>,
        /// whether this connection was closed
        closed: Arc<AtomicBool>
    },

    /// Connection is pending.
    // TODO: stronger Future type
    Pending {
        /// Future that produces the muxer.
        future: Box<Future<Item = (M, Multiaddr), Error = IoError>>,
        /// All the tasks to notify when `future` resolves.
        notify: Vec<(usize, task::Task)>,
    },

    /// An earlier connection attempt errored.
    Errored(IoError),
}

/// Struct shared between most of the `ConnectionReuse` infrastructure.
/// Knows about all connections and their current state, allows one to poll for
/// incoming and outbound connections, while managing all state-transitions that
/// might occur automatically.
// #[derive(Clone)]
struct ConnectionsManager<T, C>
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
    notify_on_new_connec: Mutex<Vec<(usize, task::Task)>>,

    /// Counter giving us the next listener_id
    listener_counter: Counter,
}

impl<T, C> ConnectionsManager<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{
    /// Notify all `Tasks` in `self.notify_on_new_connec` that a new connection was established
    /// consume the tasks in that process
    fn new_con_notify(&self) {
        let mut notifiers = self.notify_on_new_connec.lock();
        trace!("Notify {} listeners about a new connection", notifiers.len());
        for (_, task) in notifiers.drain(..) {
            task.notify();
        }
    }

    /// Register `task` under id if not yet registered
    fn register_notifier(&self, id: usize, task: task::Task) {
        let mut notifiers = self.notify_on_new_connec.lock();
        if !notifiers.iter().any(|(t, _)| *t == id) {
            trace!("Notify registered for task: {}", id);
            notifiers.push((id, task))
        }
    }

    /// resets the connections if the given PeerState is active and has
    /// substreams
    fn reset_conn(&self, state: Option<PeerState<C::Output>>) {
        if let Some(PeerState::Active { closed, .. }) = state {
            closed.store(false, Ordering::Relaxed);
        }
    }

    /// Clear the cached connection if the entry contains an error. Returns whether an error was
    /// found and has been removed.
    fn clear_error(&self, addr: Multiaddr) -> bool {
        let mut conns = self.connections.lock();

        if let Entry::Occupied(e) = conns.entry(addr) {
            if let PeerState::Errored(ref err) = e.get() {
                trace!(
                    "Clearing existing connection to {} which errored earlier: {:?}",
                    e.key(),
                    err
                );
            } else {
                return false; // nothing to do, quit
            }

            // clear the error
            e.remove();
            true
        } else {
            false
        }
    }
}

fn poll_for_substream<M>(mut outbound: M::OutboundSubstream, addr: &Multiaddr)
    -> (Result<Option<M::Substream>, IoError>, Option<PeerState<M>>)
where 
    M: StreamMuxer + Clone
{
    match outbound.poll() {
        Ok(Async::Ready(Some(inner))) => {
            trace!("Opened new outgoing substream to {}", addr);
            // all good, return the new stream
            (Ok(Some(inner)), None)
        }
        Ok(Async::NotReady) => (Ok(None), None),
        Ok(Async::Ready(None)) => {
            // The muxer can no longer produce outgoing substreams.
            // Let's reopen a connection.
            trace!("Closing existing connection to {} ; can't produce outgoing substreams", addr);

            let err = IoError::new(IoErrorKind::ConnectionRefused, "No Streams left");
            let io_err = IoError::new(IoErrorKind::ConnectionRefused, "No Streams left");
            (Err(err), Some(PeerState::Errored(io_err)))
        }
        Err(err) => {
            // If we get an error while opening a substream, we decide to ignore it
            // and open a new muxer.
            // If opening the muxer produces an error, *then* we will return it.
            debug!(
                "Error while opening outgoing substream to {}: {:?}",
                addr, err
            );
            let io_err = IoError::new(err.kind(), err.to_string());
            (Err(io_err), Some(PeerState::Errored(err)))
        }
    }
}

impl<T, C> ConnectionsManager<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
    UpgradedNode<T, C>: Transport<Output = C::Output> + Clone,
    <UpgradedNode<T, C> as Transport>::Dial: 'static,
    <UpgradedNode<T, C> as Transport>::MultiaddrFuture: 'static,
{
    /// Informs the Manager about a new inbound connection that has been
    /// established, so it
    fn new_inbound(&self, addr: Multiaddr, listener_id: usize, muxer: C::Output) {
        let mut conns = self.connections.lock();
        let new_state = PeerState::Active {
            muxer,
            listener_id: Some(listener_id),
            closed: Arc::new(AtomicBool::new(false))
        };
        let mut old_state = conns.insert(addr.clone(), new_state);

        if let Some(PeerState::Pending { ref mut notify, .. }) = old_state {
            // we need to wake some up, that we have a connection now
            trace!("Found incoming for pending connection to {}", addr);
            for (_, task) in notify.drain(..) {
                task.notify()
            }
        } else {
            self.reset_conn(old_state);
        }
    }

    /// Polls for the outbound stream of `addr`. Clears the cached value first if `reset` is true.
    /// Dials, if no connection is in the internal cache. Returns `Ok(Async::NotReady)` as long as
    /// the connection isn't establed and ready yet.
    fn poll_outbound(
        &self,
        addr: Multiaddr,
    ) -> Poll<(<C::Output as StreamMuxer>::Substream, Arc<AtomicBool>), IoError> {
        let mut conns = self.connections.lock();

        let state = conns.entry(addr.clone()).or_insert_with(|| {
            let state = match self.transport.clone().dial(addr.clone()) {
                Ok(future) => {
                    trace!("Opening new connection to {:?}", addr);
                    let future = future.and_then(|(out, addr)| addr.map(move |a| (out, a)));
                    let future = Box::new(future);

                    PeerState::Pending {
                        future,
                        // make sure we are woken up once this connects
                        notify: vec![(TASK_ID.with(|&t| t), task::current())],
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
            };
            self.new_con_notify();
            // Althought we've just started in pending state, there are transports, which are
            // immediately ready - namely 'memory' - so we need to poll our future right away
            // rather than waiting for another poll on it.
            state
        });

        let (replace_with, return_with) = match state {
            PeerState::Errored(err) => {
                // Error: return it
                let io_err = IoError::new(err.kind(), err.to_string());
                return Err(io_err);
            }
            PeerState::Active {
                muxer,
                closed,
                ..
            } => {
                if closed.load(Ordering::Relaxed) {
                    (PeerState::Errored(IoError::new(IoErrorKind::BrokenPipe, "Connection closed")),
                     Err(IoError::new(IoErrorKind::BrokenPipe, "Connection closed")))
                } else {
                    // perfect, let's reuse, update the stream_id and
                    // return a new outbound
                    trace!(
                        "Using existing connection to {} to open outbound substream",
                        addr
                    );
                    match poll_for_substream(muxer.clone().outbound(), &addr) {
                        (Ok(Some(stream)), _) => {
                            return Ok(Async::Ready((stream, closed.clone())));
                        }
                        (Err(res), Some(replace)) => (replace, Err(res)),
                        (Ok(None), _) => return Ok(Async::NotReady),
                        (Err(res), None ) => return Err(res),
                    }
                }
            }
            PeerState::Pending {
                ref mut future,
                ref mut notify,
            } => {
                // was pending, let's check if that has changed
                match future.poll() {
                    Ok(Async::NotReady) => {
                        // no it still isn't ready, keep the current task
                        // informed but return with NotReady
                        let t_id = TASK_ID.with(|&t| t);
                        if !notify.iter().any(|(id, _)| *id == t_id) {
                            notify.push((t_id, task::current()));
                        }
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
                        trace!("Successful new connection to {} ({})", addr, client_addr);
                        for (_, task) in notify.drain(..) {
                            task.notify();
                        }
                        match poll_for_substream(muxer.clone().outbound(), &addr) {
                            (Ok(Some(stream)), _) => {
                                let closed = Arc::new(AtomicBool::new(false));
                                let state = PeerState::Active {
                                    muxer,
                                    closed: closed.clone(),
                                    listener_id: None,
                                };

                                (state, Ok(Async::Ready((stream, closed))))
                            }
                            (Err(res), Some(replace)) => (replace, Err(res)),
                            (Ok(None), _) => return Ok(Async::NotReady),
                            (Err(res), None ) => return Err(res),

                        }
                    }
                }
            }
        };

        *state = replace_with;
        return_with
    }

    /// Polls the incoming substreams on all the incoming connections that match the `listener`.
    ///
    /// Returns `Ready<Some<Muxer, connection_id, client_addr>>` for the first matching connection.
    /// Return `Ready(None)` if no connection is matching the `listener`. Returns `NotReady` if
    /// one or more connections are matching the `listener` but none is ready yet.
    fn poll_incoming(
        &self,
        listener: Option<usize>,
    ) -> Poll<Option<(<C::Output as StreamMuxer>::Substream, Arc<AtomicBool>, Multiaddr)>, IoError> {
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
                    listener_id,
                    closed,
                } = state
                {
                    if closed.load(Ordering::Relaxed) {
                        to_remove.push(addr.clone());
                        continue
                    }

                    if *listener_id != listener {
                        continue;
                    }

                    found_one = true;

                    match muxer.clone().inbound().poll() {
                        Ok(Async::Ready(Some(stream))) => {
                            trace!("New incoming substream from {}", addr);
                            Ok((stream, closed.clone(), addr.clone()))
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
                            let t_id = TASK_ID.with(|&t| t);
                            if notify.iter().all(|(id, _)| *id != t_id) {
                                notify.push((t_id, task::current()));
                            }
                        }
                    }
                    continue;
                }
            };

            ret_value = Some(res);
            break;
        }

        for to_remove in to_remove {
            self.reset_conn(conn.remove(&to_remove));
        }

        match ret_value {
            Some(Ok(val)) => Ok(Async::Ready(Some(val))),
            Some(Err(err)) => Err(err),
            None => {
                if found_one {
                    self.register_notifier(TASK_ID.with(|&v| v), task::current());
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
            manager: Arc::new(ConnectionsManager {
                transport: node,
                connections: Default::default(),
                notify_on_new_connec: Default::default(),
                listener_counter: Default::default(),
            }),
        }
    }
}

impl<T, C> Transport for ConnectionReuse<T, C>
where
    T: Transport + 'static,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone + 'static, // TODO: 'static :(
    C::Output: StreamMuxer + Clone,
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
        let (listener, new_addr) = match self.manager.transport.clone().listen_on(addr.clone()) {
            Ok((l, a)) => (l, a),
            Err((_, addr)) => {
                return Err((
                    ConnectionReuse {
                        manager: self.manager,
                    },
                    addr,
                ));
            }
        };

        let listener = listener
            .map(|upgr| {
                upgr.and_then(|(out, addr)| {
                    trace!("Waiting for remote's address as listener");
                    addr.map(move |addr| {
                        trace!("Address found:{:?}", addr);
                        (out, addr)
                    })
                })
            })
            .fuse();

        let listener_id = self.manager.listener_counter.next();

        let listener = ConnectionReuseListener {
            manager: self.manager,
            listener,
            listener_id,
            current_upgrades: FuturesUnordered::new(),
        };

        Ok((Box::new(listener), new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        // If an earlier attempt to dial this multiaddress failed, we clear the error. Otherwise
        // the returned `Future` will immediately produce the error.
        self.manager.clear_error(addr.clone());
        Ok(ConnectionReuseDial {
            manager: self.manager,
            addr,
        })
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.manager
            .transport
            .transport()
            .nat_traversal(server, observed)
    }
}

impl<T, C> MuxedTransport for ConnectionReuse<T, C>
where
    T: Transport + 'static, // TODO: 'static :(
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone + 'static, // TODO: 'static :(
    C::Output: StreamMuxer + Clone,
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
            manager: self.manager,
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
    // ConnectionsManager between the whole connection reuse mechanism.
    manager: Arc<ConnectionsManager<T, C>>,

    // The address we're trying to dial.
    addr: Multiaddr,
}

impl<T, C> Future for ConnectionReuseDial<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
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
        match self.manager.poll_outbound(self.addr.clone()) {
            Ok(Async::Ready((inner, closed))) => {
                Ok(Async::Ready(
                    (ConnectionReuseSubstream { inner, closed},
                     future::ok(self.addr.clone()))))
            }
            Err(err) => Err(err),
            Ok(Async::NotReady) => Ok(Async::NotReady)
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

    /// ConnectionsManager between the whole connection reuse mechanism.
    manager: Arc<ConnectionsManager<T, C>>,
}

impl<T, C, L, Lu> Stream for ConnectionReuseListener<T, C, L>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
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
                Ok(Async::Ready(Some(val))) => val,
                Ok(Async::Ready(None)) | Ok(Async::NotReady) => break,
                Err(err) => {
                    // Insert the rest of the pending upgrades, but not the current one.
                    debug!("Error while upgrading listener connection: {:?}", err);
                    return Ok(Async::Ready(Some(future::err(err))));
                }
            };

            // Successfully upgraded a new incoming connection.
            trace!("New multiplexed connection from {}", client_addr);

            self.manager
                .new_inbound(client_addr.clone(), self.listener_id, muxer);
        }

        // Poll all the incoming connections on all the connections we opened.
        match self.manager.poll_incoming(Some(self.listener_id)) {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => {
                if self.listener.is_done() && self.current_upgrades.is_empty() {
                    trace!("Ready but empty; we are, too; Closing!");
                    Ok(Async::Ready(None))
                } else {
                    trace!("Ready but empty; but we're still going strong; We'll wait!");
                    self.manager
                        .register_notifier(TASK_ID.with(|&v| v), task::current());
                    Ok(Async::NotReady)
                }
            }
            Ok(Async::Ready(Some((inner, closed, addr)))) => {
                trace!("Stream for {:?} ready.", addr);
                Ok(Async::Ready(Some(future::ok(
                    (ConnectionReuseSubstream {inner, closed},
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
    // ConnectionsManager between the whole connection reuse system.
    manager: Arc<ConnectionsManager<T, C>>,
}

impl<T, C> Future for ConnectionReuseIncoming<T, C>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture> + Clone,
    C::Output: StreamMuxer + Clone + 'static,
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
        match self.manager.poll_incoming(None) {
            Ok(Async::Ready(Some((inner, closed, addr)))) => {
                trace!("New Incoming Substream for {}", addr);
                Ok(Async::Ready(future::ok((
                    ConnectionReuseSubstream {inner, closed},
                    future::ok(addr),))))
            },
            Ok(Async::Ready(None)) | Ok(Async::NotReady) => {
                // wake us up, when there is a new connection
                self.manager
                    .register_notifier(TASK_ID.with(|&v| v), task::current());
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
    closed: Arc<AtomicBool>
}

impl<T, C> ConnectionReuseSubstream<T, C>
where
    T: Transport,
    C: ConnectionUpgrade<T::Output, T::MultiaddrFuture>,
    C::Output: StreamMuxer + Clone,
{

    /// Reset the connection this Substream is muxed out of
    /// will result in it being removed from the cache on the
    /// next poll
    pub fn reset_connection(self) {
        self.closed.store(true, Ordering::Relaxed);
    }

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
        if self.closed.load(Ordering::Relaxed) {
            return Err(IoError::new(IoErrorKind::BrokenPipe, "Connection has been closed"));
        }

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
        if self.closed.load(Ordering::Relaxed) {
            return Err(IoError::new(IoErrorKind::BrokenPipe, "Connection has been closed"));
        }
        self.inner.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(IoError::new(IoErrorKind::BrokenPipe, "Connection has been closed"));
        }
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
        if self.closed.load(Ordering::Relaxed) {
            return Err(IoError::new(IoErrorKind::BrokenPipe, "Connection has been closed"));
        }
        self.inner.shutdown()
    }
}