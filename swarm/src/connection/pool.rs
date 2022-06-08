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
    behaviour::{THandlerInEvent, THandlerOutEvent},
    connection::{
        Connected, ConnectionError, ConnectionLimit, IncomingInfo, PendingConnectionError,
        PendingInboundConnectionError, PendingOutboundConnectionError,
    },
    transport::{Transport, TransportError},
    ConnectedPoint, ConnectionHandler, Executor, IntoConnectionHandler, Multiaddr, PeerId,
};
use concurrent_dial::ConcurrentDial;
use fnv::FnvHashMap;
use futures::prelude::*;
use futures::{
    channel::{mpsc, oneshot},
    future::{poll_fn, BoxFuture, Either},
    ready,
    stream::FuturesUnordered,
};
use libp2p_core::connection::{ConnectionId, Endpoint, PendingPoint};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerBox};
use std::{
    collections::{hash_map, HashMap},
    convert::TryFrom as _,
    fmt,
    num::{NonZeroU8, NonZeroUsize},
    pin::Pin,
    task::Context,
    task::Poll,
};
use void::Void;

mod concurrent_dial;
mod task;

/// A connection `Pool` manages a set of connections for each peer.
pub struct Pool<THandler: IntoConnectionHandler, TTrans>
where
    TTrans: Transport,
{
    local_id: PeerId,

    /// The connection counter(s).
    counters: ConnectionCounters,

    /// The managed connections of each peer that are currently considered established.
    established: FnvHashMap<
        PeerId,
        FnvHashMap<
            ConnectionId,
            EstablishedConnectionInfo<<THandler::Handler as ConnectionHandler>::InEvent>,
        >,
    >,

    /// The pending connections that are currently being negotiated.
    pending: HashMap<ConnectionId, PendingConnectionInfo<THandler>>,

    /// Next available identifier for a new connection / task.
    next_connection_id: ConnectionId,

    /// Size of the task command buffer (per task).
    task_command_buffer_size: usize,

    /// Number of addresses concurrently dialed for a single outbound connection attempt.
    dial_concurrency_factor: NonZeroU8,

    /// The configured override for substream protocol upgrades, if any.
    substream_upgrade_protocol_override: Option<libp2p_core::upgrade::Version>,

    /// The maximum number of inbound streams concurrently negotiating on a connection.
    ///
    /// See [`super::handler_wrapper::HandlerWrapper::max_negotiating_inbound_streams`].
    max_negotiating_inbound_streams: usize,

    /// The executor to use for running the background tasks. If `None`,
    /// the tasks are kept in `local_spawns` instead and polled on the
    /// current thread when the [`Pool`] is polled for new events.
    executor: Option<Box<dyn Executor + Send>>,

    /// If no `executor` is configured, tasks are kept in this set and
    /// polled on the current thread when the [`Pool`] is polled for new events.
    local_spawns: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Send>>>,

    /// Sender distributed to pending tasks for reporting events back
    /// to the pool.
    pending_connection_events_tx: mpsc::Sender<task::PendingConnectionEvent<TTrans>>,

    /// Receiver for events reported from pending tasks.
    pending_connection_events_rx: mpsc::Receiver<task::PendingConnectionEvent<TTrans>>,

    /// Sender distributed to established tasks for reporting events back
    /// to the pool.
    established_connection_events_tx:
        mpsc::Sender<task::EstablishedConnectionEvent<THandler::Handler>>,

    /// Receiver for events reported from established tasks.
    established_connection_events_rx:
        mpsc::Receiver<task::EstablishedConnectionEvent<THandler::Handler>>,
}

#[derive(Debug)]
struct EstablishedConnectionInfo<TInEvent> {
    /// [`PeerId`] of the remote peer.
    peer_id: PeerId,
    endpoint: ConnectedPoint,
    /// Channel endpoint to send commands to the task.
    sender: mpsc::Sender<task::Command<TInEvent>>,
}

impl<TInEvent> EstablishedConnectionInfo<TInEvent> {
    /// Initiates a graceful close of the connection.
    ///
    /// Has no effect if the connection is already closing.
    pub fn start_close(&mut self) {
        // Clone the sender so that we are guaranteed to have
        // capacity for the close command (every sender gets a slot).
        match self.sender.clone().try_send(task::Command::Close) {
            Ok(()) => {}
            Err(e) => assert!(e.is_disconnected(), "No capacity for close command."),
        };
    }
}

struct PendingConnectionInfo<THandler> {
    /// [`PeerId`] of the remote peer.
    peer_id: Option<PeerId>,
    /// Handler to handle connection once no longer pending but established.
    handler: THandler,
    endpoint: PendingPoint,
    /// When dropped, notifies the task which then knows to terminate.
    abort_notifier: Option<oneshot::Sender<Void>>,
}

impl<THandler: IntoConnectionHandler, TTrans: Transport> fmt::Debug for Pool<THandler, TTrans> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("Pool")
            .field("counters", &self.counters)
            .finish()
    }
}

/// Event that can happen on the `Pool`.
#[derive(Debug)]
pub enum PoolEvent<THandler: IntoConnectionHandler, TTrans>
where
    TTrans: Transport,
{
    /// A new connection has been established.
    ConnectionEstablished {
        id: ConnectionId,
        peer_id: PeerId,
        endpoint: ConnectedPoint,
        /// List of other connections to the same peer.
        ///
        /// Note: Does not include the connection reported through this event.
        other_established_connection_ids: Vec<ConnectionId>,
        /// [`Some`] when the new connection is an outgoing connection.
        /// Addresses are dialed in parallel. Contains the addresses and errors
        /// of dial attempts that failed before the one successful dial.
        concurrent_dial_errors: Option<Vec<(Multiaddr, TransportError<TTrans::Error>)>>,
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
        error: Option<ConnectionError<<THandler::Handler as ConnectionHandler>::Error>>,
        /// The remaining established connections to the same peer.
        remaining_established_connection_ids: Vec<ConnectionId>,
        handler: THandler::Handler,
    },

    /// An outbound connection attempt failed.
    PendingOutboundConnectionError {
        /// The ID of the failed connection.
        id: ConnectionId,
        /// The error that occurred.
        error: PendingOutboundConnectionError<TTrans::Error>,
        /// The handler that was supposed to handle the connection.
        handler: THandler,
        /// The (expected) peer of the failed connection.
        peer: Option<PeerId>,
    },

    /// An inbound connection attempt failed.
    PendingInboundConnectionError {
        /// The ID of the failed connection.
        id: ConnectionId,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
        /// Local connection address.
        local_addr: Multiaddr,
        /// The error that occurred.
        error: PendingInboundConnectionError<TTrans::Error>,
        /// The handler that was supposed to handle the connection.
        handler: THandler,
    },

    /// A node has produced an event.
    ConnectionEvent {
        id: ConnectionId,
        peer_id: PeerId,
        /// The produced event.
        event: THandlerOutEvent<THandler>,
    },

    /// The connection to a node has changed its address.
    AddressChange {
        id: ConnectionId,
        peer_id: PeerId,
        /// The new endpoint.
        new_endpoint: ConnectedPoint,
        /// The old endpoint.
        old_endpoint: ConnectedPoint,
    },
}

impl<THandler, TTrans> Pool<THandler, TTrans>
where
    THandler: IntoConnectionHandler,
    TTrans: Transport,
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
            next_connection_id: ConnectionId::new(0),
            task_command_buffer_size: config.task_command_buffer_size,
            dial_concurrency_factor: config.dial_concurrency_factor,
            substream_upgrade_protocol_override: config.substream_upgrade_protocol_override,
            max_negotiating_inbound_streams: config.max_negotiating_inbound_streams,
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

    /// Gets an entry representing a connection in the pool.
    ///
    /// Returns `None` if the pool has no connection with the given ID.
    pub fn get(&mut self, id: ConnectionId) -> Option<PoolConnection<'_, THandler>> {
        if let hash_map::Entry::Occupied(entry) = self.pending.entry(id) {
            Some(PoolConnection::Pending(PendingConnection { entry }))
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

    /// Returns true if we are connected to the given peer.
    ///
    /// This will return true only after a `NodeReached` event has been produced by `poll()`.
    pub fn is_connected(&self, id: PeerId) -> bool {
        self.established.contains_key(&id)
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
    pub fn disconnect(&mut self, peer: PeerId) {
        if let Some(conns) = self.established.get_mut(&peer) {
            for (_, conn) in conns.iter_mut() {
                conn.start_close();
            }
        }

        #[allow(clippy::needless_collect)]
        let pending_connections = self
            .pending
            .iter()
            .filter(|(_, PendingConnectionInfo { peer_id, .. })| peer_id.as_ref() == Some(&peer))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();

        for pending_connection in pending_connections {
            let entry = self
                .pending
                .entry(pending_connection)
                .expect_occupied("Iterating pending connections");

            PendingConnection { entry }.abort();
        }
    }

    /// Returns an iterator over all established connections of `peer`.
    pub fn iter_established_connections_of_peer(
        &mut self,
        peer: &PeerId,
    ) -> impl Iterator<Item = ConnectionId> + '_ {
        match self.established.get(peer) {
            Some(conns) => either::Either::Left(conns.iter().map(|(id, _)| *id)),
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
        self.next_connection_id = self.next_connection_id + 1;

        connection_id
    }

    fn spawn(&mut self, task: BoxFuture<'static, ()>) {
        if let Some(executor) = &mut self.executor {
            executor.exec(task);
        } else {
            self.local_spawns.push(task);
        }
    }
}

impl<THandler, TTrans> Pool<THandler, TTrans>
where
    THandler: IntoConnectionHandler,
    TTrans: Transport + 'static,
    TTrans::Output: Send + 'static,
    TTrans::Error: Send + 'static,
{
    /// Adds a pending outgoing connection to the pool in the form of a `Future`
    /// that establishes and negotiates the connection.
    ///
    /// Returns an error if the limit of pending outgoing connections
    /// has been reached.
    pub fn add_outgoing(
        &mut self,
        dials: Vec<
            BoxFuture<
                'static,
                (
                    Multiaddr,
                    Result<
                        <TTrans as Transport>::Output,
                        TransportError<<TTrans as Transport>::Error>,
                    >,
                ),
            >,
        >,
        peer: Option<PeerId>,
        handler: THandler,
        role_override: Endpoint,
        dial_concurrency_factor_override: Option<NonZeroU8>,
    ) -> Result<ConnectionId, (ConnectionLimit, THandler)>
    where
        TTrans: Send,
        TTrans::Dial: Send + 'static,
    {
        if let Err(limit) = self.counters.check_max_pending_outgoing() {
            return Err((limit, handler));
        };

        let dial = ConcurrentDial::new(
            dials,
            dial_concurrency_factor_override.unwrap_or(self.dial_concurrency_factor),
        );

        let connection_id = self.next_connection_id();

        let (abort_notifier, abort_receiver) = oneshot::channel();

        self.spawn(
            task::new_for_pending_outgoing_connection(
                connection_id,
                dial,
                abort_receiver,
                self.pending_connection_events_tx.clone(),
            )
            .boxed(),
        );

        let endpoint = PendingPoint::Dialer { role_override };

        self.counters.inc_pending(&endpoint);
        self.pending.insert(
            connection_id,
            PendingConnectionInfo {
                peer_id: peer,
                handler,
                endpoint,
                abort_notifier: Some(abort_notifier),
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
    ) -> Result<ConnectionId, (ConnectionLimit, THandler)>
    where
        TFut: Future<Output = Result<TTrans::Output, TTrans::Error>> + Send + 'static,
    {
        let endpoint = info.create_connected_point();

        if let Err(limit) = self.counters.check_max_pending_incoming() {
            return Err((limit, handler));
        }

        let connection_id = self.next_connection_id();

        let (abort_notifier, abort_receiver) = oneshot::channel();

        self.spawn(
            task::new_for_pending_incoming_connection(
                connection_id,
                future,
                abort_receiver,
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
                abort_notifier: Some(abort_notifier),
            },
        );
        Ok(connection_id)
    }

    /// Polls the connection pool for events.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<PoolEvent<THandler, TTrans>>
    where
        TTrans: Transport<Output = (PeerId, StreamMuxerBox)>,
        THandler: IntoConnectionHandler + 'static,
        THandler::Handler: ConnectionHandler + Send,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send,
    {
        // Poll for events of established connections.
        //
        // Note that established connections are polled before pending connections, thus
        // prioritizing established connections over pending connections.
        match self.established_connection_events_rx.poll_next_unpin(cx) {
            Poll::Pending => {}
            Poll::Ready(None) => unreachable!("Pool holds both sender and receiver."),

            Poll::Ready(Some(task::EstablishedConnectionEvent::Notify { id, peer_id, event })) => {
                return Poll::Ready(PoolEvent::ConnectionEvent { peer_id, id, event });
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

                return Poll::Ready(PoolEvent::AddressChange {
                    peer_id,
                    id,
                    new_endpoint,
                    old_endpoint,
                });
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
                let remaining_established_connection_ids: Vec<ConnectionId> =
                    connections.keys().cloned().collect();
                if remaining_established_connection_ids.is_empty() {
                    self.established.remove(&peer_id);
                }
                return Poll::Ready(PoolEvent::ConnectionClosed {
                    id,
                    connected: Connected { endpoint, peer_id },
                    error,
                    remaining_established_connection_ids,
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
                    output: (obtained_peer_id, muxer),
                    outgoing,
                } => {
                    let PendingConnectionInfo {
                        peer_id: expected_peer_id,
                        handler,
                        endpoint,
                        abort_notifier: _,
                    } = self
                        .pending
                        .remove(&id)
                        .expect("Entry in `self.pending` for previously pending connection.");

                    self.counters.dec_pending(&endpoint);

                    let (endpoint, concurrent_dial_errors) = match (endpoint, outgoing) {
                        (PendingPoint::Dialer { role_override }, Some((address, errors))) => (
                            ConnectedPoint::Dialer {
                                address,
                                role_override,
                            },
                            Some(errors),
                        ),
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
                        (PendingPoint::Dialer { .. }, None) => unreachable!(
                            "Established incoming connection via pending outgoing connection."
                        ),
                        (PendingPoint::Listener { .. }, Some(_)) => unreachable!(
                            "Established outgoing connection via pending incoming connection."
                        ),
                    };

                    let error: Result<(), PendingInboundConnectionError<_>> = self
                        .counters
                        // Check general established connection limit.
                        .check_max_established(&endpoint)
                        .map_err(PendingConnectionError::ConnectionLimit)
                        // Check per-peer established connection limit.
                        .and_then(|()| {
                            self.counters
                                .check_max_established_per_peer(num_peer_established(
                                    &self.established,
                                    obtained_peer_id,
                                ))
                                .map_err(PendingConnectionError::ConnectionLimit)
                        })
                        // Check expected peer id matches.
                        .and_then(|()| {
                            if let Some(peer) = expected_peer_id {
                                if peer != obtained_peer_id {
                                    Err(PendingConnectionError::WrongPeerId {
                                        obtained: obtained_peer_id,
                                        endpoint: endpoint.clone(),
                                    })
                                } else {
                                    Ok(())
                                }
                            } else {
                                Ok(())
                            }
                        })
                        // Check peer is not local peer.
                        .and_then(|()| {
                            if self.local_id == obtained_peer_id {
                                Err(PendingConnectionError::WrongPeerId {
                                    obtained: obtained_peer_id,
                                    endpoint: endpoint.clone(),
                                })
                            } else {
                                Ok(())
                            }
                        });

                    if let Err(error) = error {
                        self.spawn(
                            poll_fn(move |cx| {
                                if let Err(e) = ready!(muxer.poll_close(cx)) {
                                    log::debug!(
                                        "Failed to close connection {:?} to peer {}: {:?}",
                                        id,
                                        obtained_peer_id,
                                        e
                                    );
                                }
                                Poll::Ready(())
                            })
                            .boxed(),
                        );

                        match endpoint {
                            ConnectedPoint::Dialer { .. } => {
                                return Poll::Ready(PoolEvent::PendingOutboundConnectionError {
                                    id,
                                    error: error
                                        .map(|t| vec![(endpoint.get_remote_address().clone(), t)]),
                                    handler,
                                    peer: expected_peer_id.or(Some(obtained_peer_id)),
                                })
                            }
                            ConnectedPoint::Listener {
                                send_back_addr,
                                local_addr,
                            } => {
                                return Poll::Ready(PoolEvent::PendingInboundConnectionError {
                                    id,
                                    error,
                                    handler,
                                    send_back_addr,
                                    local_addr,
                                })
                            }
                        };
                    }

                    // Add the connection to the pool.
                    let conns = self.established.entry(obtained_peer_id).or_default();
                    let other_established_connection_ids = conns.keys().cloned().collect();
                    self.counters.inc_established(&endpoint);

                    let (command_sender, command_receiver) =
                        mpsc::channel(self.task_command_buffer_size);
                    conns.insert(
                        id,
                        EstablishedConnectionInfo {
                            peer_id: obtained_peer_id,
                            endpoint: endpoint.clone(),
                            sender: command_sender,
                        },
                    );

                    let connection = super::Connection::new(
                        muxer,
                        handler.into_handler(&obtained_peer_id, &endpoint),
                        self.substream_upgrade_protocol_override,
                        self.max_negotiating_inbound_streams,
                    );
                    self.spawn(
                        task::new_for_established_connection(
                            id,
                            obtained_peer_id,
                            connection,
                            command_receiver,
                            self.established_connection_events_tx.clone(),
                        )
                        .boxed(),
                    );

                    match self.get(id) {
                        Some(PoolConnection::Established(connection)) => {
                            return Poll::Ready(PoolEvent::ConnectionEstablished {
                                peer_id: connection.peer_id(),
                                endpoint: connection.endpoint().clone(),
                                id: connection.id(),
                                other_established_connection_ids,
                                concurrent_dial_errors,
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
                        abort_notifier: _,
                    }) = self.pending.remove(&id)
                    {
                        self.counters.dec_pending(&endpoint);

                        match (endpoint, error) {
                            (PendingPoint::Dialer { .. }, Either::Left(error)) => {
                                return Poll::Ready(PoolEvent::PendingOutboundConnectionError {
                                    id,
                                    error,
                                    handler,
                                    peer: peer_id,
                                });
                            }
                            (
                                PendingPoint::Listener {
                                    send_back_addr,
                                    local_addr,
                                },
                                Either::Right(error),
                            ) => {
                                return Poll::Ready(PoolEvent::PendingInboundConnectionError {
                                    id,
                                    error,
                                    handler,
                                    send_back_addr,
                                    local_addr,
                                });
                            }
                            (PendingPoint::Dialer { .. }, Either::Right(_)) => {
                                unreachable!("Inbound error for outbound connection.")
                            }
                            (PendingPoint::Listener { .. }, Either::Left(_)) => {
                                unreachable!("Outbound error for inbound connection.")
                            }
                        }
                    }
                }
            }
        }

        // Advance the tasks in `local_spawns`.
        while let Poll::Ready(Some(())) = self.local_spawns.poll_next_unpin(cx) {}

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
}

impl<THandler: IntoConnectionHandler> PendingConnection<'_, THandler> {
    /// Aborts the connection attempt, closing the connection.
    pub fn abort(mut self) {
        if let Some(notifier) = self.entry.get_mut().abort_notifier.take() {
            drop(notifier);
        }
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
        self.entry.get_mut().start_close()
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
            PendingPoint::Dialer { .. } => {
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
            PendingPoint::Dialer { .. } => {
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

    /// Number of addresses concurrently dialed for a single outbound connection attempt.
    pub dial_concurrency_factor: NonZeroU8,

    /// The configured override for substream protocol upgrades, if any.
    substream_upgrade_protocol_override: Option<libp2p_core::upgrade::Version>,

    /// The maximum number of inbound streams concurrently negotiating on a connection.
    ///
    /// See [super::handler_wrapper::HandlerWrapper::max_negotiating_inbound_streams].
    max_negotiating_inbound_streams: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        PoolConfig {
            executor: None,
            task_event_buffer_size: 32,
            task_command_buffer_size: 7,
            // By default, addresses of a single connection attempt are dialed in sequence.
            dial_concurrency_factor: NonZeroU8::new(1).expect("1 > 0"),
            substream_upgrade_protocol_override: None,
            max_negotiating_inbound_streams: 128,
        }
    }
}

impl PoolConfig {
    /// Configures the executor to use for spawning connection background tasks.
    pub fn with_executor(mut self, e: Box<dyn Executor + Send>) -> Self {
        self.executor = Some(e);
        self
    }

    /// Configures the executor to use for spawning connection background tasks,
    /// only if no executor has already been configured.
    pub fn or_else_with_executor<F>(mut self, f: F) -> Self
    where
        F: FnOnce() -> Option<Box<dyn Executor + Send>>,
    {
        self.executor = self.executor.or_else(f);
        self
    }

    /// Sets the maximum number of events sent to a connection's background task
    /// that may be buffered, if the task cannot keep up with their consumption and
    /// delivery to the connection handler.
    ///
    /// When the buffer for a particular connection is full, `notify_handler` will no
    /// longer be able to deliver events to the associated [`Connection`](super::Connection),
    /// thus exerting back-pressure on the connection and peer API.
    pub fn with_notify_handler_buffer_size(mut self, n: NonZeroUsize) -> Self {
        self.task_command_buffer_size = n.get() - 1;
        self
    }

    /// Sets the maximum number of buffered connection events (beyond a guaranteed
    /// buffer of 1 event per connection).
    ///
    /// When the buffer is full, the background tasks of all connections will stall.
    /// In this way, the consumers of network events exert back-pressure on
    /// the network connection I/O.
    pub fn with_connection_event_buffer_size(mut self, n: usize) -> Self {
        self.task_event_buffer_size = n;
        self
    }

    /// Number of addresses concurrently dialed for a single outbound connection attempt.
    pub fn with_dial_concurrency_factor(mut self, factor: NonZeroU8) -> Self {
        self.dial_concurrency_factor = factor;
        self
    }

    /// Configures an override for the substream upgrade protocol to use.
    pub fn with_substream_upgrade_protocol_override(
        mut self,
        v: libp2p_core::upgrade::Version,
    ) -> Self {
        self.substream_upgrade_protocol_override = Some(v);
        self
    }

    /// The maximum number of inbound streams concurrently negotiating on a connection.
    ///
    /// See [`super::handler_wrapper::HandlerWrapper::max_negotiating_inbound_streams`].
    pub fn with_max_negotiating_inbound_streams(mut self, v: usize) -> Self {
        self.max_negotiating_inbound_streams = v;
        self
    }
}

trait EntryExt<'a, K, V> {
    fn expect_occupied(self, msg: &'static str) -> hash_map::OccupiedEntry<'a, K, V>;
}

impl<'a, K: 'a, V: 'a> EntryExt<'a, K, V> for hash_map::Entry<'a, K, V> {
    fn expect_occupied(self, msg: &'static str) -> hash_map::OccupiedEntry<'a, K, V> {
        match self {
            hash_map::Entry::Occupied(entry) => entry,
            hash_map::Entry::Vacant(_) => panic!("{}", msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::Future;

    struct Dummy;

    impl Executor for Dummy {
        fn exec(&self, _: Pin<Box<dyn Future<Output = ()> + Send>>) {}
    }

    #[test]
    fn set_executor() {
        PoolConfig::default()
            .with_executor(Box::new(Dummy))
            .with_executor(Box::new(|f| {
                async_std::task::spawn(f);
            }));
    }
}
