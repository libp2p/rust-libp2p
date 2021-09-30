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
        manager::{self, Manager, ManagerConfig},
        Connected, ConnectionError, ConnectionHandler, ConnectionId, ConnectionLimit, Endpoint,
        IncomingInfo, IntoConnectionHandler, OutgoingInfo, PendingConnectionError, PendingPoint,
        Substream,
    },
    muxing::StreamMuxer,
    network::DialError,
    transport::TransportError,
    ConnectedPoint, Multiaddr, PeerId,
};
use either::Either;
use fnv::FnvHashMap;
use futures::prelude::*;
use smallvec::SmallVec;
use std::{
    collections::HashMap, convert::TryFrom as _, error, fmt, num::NonZeroU32, task::Context,
    task::Poll,
};

/// A connection `Pool` manages a set of connections for each peer.
pub struct Pool<THandler: IntoConnectionHandler, TMuxer, TTransErr> {
    local_id: PeerId,

    /// The connection counter(s).
    counters: ConnectionCounters,

    /// The connection manager that handles the connection I/O for both
    /// established and pending connections.
    ///
    /// For every established connection there is a corresponding entry in `established`.
    manager: Manager<THandler, TMuxer, TTransErr>,

    /// The managed connections of each peer that are currently considered
    /// established, as witnessed  the associated `ConnectedPoint`.
    established: FnvHashMap<PeerId, FnvHashMap<ConnectionId, ConnectedPoint>>,

    /// The pending connections that are currently being negotiated.
    pending: HashMap<ConnectionId, PendingConnectionInfo<THandler>>,
}

struct PendingConnectionInfo<THandler> {
    peer_id: Option<PeerId>,
    handler: THandler,
    endpoint: PendingPoint,
}

impl<THandler: IntoConnectionHandler, TMuxer, TTransErr> fmt::Debug
    for Pool<THandler, TMuxer, TTransErr>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("Pool")
            .field("counters", &self.counters)
            .finish()
    }
}

impl<THandler: IntoConnectionHandler, TMuxer, TTransErr> Unpin
    for Pool<THandler, TMuxer, TTransErr>
{
}

/// Event that can happen on the `Pool`.
pub enum PoolEvent<'a, THandler: IntoConnectionHandler, TMuxer, TTransErr> {
    /// A new outgoing connection has been established.
    OutgoingConnectionEstablished {
        connection: EstablishedConnection<'a, THandlerInEvent<THandler>>,
        num_established: NonZeroU32,
        errors: Vec<(Multiaddr, TransportError<TTransErr>)>,
    },

    /// A new incoming connection has been established.
    IncomingConnectionEstablished {
        connection: EstablishedConnection<'a, THandlerInEvent<THandler>>,
        num_established: NonZeroU32,
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
        /// A reference to the pool that managed the connection.
        pool: &'a mut Pool<THandler, TMuxer, TTransErr>,
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
            PoolEvent::OutgoingConnectionEstablished { ref connection, .. } => f
                .debug_tuple("PoolEvent::OutgoingConnectionEstablished")
                .field(connection)
                .finish(),
            PoolEvent::IncomingConnectionEstablished { ref connection, .. } => f
                .debug_tuple("PoolEvent::IncomingConnectionEstablished")
                .field(connection)
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
    pub fn new(local_id: PeerId, manager_config: ManagerConfig, limits: ConnectionLimits) -> Self {
        Pool {
            local_id,
            counters: ConnectionCounters::new(limits),
            manager: Manager::new(manager_config),
            established: Default::default(),
            pending: Default::default(),
        }
    }

    /// Gets the dedicated connection counters.
    pub fn counters(&self) -> &ConnectionCounters {
        &self.counters
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

        let id = self.manager.add_pending_incoming(future);
        self.counters.inc_pending_incoming();
        self.pending.insert(
            id,
            PendingConnectionInfo {
                peer_id: None,
                handler,
                endpoint: endpoint.into(),
            },
        );
        Ok(id)
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

        //         // Validate the received peer ID as the last step of the pending connection
        //         // future, so that these errors can be raised before the `handler` is consumed
        //         // by the background task, which happens when this future resolves to an
        //         // "established" connection.
        //         let future = future.and_then({
        //             let expected_peer = peer;
        //             let local_id = self.local_id;
        //             move |(peer_id, endpoint, muxer)| {
        //                 if let Some(peer) = expected_peer {
        //                     if peer != peer_id {
        //                         return future::err(PendingConnectionError::InvalidPeerId);
        //                     }
        //                 }
        //
        //                 if local_id == peer_id {
        //                     return future::err(PendingConnectionError::InvalidPeerId);
        //                 }
        //
        //                 let connected = Connected { peer_id, endpoint };
        //                 future::ready(Ok((connected, muxer)))
        //             }
        //         });

        let id = self.manager.add_pending_outgoing(dial);
        self.counters.inc_pending(&PendingPoint::Dialer);
        self.pending.insert(
            id,
            PendingConnectionInfo {
                peer_id: expected_peer_id,
                handler,
                endpoint: PendingPoint::Dialer,
            },
        );
        Ok(id)
    }

    /// Gets an entry representing a connection in the pool.
    ///
    /// Returns `None` if the pool has no connection with the given ID.
    pub fn get(&mut self, id: ConnectionId) -> Option<PoolConnection<'_, THandler>> {
        match self.manager.entry(id) {
            Some(manager::Entry::Established(entry)) => {
                Some(PoolConnection::Established(EstablishedConnection { entry }))
            }
            Some(manager::Entry::Pending(entry)) => {
                Some(PoolConnection::Pending(PendingConnection {
                    entry,
                    pending: &mut self.pending,
                    counters: &mut self.counters,
                }))
            }
            None => None,
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
        match self.pending.get(&id) {
            Some(PendingConnectionInfo {
                peer_id,
                endpoint: PendingPoint::Dialer,
                handler: _,
            }) => match self.manager.entry(id) {
                Some(manager::Entry::Pending(entry)) => {
                    let peer_id = *peer_id;
                    Some(PendingConnection {
                        entry,
                        pending: &mut self.pending,
                        counters: &mut self.counters,
                    })
                }
                _ => unreachable!("by consistency of `self.pending` with `self.manager`"),
            },
            _ => None,
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
        if let Some(conns) = self.established.get(peer) {
            for (&id, _endpoint) in conns.iter() {
                if let Some(manager::Entry::Established(e)) = self.manager.entry(id) {
                    e.start_close();
                }
            }
        }

        todo!()
        // for (&id, (_endpoint, peer2)) in &self.pending {
        //     if Some(peer) == peer2.as_ref() {
        //         if let Some(manager::Entry::Pending(e)) = self.manager.entry(id) {
        //             e.abort();
        //         }
        //     }
        // }
    }

    /// Counts the number of established connections to the given peer.
    pub fn num_peer_established(&self, peer: PeerId) -> u32 {
        num_peer_established(&self.established, peer)
    }

    /// Returns an iterator over all established connections of `peer`.
    pub fn iter_peer_established<'a>(
        &'a mut self,
        peer: &PeerId,
    ) -> EstablishedConnectionIter<
        'a,
        impl Iterator<Item = ConnectionId>,
        THandler,
        TMuxer,
        TTransErr,
    > {
        let ids = self
            .iter_peer_established_info(peer)
            .map(|(id, _endpoint)| *id)
            .collect::<SmallVec<[ConnectionId; 10]>>()
            .into_iter();

        EstablishedConnectionIter { pool: self, ids }
    }

    // /// Returns an iterator for information on all pending incoming connections.
    // pub fn iter_pending_incoming(&self) -> impl Iterator<Item = IncomingInfo<'_>> {
    //     self.iter_pending_info()
    //         .filter_map(|(_, ref endpoint, _)| match endpoint {
    //             ConnectedPoint::Listener {
    //                 local_addr,
    //                 send_back_addr,
    //             } => Some(IncomingInfo {
    //                 local_addr,
    //                 send_back_addr,
    //             }),
    //             ConnectedPoint::Dialer { .. } => None,
    //         })
    // }

    // /// Returns an iterator for information on all pending outgoing connections.
    // pub fn iter_pending_outgoing(&self) -> impl Iterator<Item = OutgoingInfo<'_>> {
    //     self.iter_pending_info()
    //         .filter_map(|(_, ref endpoint, peer_id)| match endpoint {
    //             ConnectedPoint::Listener { .. } => None,
    //             ConnectedPoint::Dialer { address } => Some(OutgoingInfo {
    //                 address,
    //                 peer_id: peer_id.as_ref(),
    //             }),
    //         })
    // }

    /// Returns an iterator over all connection IDs and associated endpoints
    /// of established connections to `peer` known to the pool.
    pub fn iter_peer_established_info(
        &self,
        peer: &PeerId,
    ) -> impl Iterator<Item = (&ConnectionId, &ConnectedPoint)> + fmt::Debug + '_ {
        match self.established.get(peer) {
            Some(conns) => Either::Left(conns.iter()),
            None => Either::Right(std::iter::empty()),
        }
    }

    // /// Returns an iterator over all pending connection IDs together
    // /// with associated endpoints and expected peer IDs in the pool.
    // pub fn iter_pending_info(
    //     &self,
    // ) -> impl Iterator<Item = (&ConnectionId, &PendingPoint, &Option<PeerId>)> + '_ {
    //     self.pending
    //         .iter()
    //         .map(|(id, (endpoint, info))| (id, endpoint, info))
    // }

    /// Returns an iterator over all connected peers, i.e. those that have
    /// at least one established connection in the pool.
    pub fn iter_connected(&self) -> impl Iterator<Item = &PeerId> {
        self.established.keys()
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
        // Poll the connection `Manager`.
        loop {
            let item = match self.manager.poll(cx) {
                Poll::Ready(item) => item,
                Poll::Pending => return Poll::Pending,
            };

            match item {
                manager::Event::PendingConnectionError { id, error } => {
                    let PendingConnectionInfo {
                        peer_id,
                        handler,
                        endpoint,
                    } = self
                        .pending
                        .remove(&id)
                        .expect("Entry in `self.pending` for previously pending connection.");

                    self.counters.dec_pending(&endpoint);
                    return Poll::Ready(PoolEvent::PendingConnectionError {
                        id,
                        endpoint,
                        error,
                        handler,
                        peer: peer_id,
                        pool: self,
                    });
                }
                manager::Event::ConnectionClosed {
                    id,
                    connected,
                    error,
                    handler,
                } => {
                    let num_established =
                        if let Some(conns) = self.established.get_mut(&connected.peer_id) {
                            if let Some(endpoint) = conns.remove(&id) {
                                self.counters.dec_established(&endpoint);
                            }
                            u32::try_from(conns.len()).unwrap()
                        } else {
                            0
                        };
                    if num_established == 0 {
                        self.established.remove(&connected.peer_id);
                    }
                    return Poll::Ready(PoolEvent::ConnectionClosed {
                        id,
                        connected,
                        error,
                        num_established,
                        pool: self,
                        handler,
                    });
                }
                manager::Event::OutgoingConnectionEstablished {
                    id,
                    peer_id,
                    address,
                    muxer,
                    // TODO: Bubble these errors up.
                    errors,
                } => {
                    let PendingConnectionInfo {
                        peer_id: expected_peer_id,
                        handler,
                        endpoint,
                    } = self
                        .pending
                        .remove(&id)
                        .expect("Entry in `self.pending` for previously pending connection.");

                    assert_eq!(endpoint, PendingPoint::Dialer);
                    self.counters.dec_pending(&endpoint);

                    let endpoint = ConnectedPoint::Dialer { address };

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
                        self.manager.add_closing(muxer);
                        return Poll::Ready(PoolEvent::PendingConnectionError {
                            id,
                            endpoint: endpoint.into(),
                            error,
                            handler,
                            peer: Some(peer_id),
                            pool: self,
                        });
                    }

                    // Add the connection to the pool.
                    let conns = self.established.entry(peer_id).or_default();
                    let num_established = NonZeroU32::new(u32::try_from(conns.len() + 1).unwrap())
                        .expect("n + 1 is always non-zero; qed");
                    self.counters.inc_established(&endpoint);
                    conns.insert(id, endpoint.clone());

                    let connected = Connected {
                        endpoint: endpoint.clone(),
                        peer_id,
                    };

                    let connection =
                        super::Connection::new(muxer, handler.into_handler(&connected));
                    self.manager.add_established(id, connection, connected);

                    match self.get(id) {
                        Some(PoolConnection::Established(connection)) => {
                            return Poll::Ready(PoolEvent::OutgoingConnectionEstablished {
                                connection,
                                num_established,
                                errors,
                            })
                        }
                        _ => unreachable!("since `entry` is an `EstablishedEntry`."),
                    }
                }
                manager::Event::IncomingConnectionEstablished { id, peer_id, muxer } => {
                    let PendingConnectionInfo {
                        peer_id: expected_peer_id,
                        handler,
                        endpoint,
                    } = self
                        .pending
                        .remove(&id)
                        .expect("Entry in `self.pending` for previously pending connection.");
                    self.counters.dec_pending(&endpoint);

                    let endpoint = match endpoint {
                        PendingPoint::Listener {
                            local_addr,
                            send_back_addr,
                        } => ConnectedPoint::Listener {
                            local_addr,
                            send_back_addr,
                        },
                        PendingPoint::Dialer => unreachable!(
                            "Established incoming connection on pending outgoing connection."
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
                        self.manager.add_closing(muxer);
                        return Poll::Ready(PoolEvent::PendingConnectionError {
                            id,
                            endpoint: endpoint.into(),
                            error,
                            handler,
                            peer: Some(peer_id),
                            pool: self,
                        });
                    }

                    // Add the connection to the pool.
                    let conns = self.established.entry(peer_id).or_default();
                    let num_established = NonZeroU32::new(u32::try_from(conns.len() + 1).unwrap())
                        .expect("n + 1 is always non-zero; qed");
                    self.counters.inc_established(&endpoint);
                    conns.insert(id, endpoint.clone());

                    let connected = Connected {
                        endpoint: endpoint.clone(),
                        peer_id,
                    };

                    let connection =
                        super::Connection::new(muxer, handler.into_handler(&connected));
                    self.manager.add_established(id, connection, connected);

                    match self.get(id) {
                        Some(PoolConnection::Established(connection)) => {
                            return Poll::Ready(PoolEvent::IncomingConnectionEstablished {
                                connection,
                                num_established,
                            })
                        }
                        _ => unreachable!("since `entry` is an `EstablishedEntry`."),
                    }
                }
                manager::Event::ConnectionEvent { entry, event } => {
                    let id = entry.id();
                    match self.get(id) {
                        Some(PoolConnection::Established(connection)) => {
                            return Poll::Ready(PoolEvent::ConnectionEvent { connection, event })
                        }
                        _ => unreachable!("since `entry` is an `EstablishedEntry`."),
                    }
                }
                manager::Event::AddressChange {
                    entry,
                    new_endpoint,
                    old_endpoint,
                } => {
                    let id = entry.id();

                    match self.established.get_mut(&entry.connected().peer_id) {
                        Some(list) => {
                            *list.get_mut(&id).expect(
                                "state inconsistency: entry is `EstablishedEntry` but absent \
                                from `established`",
                            ) = new_endpoint.clone()
                        }
                        None => unreachable!("since `entry` is an `EstablishedEntry`."),
                    };

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
            }
        }
    }
}

/// A connection in a [`Pool`].
pub enum PoolConnection<'a, THandler: IntoConnectionHandler> {
    Pending(PendingConnection<'a, THandler>),
    Established(EstablishedConnection<'a, THandlerInEvent<THandler>>),
}

/// A pending connection in a pool.
pub struct PendingConnection<'a, THandler: IntoConnectionHandler> {
    entry: manager::PendingEntry<'a, THandlerInEvent<THandler>>,
    pending: &'a mut HashMap<ConnectionId, PendingConnectionInfo<THandler>>,
    counters: &'a mut ConnectionCounters,
}

impl<THandler: IntoConnectionHandler> PendingConnection<'_, THandler> {
    /// Returns the local connection ID.
    pub fn id(&self) -> ConnectionId {
        self.entry.id()
    }

    /// Returns the (expected) identity of the remote peer, if known.
    pub fn peer_id(&self) -> &Option<PeerId> {
        &self
            .pending
            .get(&self.entry.id())
            .expect("`entry` is a pending entry")
            .peer_id
    }

    /// Returns information about this endpoint of the connection.
    pub fn endpoint(&self) -> &PendingPoint {
        &self
            .pending
            .get(&self.entry.id())
            .expect("`entry` is a pending entry")
            .endpoint
    }

    /// Aborts the connection attempt, closing the connection.
    pub fn abort(self) {
        let endpoint = self
            .pending
            .remove(&self.entry.id())
            .expect("`entry` is a pending entry")
            .endpoint;
        self.counters.dec_pending(&endpoint);
        self.entry.abort();
    }
}

/// An established connection in a pool.
pub struct EstablishedConnection<'a, TInEvent> {
    entry: manager::EstablishedEntry<'a, TInEvent>,
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
    pub fn connected(&self) -> &Connected {
        self.entry.connected()
    }

    /// Returns information about the connected endpoint.
    pub fn endpoint(&self) -> &ConnectedPoint {
        &self.entry.connected().endpoint
    }

    /// Returns the identity of the connected peer.
    pub fn peer_id(&self) -> PeerId {
        self.entry.connected().peer_id
    }

    /// Returns the local connection ID.
    pub fn id(&self) -> ConnectionId {
        self.entry.id()
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
        self.entry.notify_handler(event)
    }

    /// Checks if `notify_handler` is ready to accept an event.
    ///
    /// Returns `Ok(())` if the handler is ready to receive an event via `notify_handler`.
    ///
    /// Returns `Err(())` if the background task associated with the connection
    /// is terminating and the connection is about to close.
    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        self.entry.poll_ready_notify_handler(cx)
    }

    /// Initiates a graceful close of the connection.
    ///
    /// Has no effect if the connection is already closing.
    pub fn start_close(self) {
        self.entry.start_close()
    }
}

/// An iterator over established connections in a pool.
pub struct EstablishedConnectionIter<'a, I, THandler: IntoConnectionHandler, TMuxer, TTransErr> {
    pool: &'a mut Pool<THandler, TMuxer, TTransErr>,
    ids: I,
}

// Note: Ideally this would be an implementation of `Iterator`, but that
// requires GATs (cf. https://github.com/rust-lang/rust/issues/44265) and
// a different definition of `Iterator`.
impl<'a, I, THandler: IntoConnectionHandler, TMuxer, TTransErr>
    EstablishedConnectionIter<'a, I, THandler, TMuxer, TTransErr>
where
    I: Iterator<Item = ConnectionId>,
    TTransErr: Send + 'static,
    TMuxer: Send + 'static,
{
    /// Obtains the next connection, if any.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<EstablishedConnection<'_, THandlerInEvent<THandler>>> {
        while let Some(id) = self.ids.next() {
            if self.pool.manager.is_established(&id) {
                // (*)
                match self.pool.manager.entry(id) {
                    Some(manager::Entry::Established(entry)) => {
                        return Some(EstablishedConnection { entry })
                    }
                    _ => panic!("Established entry not found in manager."), // see (*)
                }
            }
        }
        None
    }

    /// Turns the iterator into an iterator over just the connection IDs.
    pub fn into_ids(self) -> impl Iterator<Item = ConnectionId> {
        self.ids
    }

    /// Returns the first connection, if any, consuming the iterator.
    pub fn into_first<'b>(mut self) -> Option<EstablishedConnection<'b, THandlerInEvent<THandler>>>
    where
        'a: 'b,
    {
        while let Some(id) = self.ids.next() {
            if self.pool.manager.is_established(&id) {
                // (*)
                match self.pool.manager.entry(id) {
                    Some(manager::Entry::Established(entry)) => {
                        return Some(EstablishedConnection { entry })
                    }
                    _ => panic!("Established entry not found in manager."), // see (*)
                }
            }
        }
        None
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

    // TODO: Still needed?
    // fn dec_pending_incoming(&mut self) {
    //     self.pending_incoming -= 1;
    // }

    // TODO: Still needed?
    // fn dec_pending_outgoing(&mut self) {
    //     self.pending_outgoing -= 1;
    // }

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
fn num_peer_established(
    established: &FnvHashMap<PeerId, FnvHashMap<ConnectionId, ConnectedPoint>>,
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
