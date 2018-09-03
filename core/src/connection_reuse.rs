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
use futures::{Async, Future, Poll, Stream, stream, task};
use futures::stream::FuturesUnordered;
use multiaddr::Multiaddr;
use muxing::{self, StreamMuxer};
use parking_lot::Mutex;
use std::collections::hash_map::Entry;
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Read, Write};
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering};
use tokio_io::{AsyncRead, AsyncWrite};
use transport::{MuxedTransport, Transport};

/// Allows reusing the same muxed connection multiple times.
///
/// Can be created from an `UpgradedNode` through the `From` trait.
///
/// Implements the `Transport` trait.
pub struct ConnectionReuse<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    /// Struct shared between most of the `ConnectionReuse` infrastructure.
    shared: Arc<Mutex<Shared<T, D, M>>>,
}

impl<T, D, M> Clone for ConnectionReuse<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    #[inline]
    fn clone(&self) -> Self {
        ConnectionReuse {
            shared: self.shared.clone(),
        }
    }
}

/// Struct shared between most of the `ConnectionReuse` infrastructure.
struct Shared<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    /// Underlying transport and connection upgrade, used when we need to dial or listen.
    transport: T,

    /// All the connections that were opened, whether successful and/or active or not.
    // TODO: this will grow forever
    connections: FnvHashMap<Multiaddr, PeerState<D, M>>,

    /// Tasks to notify when one or more new elements were added to `connections`.
    notify_on_new_connec: FnvHashMap<usize, task::Task>,

    /// Next `connection_id` to use when opening a connection.
    next_connection_id: u64,

    /// Next `listener_id` for the next listener we create.
    next_listener_id: u64,
}

enum PeerState<D, M> where M: StreamMuxer {
    /// Connection is active and can be used to open substreams.
    Active {
        /// The muxer to open new substreams.
        muxer: Arc<M>,
        /// Custom data passed to the output.
        custom_data: D,
        /// Future of the address of the client.
        client_addr: Multiaddr,
        /// Unique identifier for this connection in the `ConnectionReuse`.
        connection_id: u64,
        /// Number of open substreams.
        num_substreams: u64,
        /// Id of the listener that created this connection, or `None` if it was opened by a
        /// dialer.
        listener_id: Option<u64>,
    },

    /// Connection is pending.
    // TODO: stronger Future type
    Pending {
        /// Future that produces the muxer.
        future: Box<Future<Item = ((D, M), Multiaddr), Error = IoError>>,
        /// All the tasks to notify when `future` resolves.
        notify: FnvHashMap<usize, task::Task>,
    },

    /// An earlier connection attempt errored.
    Errored(IoError),

    /// The `PeerState` is poisonned. Happens if a panic happened while executing some of the
    /// functions.
    Poisonned,
}

impl<T, D, M> ConnectionReuse<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    #[inline]
    pub(crate) fn new(node: T) -> ConnectionReuse<T, D, M> {
        ConnectionReuse {
            shared: Arc::new(Mutex::new(Shared {
                transport: node,
                connections: Default::default(),
                notify_on_new_connec: Default::default(),
                next_connection_id: 0,
                next_listener_id: 0,
            })),
        }
    }
}

impl<T, D, M> Transport for ConnectionReuse<T, D, M>
where
    T: Transport + 'static, // TODO: 'static :(
    T: Transport<Output = (D, M)> + Clone + 'static, // TODO: 'static :(
    M: StreamMuxer + 'static,
    D: Clone + 'static,
    T: Clone,
{
    type Output = (D, ConnectionReuseSubstream<T, D, M>);
    type MultiaddrFuture = future::FutureResult<Multiaddr, IoError>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = FutureResult<(Self::Output, Self::MultiaddrFuture), IoError>;
    type Dial = ConnectionReuseDial<T, D, M>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let mut shared = self.shared.lock();

        let (listener, new_addr) = match shared.transport.clone().listen_on(addr.clone()) {
            Ok((l, a)) => (l, a),
            Err((_, addr)) => {
                return Err((
                    ConnectionReuse {
                        shared: self.shared.clone(),
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

        let listener_id = shared.next_listener_id;
        shared.next_listener_id += 1;

        let listener = ConnectionReuseListener {
            shared: self.shared.clone(),
            listener,
            listener_id,
            current_upgrades: FuturesUnordered::new(),
        };

        Ok((Box::new(listener) as Box<_>, new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let mut shared = self.shared.lock();

        // If an earlier attempt to dial this multiaddress failed, we clear the error. Otherwise
        // the returned `Future` will immediately produce the error.
        let must_clear = match shared.connections.get(&addr) {
            Some(&PeerState::Errored(ref err)) => {
                trace!("Clearing existing connection to {} which errored earlier: {:?}", addr, err);
                true
            },
            _ => false,
        };
        if must_clear {
            shared.connections.remove(&addr);
        }

        Ok(ConnectionReuseDial {
            outbound: None,
            shared: self.shared.clone(),
            addr,
        })
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.shared.lock().transport.nat_traversal(server, observed)
    }
}

impl<T, D, M> MuxedTransport for ConnectionReuse<T, D, M>
where
    T: Transport + 'static, // TODO: 'static :(
    T: Transport<Output = (D, M)> + Clone + 'static, // TODO: 'static :(
    M: StreamMuxer + 'static,
    D: Clone + 'static,
    T: Clone,
{
    type Incoming = ConnectionReuseIncoming<T, D, M>;
    type IncomingUpgrade =
        future::FutureResult<((D, ConnectionReuseSubstream<T, D, M>), Self::MultiaddrFuture), IoError>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        ConnectionReuseIncoming {
            shared: self.shared.clone(),
        }
    }
}

static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(0);
// `TASK_ID` is used internally to uniquely identify each task.
task_local!{
    static TASK_ID: usize = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
}

/// Implementation of `Future` for dialing a node.
pub struct ConnectionReuseDial<T, D, M>
where 
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    /// The future that will construct the substream, the connection id the muxer comes from, and
    /// the `Future` of the client's multiaddr.
    /// If `None`, we need to grab a new outbound substream from the muxer.
    outbound: Option<ConnectionReuseDialOut<D, M>>,

    // Shared between the whole connection reuse mechanism.
    shared: Arc<Mutex<Shared<T, D, M>>>,

    // The address we're trying to dial.
    addr: Multiaddr,
}

struct ConnectionReuseDialOut<D, M>
where 
    M: StreamMuxer,
{
    /// Custom data for the connection.
    custom_data: D,
    /// The pending outbound substream.
    stream: muxing::OutboundSubstreamRefWrapFuture<Arc<M>>,
    /// Id of the connection that was used to create the substream.
    connection_id: u64,
    /// Address of the remote.
    client_addr: Multiaddr,
}

impl<T, D, M> Future for ConnectionReuseDial<T, D, M>
where 
    T: Transport<Output = (D, M)> + Clone,
    M: StreamMuxer + 'static,
    D: Clone + 'static,
    <T as Transport>::Dial: 'static,
    <T as Transport>::MultiaddrFuture: 'static,
{
    type Item = ((D, ConnectionReuseSubstream<T, D, M>), FutureResult<Multiaddr, IoError>);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let should_kill_existing_muxer;
            if let Some(mut outbound) = self.outbound.take() {
                match outbound.stream.poll() {
                    Ok(Async::Ready(Some(inner))) => {
                        trace!("Opened new outgoing substream to {}", self.addr);
                        let substream = ConnectionReuseSubstream {
                            connection_id: outbound.connection_id,
                            shared: self.shared.clone(),
                            inner,
                            addr: outbound.client_addr.clone(),
                        };
                        return Ok(Async::Ready(((outbound.custom_data, substream), future::ok(outbound.client_addr))));
                    },
                    Ok(Async::NotReady) => {
                        self.outbound = Some(outbound);
                        return Ok(Async::NotReady);
                    },
                    Ok(Async::Ready(None)) => {
                        // The muxer can no longer produce outgoing substreams.
                        // Let's reopen a connection.
                        trace!("Closing existing connection to {} ; can't produce outgoing substreams", self.addr);
                        should_kill_existing_muxer = true;
                    },
                    Err(err) => {
                        // If we get an error while opening a substream, we decide to ignore it
                        // and open a new muxer.
                        // If opening the muxer produces an error, *then* we will return it.
                        debug!("Error while opening outgoing substream to {}: {:?}", self.addr, err);
                        should_kill_existing_muxer = true;
                    },
                }
            } else {
                should_kill_existing_muxer = false;
            }

            // If we reach this point, that means we have to fill `self.outbound`.
            // If `should_kill_existing_muxer`, do not use any existing connection but create a
            // new one instead.
            let mut shared = self.shared.lock();
            let shared = &mut *shared;      // Avoids borrow errors

            // TODO: could be optimized
            if should_kill_existing_muxer {
                shared.connections.remove(&self.addr);
            }
            let connec = match shared.connections.entry(self.addr.clone()) {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => {
                    // Build the connection.
                    let state = match shared.transport.clone().dial(self.addr.clone()) {
                        Ok(future) => {
                            trace!("Opened new connection to {:?}", self.addr);
                            let future = future.and_then(|(out, addr)| addr.map(move |a| (out, a)));
                            let future = Box::new(future);
                            PeerState::Pending { future, notify: Default::default() }
                        },
                        Err(_) => {
                            trace!("Failed to open connection to {:?}, multiaddr not supported", self.addr);
                            let err = IoError::new(IoErrorKind::ConnectionRefused, "multiaddr not supported");
                            PeerState::Errored(err)
                        },
                    };

                    for task in shared.notify_on_new_connec.drain() {
                        task.1.notify();
                    }

                    e.insert(state)
                },
            };

            match mem::replace(&mut *connec, PeerState::Poisonned) {
                PeerState::Active { muxer, custom_data, connection_id, listener_id, mut num_substreams, client_addr } => {
                    let outbound = muxing::outbound_from_ref_and_wrap(muxer.clone());
                    num_substreams += 1;
                    *connec = PeerState::Active { muxer, custom_data: custom_data.clone(), connection_id, listener_id, num_substreams, client_addr: client_addr.clone() };
                    trace!("Using existing connection to {} to open outbound substream", self.addr);
                    self.outbound = Some(ConnectionReuseDialOut {
                        custom_data,
                        stream: outbound,
                        connection_id,
                        client_addr,
                    });
                },
                PeerState::Pending { mut future, mut notify } => {
                    match future.poll() {
                        Ok(Async::Ready(((custom_data, muxer), client_addr))) => {
                            trace!("Successful new connection to {} ({})", self.addr, client_addr);
                            for task in notify {
                                task.1.notify();
                            }
                            let muxer = Arc::new(muxer);
                            let first_outbound = muxing::outbound_from_ref_and_wrap(muxer.clone());
                            let connection_id = shared.next_connection_id;
                            shared.next_connection_id += 1;
                            *connec = PeerState::Active { muxer, custom_data: custom_data.clone(), connection_id, num_substreams: 1, listener_id: None, client_addr: client_addr.clone() };
                            self.outbound = Some(ConnectionReuseDialOut {
                                custom_data,
                                stream: first_outbound,
                                connection_id,
                                client_addr,
                            });
                        },
                        Ok(Async::NotReady) => {
                            notify.insert(TASK_ID.with(|&t| t), task::current());
                            *connec = PeerState::Pending { future, notify };
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            trace!("Failed new connection to {}: {:?}", self.addr, err);
                            let io_err = IoError::new(err.kind(), err.to_string());
                            *connec = PeerState::Errored(err);
                            return Err(io_err);
                        },
                    }
                },
                PeerState::Errored(err) => {
                    trace!("Existing new connection to {} errored earlier: {:?}", self.addr, err);
                    let io_err = IoError::new(err.kind(), err.to_string());
                    *connec = PeerState::Errored(err);
                    return Err(io_err);
                },
                PeerState::Poisonned => {
                    panic!("Poisonned peer state");
                },
            }
        }
    }
}

impl<T, D, M> Drop for ConnectionReuseDial<T, D, M>
where 
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    fn drop(&mut self) {
        if let Some(outbound) = self.outbound.take() {
            let mut shared = self.shared.lock();
            remove_one_substream(&mut *shared, outbound.connection_id, &outbound.client_addr);
        }
    }
}

/// Implementation of `Stream` for the connections incoming from listening on a specific address.
pub struct ConnectionReuseListener<T, D, M, L>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    /// The main listener.
    listener: stream::Fuse<L>,
    /// Identifier for this listener. Used to determine which connections were opened by it.
    listener_id: u64,
    /// Opened connections that need to be upgraded.
    current_upgrades: FuturesUnordered<Box<Future<Item = (T::Output, Multiaddr), Error = IoError>>>,

    /// Shared between the whole connection reuse mechanism.
    shared: Arc<Mutex<Shared<T, D, M>>>,
}

impl<T, D, M, L, Lu> Stream for ConnectionReuseListener<T, D, M, L>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
    D: Clone,
    L: Stream<Item = Lu, Error = IoError>,
    Lu: Future<Item = (T::Output, Multiaddr), Error = IoError> + 'static,
{
    type Item = FutureResult<((D, ConnectionReuseSubstream<T, D, M>), FutureResult<Multiaddr, IoError>), IoError>;
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
            match self.current_upgrades.poll() {
                Ok(Async::Ready(Some(((custom_data, muxer), client_addr)))) => {
                    // Successfully upgraded a new incoming connection.
                    trace!("New multiplexed connection from {}", client_addr);
                    let mut shared = self.shared.lock();
                    let muxer = Arc::new(muxer);
                    let connection_id = shared.next_connection_id;
                    shared.next_connection_id += 1;
                    let state = PeerState::Active { muxer, custom_data, connection_id, listener_id: Some(self.listener_id), num_substreams: 1, client_addr: client_addr.clone() };
                    shared.connections.insert(client_addr, state);
                    for to_notify in shared.notify_on_new_connec.drain() {
                        to_notify.1.notify();
                    }
                }
                Ok(Async::Ready(None)) | Ok(Async::NotReady) => {
                    break;
                },
                Err(err) => {
                    // Insert the rest of the pending upgrades, but not the current one.
                    debug!("Error while upgrading listener connection: {:?}", err);
                    return Ok(Async::Ready(Some(future::err(err))));
                }
            }
        }

        // Poll all the incoming connections on all the connections we opened.
        let mut shared = self.shared.lock();
        match poll_incoming(&self.shared, &mut shared, Some(self.listener_id)) {
            Ok(Async::Ready(None)) => {
                if self.listener.is_done() && self.current_upgrades.is_empty() {
                    Ok(Async::Ready(None))
                } else {
                    Ok(Async::NotReady)
                }
            },
            Ok(Async::Ready(Some(substream))) => {
                Ok(Async::Ready(Some(substream)))
            },
            Ok(Async::NotReady) => {
                Ok(Async::NotReady)
            }
            Err(err) => {
                Ok(Async::Ready(Some(future::err(err))))
            }
        }
    }
}

/// Implementation of `Future` that yields the next incoming substream from a dialed connection.
pub struct ConnectionReuseIncoming<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    // Shared between the whole connection reuse system.
    shared: Arc<Mutex<Shared<T, D, M>>>,
}

impl<T, D, M> Future for ConnectionReuseIncoming<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
    D: Clone,
{
    type Item = future::FutureResult<((D, ConnectionReuseSubstream<T, D, M>), future::FutureResult<Multiaddr, IoError>), IoError>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut shared = self.shared.lock();
        match poll_incoming(&self.shared, &mut shared, None) {
            Ok(Async::Ready(Some(substream))) => {
                Ok(Async::Ready(substream))
            },
            Ok(Async::Ready(None)) | Ok(Async::NotReady) => {
                // TODO: will add an element to the list every time
                shared.notify_on_new_connec.insert(TASK_ID.with(|&v| v), task::current());
                Ok(Async::NotReady)
            },
            Err(err) => Err(err)
        }
    }
}

/// Polls the incoming substreams on all the incoming connections that match the `listener`.
///
/// Returns `Ready(None)` if no connection is matching the `listener`. Returns `NotReady` if
/// one or more connections are matching the `listener` but they are not ready.
fn poll_incoming<T, D, M>(shared_arc: &Arc<Mutex<Shared<T, D, M>>>, shared: &mut Shared<T, D, M>, listener: Option<u64>)
    -> Poll<Option<FutureResult<((D, ConnectionReuseSubstream<T, D, M>), FutureResult<Multiaddr, IoError>), IoError>>, IoError>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
    D: Clone,
{
    // Keys of the elements in `shared.connections` to remove afterwards.
    let mut to_remove = Vec::new();
    // Substream to return, if any found.
    let mut ret_value = None;
    let mut found_one = false;

    for (addr, state) in shared.connections.iter_mut() {
        match *state {
            PeerState::Active { ref custom_data, ref muxer, ref mut num_substreams, connection_id, ref client_addr, listener_id } => {
                if listener_id != listener {
                    continue;
                }
                found_one = true;

                match muxer.poll_inbound() {
                    Ok(Async::Ready(Some(inner))) => {
                        trace!("New incoming substream from {}", client_addr);
                        *num_substreams += 1;
                        let substream = ConnectionReuseSubstream {
                            inner: muxing::substream_from_ref(muxer.clone(), inner),
                            shared: shared_arc.clone(),
                            connection_id,
                            addr: client_addr.clone(),
                        };
                        ret_value = Some(Ok(((custom_data.clone(), substream), future::ok(client_addr.clone()))));
                        break;
                    },
                    Ok(Async::Ready(None)) => {
                        // The muxer isn't capable of opening any inbound stream anymore, so
                        // we close the connection entirely.
                        trace!("Removing existing connection to {} as it cannot open inbound anymore", addr);
                        to_remove.push(addr.clone());
                    },
                    Ok(Async::NotReady) => (),
                    Err(err) => {
                        // If an error happens while opening an inbound stream, we close the
                        // connection entirely.
                        trace!("Error while opening inbound substream to {}: {:?}", addr, err);
                        to_remove.push(addr.clone());
                        ret_value = Some(Err(err));
                        break;
                    },
                }
            },
            PeerState::Pending { ref mut notify, .. } => {
                notify.insert(TASK_ID.with(|&t| t), task::current());
            },
            PeerState::Errored(_) => {},
            PeerState::Poisonned => {
                panic!("Poisonned peer state");
            },
        }
    }

    for to_remove in to_remove {
        shared.connections.remove(&to_remove);
    }

    match ret_value {
        Some(Ok(val)) => Ok(Async::Ready(Some(future::ok(val)))),
        Some(Err(err)) => Err(err),
        None => {
            if found_one {
                Ok(Async::NotReady)
            } else {
                Ok(Async::Ready(None))
            }
        },
    }
}

/// Removes one substream from an active connection. Closes the connection if necessary.
fn remove_one_substream<T, D, M>(shared: &mut Shared<T, D, M>, connec_id: u64, addr: &Multiaddr)
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    shared.connections.retain(|_, connec| {
        if let PeerState::Active { connection_id, ref mut num_substreams, .. } = connec {
            if *connection_id == connec_id {
                *num_substreams -= 1;
                if *num_substreams == 0 {
                    trace!("All substreams to {} closed ; closing main connection", addr);
                    return false;
                }
            }
        }

        true
    });
}

/// Wraps around the `Substream`.
pub struct ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    inner: muxing::SubstreamRef<Arc<M>>,
    shared: Arc<Mutex<Shared<T, D, M>>>,
    /// Id this connection was created from.
    connection_id: u64,
    /// Address of the remote.
    addr: Multiaddr,
}

impl<T, D, M> Deref for ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    type Target = muxing::SubstreamRef<Arc<M>>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T, D, M> DerefMut for ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T, D, M> Read for ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.inner.read(buf)
    }
}

impl<T, D, M> AsyncRead for ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
}

impl<T, D, M> Write for ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
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

impl<T, D, M> AsyncWrite for ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        self.inner.shutdown()
    }
}

impl<T, D, M> Drop for ConnectionReuseSubstream<T, D, M>
where
    T: Transport,
    T: Transport<Output = (D, M)>,
    M: StreamMuxer,
{
    fn drop(&mut self) {
        let mut shared = self.shared.lock();
        remove_one_substream(&mut *shared, self.connection_id, &self.addr);
    }
}
