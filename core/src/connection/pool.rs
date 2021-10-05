// Copyright 2021 Protocol Labs.
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

use crate::{
    connection::{
        handler::{THandlerError, THandlerInEvent, THandlerOutEvent},
        Connected, ConnectionError, ConnectionHandler, ConnectionLimit, IncomingInfo,
        IntoConnectionHandler, PendingConnectionError, PendingPoint, Substream,
    },
    muxing::StreamMuxer,
    network::DialError,
    transport::TransportError,
    ConnectedPoint, Executor, Multiaddr, PeerId,
};
use fnv::FnvHashMap;
use futures::prelude::*;
use futures::{
    channel::{mpsc, oneshot},
    future::{poll_fn, BoxFuture},
    ready,
    stream::FuturesUnordered,
};
use smallvec::SmallVec;
use std::{
    collections::{hash_map, HashMap},
    convert::TryFrom as _,
    error, fmt,
    num::NonZeroU32,
    pin::Pin,
    task::Context,
    task::Poll,
};
use void::Void;

mod task;

/// A connection `Pool` manages a set of connections for each peer.
pub struct Pool<THandler: IntoConnectionHandler, TMuxer, TTransErr> {
    local_id: PeerId,

    /// The connection counter(s).
    counters: ConnectionCounters,

    /// The managed connections of each peer that are currently considered established.
    established: FnvHashMap<
        PeerId,
        FnvHashMap<ConnectionId, EstablishedConnectionInfo<THandlerInEvent<THandler>>>,
    >,

    /// The pending connections that are currently being negotiated.
    pending: HashMap<ConnectionId, PendingConnectionInfo<THandler>>,

    /// Next available identifier for a new connection / task.
    next_connection_id: ConnectionId,

    /// Size of the task command buffer (per task).
    task_command_buffer_size: usize,

    /// The executor to use for running the background tasks. If `None`,
    /// the tasks are kept in `local_spawns` instead and polled on the
    /// current thread when the [`Pool`] is polled for new events.
    executor: Option<Box<dyn Executor + Send>>,

    /// If no `executor` is configured, tasks are kept in this set and
    /// polled on the current thread when the [`Pool`] is polled for new events.
    local_spawns: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Send>>>,

    /// Sender distributed to pending tasks for reporting events back
    /// to the pool.
    pending_connection_events_tx: mpsc::Sender<task::PendingConnectionEvent<TMuxer, TTransErr>>,

    /// Receiver for events reported from pending tasks.
    pending_connection_events_rx: mpsc::Receiver<task::PendingConnectionEvent<TMuxer, TTransErr>>,

    /// Sender distributed to established tasks for reporting events back
    /// to the pool.
    established_connection_events_tx: mpsc::Sender<task::EstablishedConnectionEvent<THandler>>,

    /// Receiver for events reported from established tasks.
    established_connection_events_rx: mpsc::Receiver<task::EstablishedConnectionEvent<THandler>>,
}

#[derive(Debug)]
struct EstablishedConnectionInfo<TInEvent> {
    peer_id: PeerId,
    endpoint: ConnectedPoint,
    /// Channel endpoint to send messages to the task.
    sender: mpsc::Sender<task::Command<TInEvent>>,
}

struct PendingConnectionInfo<THandler> {
    peer_id: Option<PeerId>,
    handler: THandler,
    endpoint: PendingPoint,
    /// When dropped, notifies the task which can then terminate.
    _drop_notifier: oneshot::Sender<Void>,
}

impl<THandler: IntoConnectionHandler, TMuxer, TTransErr> fmt::Debug
    for Pool<THandler, TMuxer, TTransErr>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("Pool")
            .field("counters", &self.counters)
            // TODO: Should we add more fields?
            .finish()
    }
}

impl<THandler: IntoConnectionHandler, TMuxer, TTransErr> Unpin
    for Pool<THandler, TMuxer, TTransErr>
{
}

/// Event that can happen on the `Pool`.
pub enum PoolEvent<'a, THandler: IntoConnectionHandler, TMuxer, TTransErr> {
    /// A new connection has been established.
    ConnectionEstablished {
        connection: EstablishedConnection<'a, THandlerInEvent<THandler>>,
        num_established: NonZeroU32,
        outgoing: Option<Vec<(Multiaddr, TransportError<TTransErr>)>>,
    },

    /// An established connection was closed.
    ///
    /// A connection may close if
    ///
    ///   * it encounters an error, which includes the connection being
    ///     closed by the remote. In this case `error` is `Some`.
    ///   * it was actively closed by [`EstablishedConnection::start_close`],
    ///     i.e. a successful, orderly close.
    ///   * it was actively closed by [`Pool::disconnect`], i.e.
    ///     dropped without an orderly close.
    ///
    ConnectionClosed {
        id: ConnectionId,
        /// Information about the connection that errored.
        connected: Connected,
        /// The error that occurred, if any. If `None`, the connection
        /// was closed by the local peer.
        error: Option<ConnectionError<THandlerError<THandler>>>,
        /// A reference to the pool that used to manage the connection.
        pool: &'a mut Pool<THandler, TMuxer, TTransErr>,
        /// The remaining number of established connections to the same peer.
        num_established: u32,
        handler: THandler::Handler,
    },

    /// A connection attempt failed.
    PendingConnectionError {
        /// The ID of the failed connection.
        id: ConnectionId,
        /// The local endpoint of the failed connection.
        endpoint: PendingPoint,
        /// The error that occurred.
        error: PendingConnectionError<TTransErr>,
        /// The handler that was supposed to handle the connection,
        /// if the connection failed before the handler was consumed.
        handler: THandler,
        /// The (expected) peer of the failed connection.
        peer: Option<PeerId>,
    },

    /// A node has produced an event.
    ConnectionEvent {
        /// The connection that has generated the event.
        connection: EstablishedConnection<'a, THandlerInEvent<THandler>>,
        /// The produced event.
        event: THandlerOutEvent<THandler>,
    },

    /// The connection to a node has changed its address.
    AddressChange {
        /// The connection that has changed address.
        connection: EstablishedConnection<'a, THandlerInEvent<THandler>>,
        /// The new endpoint.
        new_endpoint: ConnectedPoint,
        /// The old endpoint.
        old_endpoint: ConnectedPoint,
    },
}

impl<'a, THandler: IntoConnectionHandler, TMuxer, TTransErr> fmt::Debug
    for PoolEvent<'a, THandler, TMuxer, TTransErr>
where
    TTransErr: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            PoolEvent::ConnectionEstablished {
                ref connection,
                ref outgoing,
                ..
            } => f
                .debug_tuple("PoolEvent::OutgoingConnectionEstablished")
                .field(connection)
                .field(outgoing)
                .finish(),
            PoolEvent::ConnectionClosed {
                ref id,
                ref connected,
                ref error,
                ..
            } => f
                .debug_struct("PoolEvent::ConnectionClosed")
                .field("id", id)
                .field("connected", connected)
                .field("error", error)
                .finish(),
            PoolEvent::PendingConnectionError {
                ref id, ref error, ..
            } => f
                .debug_struct("PoolEvent::PendingConnectionError")
                .field("id", id)
                .field("error", error)
                .finish(),
            PoolEvent::ConnectionEvent {
                ref connection,
                ref event,
            } => f
                .debug_struct("PoolEvent::ConnectionEvent")
                .field("peer", &connection.peer_id())
                .field("event", event)
                .finish(),
            PoolEvent::AddressChange {
                ref connection,
                ref new_endpoint,
                ref old_endpoint,
            } => f
                .debug_struct("PoolEvent::AddressChange")
                .field("peer", &connection.peer_id())
                .field("new_endpoint", new_endpoint)
                .field("old_endpoint", old_endpoint)
                .finish(),
        }
    }
}

impl<THandler: IntoConnectionHandler, TMuxer: Send + 'static, TTransErr: Send + 'static>
    Pool<THandler, TMuxer, TTransErr>
{
    /// Creates a new empty `Pool`.
    pub fn new(local_id: PeerId, config: PoolConfig, limits: ConnectionLimits) -> Self {
        let (pending_connection_events_tx, pending_connection_events_rx) =
            mpsc::channel(config.task_event_buffer_size);
        let (established_connection_events_tx, established_connection_events_rx) =
            mpsc::channel(config.task_event_buffer_size);
        Pool {
            local_id,
            counters: ConnectionCounters::new(limits),
            established: Default::default(),
            pending: Default::default(),
            next_connection_id: ConnectionId(0),
            task_command_buffer_size: config.task_command_buffer_size,
            executor: config.executor,
            local_spawns: FuturesUnordered::new(),
            pending_connection_events_tx,
            pending_connection_events_rx,
            established_connection_events_tx,
            established_connection_events_rx,
        }
    }

    /// Gets the dedicated connection counters.
    pub fn counters(&self) -> &ConnectionCounters {
        &self.counters
    }

    /// Adds a pending outgoing connection to the pool in the form of a `Future`
    /// that establishes and negotiates the connection.
    ///
    /// Returns an error if the limit of pending outgoing connections
    /// has been reached.
    pub fn add_outgoing(
        &mut self,
        dial: crate::network::concurrent_dial::ConcurrentDial<TMuxer, TTransErr>,
        handler: THandler,
        expected_peer_id: Option<PeerId>,
    ) -> Result<ConnectionId, DialError<THandler>>
    where
        THandler: IntoConnectionHandler + Send + 'static,
        THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>> + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TTransErr: error::Error + Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
    {
        if let Err(limit) = self.counters.check_max_pending_outgoing() {
            return Err(DialError::ConnectionLimit { limit, handler });
        };

        let connection_id = self.next_connection_id();

        let (drop_notifier, drop_receiver) = oneshot::channel();

        self.spawn(
            task::new_for_pending_outgoing_connection(
                connection_id,
                dial,
                drop_receiver,
                self.pending_connection_events_tx.clone(),
            )
            .boxed(),
        );

        self.counters.inc_pending(&PendingPoint::Dialer);
        self.pending.insert(
            connection_id,
            PendingConnectionInfo {
                peer_id: expected_peer_id,
                handler,
                endpoint: PendingPoint::Dialer,
                _drop_notifier: drop_notifier,
            },
        );
        Ok(connection_id)
    }

    /// Adds a pending incoming connection to the pool in the form of a
    /// `Future` that establishes and negotiates the connection.
    ///
    /// Returns an error if the limit of pending incoming connections
    /// has been reached.
    pub fn add_incoming<TFut>(
        &mut self,
        future: TFut,
        handler: THandler,
        info: IncomingInfo<'_>,
    ) -> Result<ConnectionId, ConnectionLimit>
    where
        TFut: Future<Output = Result<(PeerId, TMuxer), PendingConnectionError<TTransErr>>>
            + Send
            + 'static,
        // TODO: Bounds still needed here?
        THandler: IntoConnectionHandler + Send + 'static,
        THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>> + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TTransErr: error::Error + Send + 'static,
        // TODO: Still needed?
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
    {
        // TODO: This is a hack. Fix.
        let endpoint = info.clone().to_connected_point();
        // TODO: We loose the handler here.
        self.counters.check_max_pending_incoming()?;

        let connection_id = self.next_connection_id();

        let (drop_notifier, drop_receiver) = oneshot::channel();

        self.spawn(
            task::new_for_pending_incoming_connection(
                connection_id,
                future,
                drop_receiver,
                self.pending_connection_events_tx.clone(),
            )
            .boxed(),
        );

        self.counters.inc_pending_incoming();
        self.pending.insert(
            connection_id,
            PendingConnectionInfo {
                peer_id: None,
                handler,
                endpoint: endpoint.into(),
                _drop_notifier: drop_notifier,
            },
        );
        Ok(connection_id)
    }

    /// Gets an entry representing a connection in the pool.
    ///
    /// Returns `None` if the pool has no connection with the given ID.
    pub fn get(&mut self, id: ConnectionId) -> Option<PoolConnection<'_, THandler>> {
        if let hash_map::Entry::Occupied(entry) = self.pending.entry(id) {
            Some(PoolConnection::Pending(PendingConnection {
                entry,
                counters: &mut self.counters,
            }))
        } else {
            self.established
                .iter_mut()
                .find_map(|(_, cs)| match cs.entry(id) {
                    hash_map::Entry::Occupied(entry) => {
                        Some(PoolConnection::Established(EstablishedConnection { entry }))
                    }
                    hash_map::Entry::Vacant(_) => None,
                })
        }
    }

    /// Gets an established connection from the pool by ID.
    pub fn get_established(
        &mut self,
        id: ConnectionId,
    ) -> Option<EstablishedConnection<'_, THandlerInEvent<THandler>>> {
        match self.get(id) {
            Some(PoolConnection::Established(c)) => Some(c),
            _ => None,
        }
    }

    /// Gets a pending outgoing connection by ID.
    pub fn get_outgoing(&mut self, id: ConnectionId) -> Option<PendingConnection<'_, THandler>> {
        match self.pending.entry(id) {
            hash_map::Entry::Occupied(entry) => Some(PendingConnection {
                entry,
                counters: &mut self.counters,
            }),
            hash_map::Entry::Vacant(_) => None,
        }
    }

    /// Returns true if we are connected to the given peer.
    ///
    /// This will return true only after a `NodeReached` event has been produced by `poll()`.
    pub fn is_connected(&self, id: &PeerId) -> bool {
        self.established.contains_key(id)
    }

    /// Returns the number of connected peers, i.e. those with at least one
    /// established connection in the pool.
    pub fn num_peers(&self) -> usize {
        self.established.len()
    }

    /// (Forcefully) close all connections to the given peer.
    ///
    /// All connections to the peer, whether pending or established are
    /// closed asap and no more events from these connections are emitted
    /// by the pool effective immediately.
    pub fn disconnect(&mut self, peer: &PeerId) {
        if let Some(conns) = self.established.get_mut(peer) {
            // TODO: Detour via EstablishedConnection not ideal, but at least only one code path to
            // start closing a connection.
            let connection_ids = conns.iter().map(|(id, _)| *id).collect::<Vec<_>>();

            for id in connection_ids.into_iter() {
                let established_connection = match conns.entry(id) {
                    hash_map::Entry::Occupied(entry) => EstablishedConnection { entry },
                    hash_map::Entry::Vacant(_) => {
                        unreachable!("Iterating established connections.")
                    }
                };

                established_connection.start_close();
            }
        }

        let pending_connections = self
            .pending
            .iter()
            .filter(|(_, PendingConnectionInfo { peer_id, .. })| peer_id.as_ref() == Some(peer))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();

        for pending_connection in pending_connections.into_iter() {
            let pending_connection = match self.pending.entry(pending_connection) {
                hash_map::Entry::Occupied(entry) => PendingConnection {
                    entry,
                    counters: &mut self.counters,
                },
                hash_map::Entry::Vacant(_) => unreachable!("Iterating pending connections"),
            };

            pending_connection.abort();
        }
    }

    /// Counts the number of established connections to the given peer.
    pub fn num_peer_established(&self, peer: PeerId) -> u32 {
        num_peer_established(&self.established, peer)
    }

    /// Returns an iterator over all established connections of `peer`.
    pub fn iter_peer_established<'a>(
        &'a mut self,
        peer: &PeerId,
    ) -> EstablishedConnectionIter<'a, impl Iterator<Item = ConnectionId>, THandlerInEvent<THandler>>
    {
        let ids = self
            .iter_peer_established_info(peer)
            .map(|(id, _endpoint)| *id)
            .collect::<SmallVec<[ConnectionId; 10]>>()
            .into_iter();

        EstablishedConnectionIter {
            connections: self.established.get_mut(peer),
            ids,
        }
    }

    /// Returns an iterator for information on all pending incoming connections.
    pub fn iter_pending_incoming(&self) -> impl Iterator<Item = IncomingInfo<'_>> {
        self.iter_pending_info()
            .filter_map(|(_, ref endpoint, _)| match endpoint {
                PendingPoint::Listener {
                    local_addr,
                    send_back_addr,
                } => Some(IncomingInfo {
                    local_addr,
                    send_back_addr,
                }),
                PendingPoint::Dialer => None,
            })
    }

    /// Returns an iterator over all connection IDs and associated endpoints
    /// of established connections to `peer` known to the pool.
    pub fn iter_peer_established_info(
        &self,
        peer: &PeerId,
    ) -> impl Iterator<Item = (&ConnectionId, &ConnectedPoint)> {
        match self.established.get(peer) {
            Some(conns) => either::Either::Left(
                conns
                    .iter()
                    .map(|(id, EstablishedConnectionInfo { endpoint, .. })| (id, endpoint)),
            ),
            None => either::Either::Right(std::iter::empty()),
        }
    }

    /// Returns an iterator over all pending connection IDs together
    /// with associated endpoints and expected peer IDs in the pool.
    pub fn iter_pending_info(
        &self,
    ) -> impl Iterator<Item = (&ConnectionId, &PendingPoint, &Option<PeerId>)> + '_ {
        self.pending.iter().map(
            |(
                id,
                PendingConnectionInfo {
                    peer_id, endpoint, ..
                },
            )| (id, endpoint, peer_id),
        )
    }

    /// Returns an iterator over all connected peers, i.e. those that have
    /// at least one established connection in the pool.
    pub fn iter_connected(&self) -> impl Iterator<Item = &PeerId> {
        self.established.keys()
    }

    fn next_connection_id(&mut self) -> ConnectionId {
        let connection_id = self.next_connection_id;
        self.next_connection_id.0 += 1;

        connection_id
    }

    fn spawn(&mut self, task: BoxFuture<'static, ()>) {
        if let Some(executor) = &mut self.executor {
            executor.exec(task);
        } else {
            self.local_spawns.push(task);
        }
    }

    /// Polls the connection pool for events.
    ///
    /// > **Note**: We use a regular `poll` method instead of implementing `Stream`,
    /// > because we want the `Pool` to stay borrowed if necessary.
    pub fn poll<'a>(
        &'a mut self,
        cx: &mut Context<'_>,
    ) -> Poll<PoolEvent<'a, THandler, TMuxer, TTransErr>>
    where
        TMuxer: StreamMuxer + Send + Sync + 'static,
        THandler: IntoConnectionHandler + Send + 'static,
        THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>> + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
    {
        // Advance the tasks in `local_spawns`.
        while let Poll::Ready(Some(_)) = self.local_spawns.poll_next_unpin(cx) {}

        // Poll for events of established connections.
        //
        // Note that established connections are polled before pending connections, thus
        // prioritizing established connections over pending connections.
        match self.established_connection_events_rx.poll_next_unpin(cx) {
            Poll::Pending => {}
            Poll::Ready(None) => unreachable!("Pool holds both sender and receiver."),

            Poll::Ready(Some(task::EstablishedConnectionEvent::Notify { id, peer_id, event })) => {
                match self
                    .established
                    .get_mut(&peer_id)
                    .expect("Receive `Notify` event for established peer.")
                    .entry(id)
                {
                    hash_map::Entry::Occupied(entry) => {
                        return Poll::Ready(PoolEvent::ConnectionEvent {
                            connection: EstablishedConnection { entry },
                            event,
                        })
                    }
                    hash_map::Entry::Vacant(_) => {
                        unreachable!("Receive `Notify` event from established connection")
                    }
                }
            }
            Poll::Ready(Some(task::EstablishedConnectionEvent::AddressChange {
                id,
                peer_id,
                new_address,
            })) => {
                let connection = self
                    .established
                    .get_mut(&peer_id)
                    .expect("Receive `AddressChange` event for established peer.")
                    .get_mut(&id)
                    .expect("Receive `AddressChange` event from established connection");
                let mut new_endpoint = connection.endpoint.clone();
                new_endpoint.set_remote_address(new_address);
                let old_endpoint =
                    std::mem::replace(&mut connection.endpoint, new_endpoint.clone());

                match self.get(id) {
                    Some(PoolConnection::Established(connection)) => {
                        return Poll::Ready(PoolEvent::AddressChange {
                            connection,
                            new_endpoint,
                            old_endpoint,
                        })
                    }
                    _ => unreachable!("since `entry` is an `EstablishedEntry`."),
                }
            }
            Poll::Ready(Some(task::EstablishedConnectionEvent::Closed {
                id,
                peer_id,
                error,
                handler,
            })) => {
                let connections = self
                    .established
                    .get_mut(&peer_id)
                    .expect("`Closed` event for established connection");
                let EstablishedConnectionInfo { endpoint, .. } =
                    connections.remove(&id).expect("Connection to be present");
                self.counters.dec_established(&endpoint);
                let num_established = u32::try_from(connections.len()).unwrap();
                if num_established == 0 {
                    self.established.remove(&peer_id);
                }
                return Poll::Ready(PoolEvent::ConnectionClosed {
                    id,
                    connected: Connected { endpoint, peer_id },
                    error,
                    num_established,
                    pool: self,
                    handler,
                });
            }
        }

        // Poll for events of pending connections.
        loop {
            let event = match self.pending_connection_events_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => event,
                Poll::Pending => break,
                Poll::Ready(None) => unreachable!("Pool holds both sender and receiver."),
            };

            match event {
                task::PendingConnectionEvent::ConnectionEstablished {
                    id,
                    peer_id,
                    muxer,
                    outgoing,
                } => {
                    let PendingConnectionInfo {
                        peer_id: expected_peer_id,
                        handler,
                        endpoint,
                        _drop_notifier,
                    } = self
                        .pending
                        .remove(&id)
                        .expect("Entry in `self.pending` for previously pending connection.");

                    self.counters.dec_pending(&endpoint);

                    let (endpoint, dialing_errors) = match (endpoint, outgoing) {
                        (PendingPoint::Dialer, Some((address, errors))) => {
                            (ConnectedPoint::Dialer { address }, Some(errors))
                        }
                        (
                            PendingPoint::Listener {
                                local_addr,
                                send_back_addr,
                            },
                            None,
                        ) => (
                            ConnectedPoint::Listener {
                                local_addr,
                                send_back_addr,
                            },
                            None,
                        ),
                        (PendingPoint::Dialer, None) => unreachable!(
                            "Established incoming connection via pending outgoing connection."
                        ),
                        (PendingPoint::Listener { .. }, Some(_)) => unreachable!(
                            "Established outgoing connection via pending incoming connection."
                        ),
                    };

                    let error = self
                        .counters
                        // Check general established connection limit.
                        .check_max_established(&endpoint)
                        .map_err(PendingConnectionError::ConnectionLimit)
                        // Check per-peer established connection limit.
                        .and_then(|()| {
                            self.counters
                                .check_max_established_per_peer(num_peer_established(
                                    &self.established,
                                    peer_id,
                                ))
                                .map_err(PendingConnectionError::ConnectionLimit)
                        })
                        // Check expected peer id matches.
                        .and_then(|()| {
                            if let Some(peer) = expected_peer_id {
                                if peer != peer_id {
                                    return Err(PendingConnectionError::InvalidPeerId);
                                }
                            }
                            Ok(())
                        })
                        // Check peer is not local peer.
                        .and_then(|()| {
                            if self.local_id == peer_id {
                                Err(PendingConnectionError::InvalidPeerId)
                            } else {
                                Ok(())
                            }
                        });

                    if let Err(error) = error {
                        self.spawn(
                            poll_fn(move |cx| {
                                // TODO: Should we send the result back to the Pool?
                                let _ = ready!(muxer.close(cx));
                                Poll::Ready(())
                            })
                            .boxed(),
                        );

                        return Poll::Ready(PoolEvent::PendingConnectionError {
                            id,
                            endpoint: endpoint.into(),
                            error,
                            handler,
                            peer: Some(peer_id),
                        });
                    }

                    // Add the connection to the pool.
                    let conns = self.established.entry(peer_id).or_default();
                    let num_established = NonZeroU32::new(u32::try_from(conns.len() + 1).unwrap())
                        .expect("n + 1 is always non-zero; qed");
                    self.counters.inc_established(&endpoint);

                    let (command_sender, command_receiver) =
                        mpsc::channel(self.task_command_buffer_size);
                    conns.insert(
                        id,
                        EstablishedConnectionInfo {
                            peer_id,
                            endpoint: endpoint.clone(),
                            sender: command_sender,
                        },
                    );

                    let connected = Connected { peer_id, endpoint };

                    let connection =
                        super::Connection::new(muxer, handler.into_handler(&connected));
                    self.spawn(
                        task::new_for_established_connection(
                            id,
                            peer_id,
                            connection,
                            command_receiver,
                            self.established_connection_events_tx.clone(),
                        )
                        .boxed(),
                    );

                    match self.get(id) {
                        Some(PoolConnection::Established(connection)) => {
                            return Poll::Ready(PoolEvent::ConnectionEstablished {
                                connection,
                                num_established,
                                outgoing: dialing_errors,
                            })
                        }
                        _ => unreachable!("since `entry` is an `EstablishedEntry`."),
                    }
                }
                task::PendingConnectionEvent::PendingFailed { id, error } => {
                    if let Some(PendingConnectionInfo {
                        peer_id,
                        handler,
                        endpoint,
                        _drop_notifier,
                    }) = self.pending.remove(&id)
                    {
                        self.counters.dec_pending(&endpoint);
                        return Poll::Ready(PoolEvent::PendingConnectionError {
                            id,
                            endpoint,
                            error,
                            handler,
                            peer: peer_id,
                        });
                    }
                }
            }
        }

        Poll::Pending
    }
}

/// A connection in a [`Pool`].
pub enum PoolConnection<'a, THandler: IntoConnectionHandler> {
    Pending(PendingConnection<'a, THandler>),
    Established(EstablishedConnection<'a, THandlerInEvent<THandler>>),
}

/// A pending connection in a pool.
pub struct PendingConnection<'a, THandler: IntoConnectionHandler> {
    entry: hash_map::OccupiedEntry<'a, ConnectionId, PendingConnectionInfo<THandler>>,
    counters: &'a mut ConnectionCounters,
}

impl<THandler: IntoConnectionHandler> PendingConnection<'_, THandler> {
    /// Returns the local connection ID.
    pub fn id(&self) -> ConnectionId {
        *self.entry.key()
    }

    /// Returns the (expected) identity of the remote peer, if known.
    pub fn peer_id(&self) -> &Option<PeerId> {
        &self.entry.get().peer_id
    }

    /// Returns information about this endpoint of the connection.
    pub fn endpoint(&self) -> &PendingPoint {
        &self.entry.get().endpoint
    }

    /// Aborts the connection attempt, closing the connection.
    pub fn abort(self) {
        self.counters.dec_pending(&self.entry.get().endpoint);
        self.entry.remove();
    }
}

/// An established connection in a pool.
pub struct EstablishedConnection<'a, TInEvent> {
    entry: hash_map::OccupiedEntry<'a, ConnectionId, EstablishedConnectionInfo<TInEvent>>,
}

impl<TInEvent> fmt::Debug for EstablishedConnection<'_, TInEvent>
where
    TInEvent: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("EstablishedConnection")
            .field("entry", &self.entry)
            .finish()
    }
}

impl<TInEvent> EstablishedConnection<'_, TInEvent> {
    /// Returns information about the connected endpoint.
    pub fn endpoint(&self) -> &ConnectedPoint {
        &self.entry.get().endpoint
    }

    /// Returns the identity of the connected peer.
    pub fn peer_id(&self) -> PeerId {
        self.entry.get().peer_id
    }

    /// Returns the local connection ID.
    pub fn id(&self) -> ConnectionId {
        *self.entry.key()
    }

    /// (Asynchronously) sends an event to the connection handler.
    ///
    /// If the handler is not ready to receive the event, either because
    /// it is busy or the connection is about to close, the given event
    /// is returned with an `Err`.
    ///
    /// If execution of this method is preceded by successful execution of
    /// `poll_ready_notify_handler` without another intervening execution
    /// of `notify_handler`, it only fails if the connection is now about
    /// to close.
    pub fn notify_handler(&mut self, event: TInEvent) -> Result<(), TInEvent> {
        let cmd = task::Command::NotifyHandler(event);
        self.entry
            .get_mut()
            .sender
            .try_send(cmd)
            .map_err(|e| match e.into_inner() {
                task::Command::NotifyHandler(event) => event,
                _ => unreachable!("Expect failed send to return initial event."),
            })
    }

    /// Checks if `notify_handler` is ready to accept an event.
    ///
    /// Returns `Ok(())` if the handler is ready to receive an event via `notify_handler`.
    ///
    /// Returns `Err(())` if the background task associated with the connection
    /// is terminating and the connection is about to close.
    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        self.entry.get_mut().sender.poll_ready(cx).map_err(|_| ())
    }

    /// Initiates a graceful close of the connection.
    ///
    /// Has no effect if the connection is already closing.
    pub fn start_close(mut self) {
        // Clone the sender so that we are guaranteed to have
        // capacity for the close command (every sender gets a slot).
        match self
            .entry
            .get_mut()
            .sender
            .clone()
            .try_send(task::Command::Close)
        {
            Ok(()) => {}
            Err(e) => assert!(e.is_disconnected(), "No capacity for close command."),
        };
    }
}

/// An iterator over established connections in a pool.
pub struct EstablishedConnectionIter<'a, I, TInEvent> {
    connections: Option<&'a mut FnvHashMap<ConnectionId, EstablishedConnectionInfo<TInEvent>>>,
    ids: I,
}

// Note: Ideally this would be an implementation of `Iterator`, but that
// requires GATs (cf. https://github.com/rust-lang/rust/issues/44265) and
// a different definition of `Iterator`.
impl<'a, I, TInEvent> EstablishedConnectionIter<'a, I, TInEvent>
where
    I: Iterator<Item = ConnectionId>,
{
    /// Obtains the next connection, if any.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<EstablishedConnection<'_, TInEvent>> {
        if let (Some(id), Some(connections)) = (self.ids.next(), self.connections.as_mut()) {
            match connections.entry(id) {
                hash_map::Entry::Occupied(entry) => Some(EstablishedConnection { entry }),
                hash_map::Entry::Vacant(_) => unreachable!("Established entry not found in pool."),
            }
        } else {
            None
        }
    }

    /// Turns the iterator into an iterator over just the connection IDs.
    pub fn into_ids(self) -> impl Iterator<Item = ConnectionId> {
        self.ids
    }

    /// Returns the first connection, if any, consuming the iterator.
    pub fn into_first<'b>(mut self) -> Option<EstablishedConnection<'b, TInEvent>>
    where
        'a: 'b,
    {
        if let (Some(id), Some(connections)) = (self.ids.next(), self.connections) {
            match connections.entry(id) {
                hash_map::Entry::Occupied(entry) => Some(EstablishedConnection { entry }),
                hash_map::Entry::Vacant(_) => unreachable!("Established entry not found in pool."),
            }
        } else {
            None
        }
    }
}

/// Network connection information.
#[derive(Debug, Clone)]
pub struct ConnectionCounters {
    /// The effective connection limits.
    limits: ConnectionLimits,
    /// The current number of incoming connections.
    pending_incoming: u32,
    /// The current number of outgoing connections.
    pending_outgoing: u32,
    /// The current number of established inbound connections.
    established_incoming: u32,
    /// The current number of established outbound connections.
    established_outgoing: u32,
}

impl ConnectionCounters {
    fn new(limits: ConnectionLimits) -> Self {
        Self {
            limits,
            pending_incoming: 0,
            pending_outgoing: 0,
            established_incoming: 0,
            established_outgoing: 0,
        }
    }

    /// The effective connection limits.
    pub fn limits(&self) -> &ConnectionLimits {
        &self.limits
    }

    /// The total number of connections, both pending and established.
    pub fn num_connections(&self) -> u32 {
        self.num_pending() + self.num_established()
    }

    /// The total number of pending connections, both incoming and outgoing.
    pub fn num_pending(&self) -> u32 {
        self.pending_incoming + self.pending_outgoing
    }

    /// The number of incoming connections being established.
    pub fn num_pending_incoming(&self) -> u32 {
        self.pending_incoming
    }

    /// The number of outgoing connections being established.
    pub fn num_pending_outgoing(&self) -> u32 {
        self.pending_outgoing
    }

    /// The number of established incoming connections.
    pub fn num_established_incoming(&self) -> u32 {
        self.established_incoming
    }

    /// The number of established outgoing connections.
    pub fn num_established_outgoing(&self) -> u32 {
        self.established_outgoing
    }

    /// The total number of established connections.
    pub fn num_established(&self) -> u32 {
        self.established_outgoing + self.established_incoming
    }

    fn inc_pending(&mut self, endpoint: &PendingPoint) {
        match endpoint {
            PendingPoint::Dialer => {
                self.pending_outgoing += 1;
            }
            PendingPoint::Listener { .. } => {
                self.pending_incoming += 1;
            }
        }
    }

    fn inc_pending_incoming(&mut self) {
        self.pending_incoming += 1;
    }

    fn dec_pending(&mut self, endpoint: &PendingPoint) {
        match endpoint {
            PendingPoint::Dialer => {
                self.pending_outgoing -= 1;
            }
            PendingPoint::Listener { .. } => {
                self.pending_incoming -= 1;
            }
        }
    }

    fn inc_established(&mut self, endpoint: &ConnectedPoint) {
        match endpoint {
            ConnectedPoint::Dialer { .. } => {
                self.established_outgoing += 1;
            }
            ConnectedPoint::Listener { .. } => {
                self.established_incoming += 1;
            }
        }
    }

    fn dec_established(&mut self, endpoint: &ConnectedPoint) {
        match endpoint {
            ConnectedPoint::Dialer { .. } => {
                self.established_outgoing -= 1;
            }
            ConnectedPoint::Listener { .. } => {
                self.established_incoming -= 1;
            }
        }
    }

    fn check_max_pending_outgoing(&self) -> Result<(), ConnectionLimit> {
        Self::check(self.pending_outgoing, self.limits.max_pending_outgoing)
    }

    fn check_max_pending_incoming(&self) -> Result<(), ConnectionLimit> {
        Self::check(self.pending_incoming, self.limits.max_pending_incoming)
    }

    fn check_max_established(&self, endpoint: &ConnectedPoint) -> Result<(), ConnectionLimit> {
        // Check total connection limit.
        Self::check(self.num_established(), self.limits.max_established_total)?;
        // Check incoming/outgoing connection limits
        match endpoint {
            ConnectedPoint::Dialer { .. } => Self::check(
                self.established_outgoing,
                self.limits.max_established_outgoing,
            ),
            ConnectedPoint::Listener { .. } => Self::check(
                self.established_incoming,
                self.limits.max_established_incoming,
            ),
        }
    }

    fn check_max_established_per_peer(&self, current: u32) -> Result<(), ConnectionLimit> {
        Self::check(current, self.limits.max_established_per_peer)
    }

    fn check(current: u32, limit: Option<u32>) -> Result<(), ConnectionLimit> {
        if let Some(limit) = limit {
            if current >= limit {
                return Err(ConnectionLimit { limit, current });
            }
        }
        Ok(())
    }
}

/// Counts the number of established connections to the given peer.
fn num_peer_established<TInEvent>(
    established: &FnvHashMap<PeerId, FnvHashMap<ConnectionId, EstablishedConnectionInfo<TInEvent>>>,
    peer: PeerId,
) -> u32 {
    established.get(&peer).map_or(0, |conns| {
        u32::try_from(conns.len()).expect("Unexpectedly large number of connections for a peer.")
    })
}

/// The configurable connection limits.
///
/// By default no connection limits apply.
#[derive(Debug, Clone, Default)]
pub struct ConnectionLimits {
    max_pending_incoming: Option<u32>,
    max_pending_outgoing: Option<u32>,
    max_established_incoming: Option<u32>,
    max_established_outgoing: Option<u32>,
    max_established_per_peer: Option<u32>,
    max_established_total: Option<u32>,
}

impl ConnectionLimits {
    /// Configures the maximum number of concurrently incoming connections being established.
    pub fn with_max_pending_incoming(mut self, limit: Option<u32>) -> Self {
        self.max_pending_incoming = limit;
        self
    }

    /// Configures the maximum number of concurrently outgoing connections being established.
    pub fn with_max_pending_outgoing(mut self, limit: Option<u32>) -> Self {
        self.max_pending_outgoing = limit;
        self
    }

    /// Configures the maximum number of concurrent established inbound connections.
    pub fn with_max_established_incoming(mut self, limit: Option<u32>) -> Self {
        self.max_established_incoming = limit;
        self
    }

    /// Configures the maximum number of concurrent established outbound connections.
    pub fn with_max_established_outgoing(mut self, limit: Option<u32>) -> Self {
        self.max_established_outgoing = limit;
        self
    }

    /// Configures the maximum number of concurrent established connections (both
    /// inbound and outbound).
    ///
    /// Note: This should be used in conjunction with
    /// [`ConnectionLimits::with_max_established_incoming`] to prevent possible
    /// eclipse attacks (all connections being inbound).
    pub fn with_max_established(mut self, limit: Option<u32>) -> Self {
        self.max_established_total = limit;
        self
    }

    /// Configures the maximum number of concurrent established connections per peer,
    /// regardless of direction (incoming or outgoing).
    pub fn with_max_established_per_peer(mut self, limit: Option<u32>) -> Self {
        self.max_established_per_peer = limit;
        self
    }
}

/// Connection identifier.
// TODO: Should this live in connection.rs?
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId(usize);

impl ConnectionId {
    /// Creates a `ConnectionId` from a non-negative integer.
    ///
    /// This is primarily useful for creating connection IDs
    /// in test environments. There is in general no guarantee
    /// that all connection IDs are based on non-negative integers.
    pub fn new(id: usize) -> Self {
        ConnectionId(id)
    }
}

/// Configuration options when creating a [`Pool`].
///
/// The default configuration specifies no dedicated task executor, a
/// task event buffer size of 32, and a task command buffer size of 7.
pub struct PoolConfig {
    /// Executor to use to spawn tasks.
    pub executor: Option<Box<dyn Executor + Send>>,

    /// Size of the task command buffer (per task).
    pub task_command_buffer_size: usize,

    /// Size of the pending connection task event buffer and the established connection task event
    /// buffer.
    pub task_event_buffer_size: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        PoolConfig {
            executor: None,
            task_event_buffer_size: 32,
            task_command_buffer_size: 7,
        }
    }
}
