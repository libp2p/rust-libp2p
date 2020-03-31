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
    Executor,
    ConnectedPoint,
    PeerId,
    connection::{
        self,
        Connected,
        Connection,
        ConnectionId,
        ConnectionLimit,
        ConnectionError,
        ConnectionHandler,
        ConnectionInfo,
        IncomingInfo,
        IntoConnectionHandler,
        OutgoingInfo,
        Substream,
        PendingConnectionError,
        manager::{self, Manager},
    },
    muxing::StreamMuxer,
};
use either::Either;
use fnv::FnvHashMap;
use futures::prelude::*;
use smallvec::SmallVec;
use std::{convert::TryFrom as _, error, fmt, hash::Hash, num::NonZeroU32, task::Context, task::Poll};

/// A connection `Pool` manages a set of connections for each peer.
pub struct Pool<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo = PeerId, TPeerId = PeerId> {
    local_id: TPeerId,

    /// The configuration of the pool.
    limits: PoolLimits,

    /// The connection manager that handles the connection I/O for both
    /// established and pending connections.
    ///
    /// For every established connection there is a corresponding entry in `established`.
    manager: Manager<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo>,

    /// The managed connections of each peer that are currently considered
    /// established, as witnessed by the associated `ConnectedPoint`.
    established: FnvHashMap<TPeerId, FnvHashMap<ConnectionId, ConnectedPoint>>,

    /// The pending connections that are currently being negotiated.
    pending: FnvHashMap<ConnectionId, (ConnectedPoint, Option<TPeerId>)>,
}

impl<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId> fmt::Debug
for Pool<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        // TODO: More useful debug impl?
        f.debug_struct("Pool")
            .field("limits", &self.limits)
            .finish()
    }
}

impl<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId> Unpin
for Pool<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId> {}

/// Event that can happen on the `Pool`.
pub enum PoolEvent<'a, TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId> {
    /// A new connection has been established.
    ConnectionEstablished {
        connection: EstablishedConnection<'a, TInEvent, TConnInfo, TPeerId>,
        num_established: NonZeroU32,
    },

    /// An established connection has encountered an error.
    ConnectionError {
        id: ConnectionId,
        /// Information about the connection that errored.
        connected: Connected<TConnInfo>,
        /// The error that occurred.
        error: ConnectionError<THandlerErr>,
        /// A reference to the pool that used to manage the connection.
        pool: &'a mut Pool<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>,
        /// The remaining number of established connections to the same peer.
        num_established: u32,
    },

    /// A connection attempt failed.
    PendingConnectionError {
        /// The ID of the failed connection.
        id: ConnectionId,
        /// The local endpoint of the failed connection.
        endpoint: ConnectedPoint,
        /// The error that occurred.
        error: PendingConnectionError<TTransErr>,
        /// The handler that was supposed to handle the connection,
        /// if the connection failed before the handler was consumed.
        handler: THandler,
        /// The (expected) peer of the failed connection.
        peer: Option<TPeerId>,
        /// A reference to the pool that managed the connection.
        pool: &'a mut Pool<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>,
    },

    /// A node has produced an event.
    ConnectionEvent {
        /// The connection that has generated the event.
        connection: EstablishedConnection<'a, TInEvent, TConnInfo, TPeerId>,
        /// The produced event.
        event: TOutEvent,
    },
}

impl<'a, TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId> fmt::Debug
for PoolEvent<'a, TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>
where
    TOutEvent: fmt::Debug,
    TTransErr: fmt::Debug,
    THandlerErr: fmt::Debug,
    TConnInfo: fmt::Debug,
    TInEvent: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            PoolEvent::ConnectionEstablished { ref connection, .. } => {
                f.debug_tuple("PoolEvent::ConnectionEstablished")
                    .field(connection)
                    .finish()
            },
            PoolEvent::ConnectionError { ref id, ref connected, ref error, .. } => {
                f.debug_struct("PoolEvent::ConnectionError")
                    .field("id", id)
                    .field("connected", connected)
                    .field("error", error)
                    .finish()
            },
            PoolEvent::PendingConnectionError { ref id, ref error, .. } => {
                f.debug_struct("PoolEvent::PendingConnectionError")
                    .field("id", id)
                    .field("error", error)
                    .finish()
            },
            PoolEvent::ConnectionEvent { ref connection, ref event } => {
                f.debug_struct("PoolEvent::ConnectionEvent")
                    .field("conn_info", connection.info())
                    .field("event", event)
                    .finish()
            },
        }
    }
}

impl<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>
    Pool<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>
where
    TPeerId: Eq + Hash,
{
    /// Creates a new empty `Pool`.
    pub fn new(
        local_id: TPeerId,
        executor: Option<Box<dyn Executor + Send>>,
        limits: PoolLimits
    ) -> Self {
        Pool {
            local_id,
            limits,
            manager: Manager::new(executor),
            established: Default::default(),
            pending: Default::default(),
        }
    }

    /// Gets the configured connection limits of the pool.
    pub fn limits(&self) -> &PoolLimits {
        &self.limits
    }

    /// Adds a pending incoming connection to the pool in the form of a
    /// `Future` that establishes and negotiates the connection.
    ///
    /// Returns an error if the limit of pending incoming connections
    /// has been reached.
    pub fn add_incoming<TFut, TMuxer>(
        &mut self,
        future: TFut,
        handler: THandler,
        info: IncomingInfo,
    ) -> Result<ConnectionId, ConnectionLimit>
    where
        TConnInfo: ConnectionInfo<PeerId = TPeerId> + Send + 'static,
        TFut: Future<
            Output = Result<(TConnInfo, TMuxer), PendingConnectionError<TTransErr>>
        > + Send + 'static,
        THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
        THandler::Handler: ConnectionHandler<
            Substream = Substream<TMuxer>,
            InEvent = TInEvent,
            OutEvent = TOutEvent,
            Error = THandlerErr
        > + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TTransErr: error::Error + Send + 'static,
        THandlerErr: error::Error + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
        TPeerId: Clone + Send + 'static,
    {
        let endpoint = info.to_connected_point();
        if let Some(limit) = self.limits.max_pending_incoming {
            let current = self.iter_pending_incoming().count();
            if current >= limit {
                return Err(ConnectionLimit { limit, current })
            }
        }
        Ok(self.add_pending(future, handler, endpoint, None))
    }

    /// Adds a pending outgoing connection to the pool in the form of a `Future`
    /// that establishes and negotiates the connection.
    ///
    /// Returns an error if the limit of pending outgoing connections
    /// has been reached.
    pub fn add_outgoing<TFut, TMuxer>(
        &mut self,
        future: TFut,
        handler: THandler,
        info: OutgoingInfo<TPeerId>,
    ) -> Result<ConnectionId, ConnectionLimit>
    where
        TConnInfo: ConnectionInfo<PeerId = TPeerId> + Send + 'static,
        TFut: Future<
            Output = Result<(TConnInfo, TMuxer), PendingConnectionError<TTransErr>>
        > + Send + 'static,
        THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
        THandler::Handler: ConnectionHandler<
            Substream = Substream<TMuxer>,
            InEvent = TInEvent,
            OutEvent = TOutEvent,
            Error = THandlerErr
        > + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TTransErr: error::Error + Send + 'static,
        THandlerErr: error::Error + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
        TPeerId: Clone + Send + 'static,
    {
        self.limits.check_outgoing(|| self.iter_pending_outgoing().count())?;
        let endpoint = info.to_connected_point();
        Ok(self.add_pending(future, handler, endpoint, info.peer_id.cloned()))
    }

    /// Adds a pending connection to the pool in the form of a
    /// `Future` that establishes and negotiates the connection.
    fn add_pending<TFut, TMuxer>(
        &mut self,
        future: TFut,
        handler: THandler,
        endpoint: ConnectedPoint,
        peer: Option<TPeerId>,
    ) -> ConnectionId
    where
        TConnInfo: ConnectionInfo<PeerId = TPeerId> + Send + 'static,
        TFut: Future<
            Output = Result<(TConnInfo, TMuxer), PendingConnectionError<TTransErr>>
        > + Send + 'static,
        THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
        THandler::Handler: ConnectionHandler<
            Substream = Substream<TMuxer>,
            InEvent = TInEvent,
            OutEvent = TOutEvent,
            Error = THandlerErr
        > + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TTransErr: error::Error + Send + 'static,
        THandlerErr: error::Error + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
        TPeerId: Clone + Send + 'static,
    {
        // Validate the received peer ID as the last step of the pending connection
        // future, so that these errors can be raised before the `handler` is consumed
        // by the background task, which happens when this future resolves to an
        // "established" connection.
        let future = future.and_then({
            let endpoint = endpoint.clone();
            let expected_peer = peer.clone();
            let local_id = self.local_id.clone();
            move |(info, muxer)| {
                if let Some(peer) = expected_peer {
                    if &peer != info.peer_id() {
                        return future::err(PendingConnectionError::InvalidPeerId)
                    }
                }

                if &local_id == info.peer_id() {
                    return future::err(PendingConnectionError::InvalidPeerId)
                }

                let connected = Connected { info, endpoint };
                future::ready(Ok((connected, muxer)))
            }
        });

        let id = self.manager.add_pending(future, handler);
        self.pending.insert(id, (endpoint, peer));
        id
    }

    /// Adds an existing established connection to the pool.
    ///
    /// Returns the assigned connection ID on success. An error is returned
    /// if the configured maximum number of established connections for the
    /// connected peer has been reached.
    pub fn add<TMuxer>(&mut self, c: Connection<TMuxer, THandler::Handler>, i: Connected<TConnInfo>)
        -> Result<ConnectionId, ConnectionLimit>
    where
        THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
        THandler::Handler: ConnectionHandler<
            Substream = connection::Substream<TMuxer>,
            InEvent = TInEvent,
            OutEvent = TOutEvent,
            Error = THandlerErr
        > + Send + 'static,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TTransErr: error::Error + Send + 'static,
        THandlerErr: error::Error + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
        TConnInfo: Clone + Send + 'static,
        TPeerId: Clone,
        TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    {
        if let Some(limit) = self.limits.max_established_per_peer {
            let current = self.num_peer_established(i.peer_id());
            if limit >= current {
                return Err(ConnectionLimit { limit, current })
            }
        }
        let id = self.manager.add(c, i.clone());
        self.established.entry(i.peer_id().clone()).or_default().insert(id, i.endpoint);
        Ok(id)
    }

    /// Gets an entry representing a connection in the pool.
    ///
    /// Returns `None` if the pool has no connection with the given ID.
    pub fn get(&mut self, id: ConnectionId)
        -> Option<PoolConnection<'_, TInEvent, TConnInfo, TPeerId>>
    {
        match self.manager.entry(id) {
            Some(manager::Entry::Established(entry)) =>
                Some(PoolConnection::Established(EstablishedConnection {
                    entry,
                    established: &mut self.established,
                })),
            Some(manager::Entry::Pending(entry)) =>
                Some(PoolConnection::Pending(PendingConnection {
                    entry,
                    pending: &mut self.pending,
                })),
            None => None
        }
    }

    /// Gets an established connection from the pool by ID.
    pub fn get_established(&mut self, id: ConnectionId)
        -> Option<EstablishedConnection<'_, TInEvent, TConnInfo, TPeerId>>
    {
        match self.get(id) {
            Some(PoolConnection::Established(c)) => Some(c),
            _ => None
        }
    }

    /// Gets a pending outgoing connection by ID.
    pub fn get_outgoing(&mut self, id: ConnectionId)
        -> Option<PendingConnection<'_, TInEvent, TConnInfo, TPeerId>>
    {
        match self.pending.get(&id) {
            Some((ConnectedPoint::Dialer { .. }, _peer)) =>
                match self.manager.entry(id) {
                    Some(manager::Entry::Pending(entry)) =>
                        Some(PendingConnection {
                            entry,
                            pending: &mut self.pending,
                        }),
                    _ => unreachable!("by consistency of `self.pending` with `self.manager`")
                }
            _ => None
        }
    }

    /// Returns true if we are connected to the given peer.
    ///
    /// This will return true only after a `NodeReached` event has been produced by `poll()`.
    pub fn is_connected(&self, id: &TPeerId) -> bool {
        self.established.contains_key(id)
    }

    /// Returns the number of connected peers, i.e. those with at least one
    /// established connection in the pool.
    pub fn num_connected(&self) -> usize {
        self.established.len()
    }

    /// Close all connections to the given peer.
    pub fn disconnect(&mut self, peer: &TPeerId) {
        if let Some(conns) = self.established.get(peer) {
            for id in conns.keys() {
                match self.manager.entry(*id) {
                    Some(manager::Entry::Established(e)) => { e.close(); },
                    _ => {}
                }
            }
        }

        for (id, (_endpoint, peer2)) in &self.pending {
            if Some(peer) == peer2.as_ref() {
                match self.manager.entry(*id) {
                    Some(manager::Entry::Pending(e)) => { e.abort(); },
                    _ => {}
                }
            }
        }
    }

    /// Counts the number of established connections in the pool.
    pub fn num_established(&self) -> usize {
        self.established.iter().fold(0, |n, (_, conns)| n + conns.len())
    }

    /// Counts the number of pending connections in the pool.
    pub fn num_pending(&self) -> usize {
        self.iter_pending_info().count()
    }

    /// Counts the number of established connections to the given peer.
    pub fn num_peer_established(&self, peer: &TPeerId) -> usize {
        self.established.get(peer).map_or(0, |conns| conns.len())
    }

    /// Returns an iterator over all established connections of `peer`.
    pub fn iter_peer_established<'a>(&'a mut self, peer: &TPeerId)
        -> EstablishedConnectionIter<'a,
            impl Iterator<Item = ConnectionId>,
            TInEvent,
            TOutEvent,
            THandler,
            TTransErr,
            THandlerErr,
            TConnInfo,
            TPeerId>
    {
        let ids = self.iter_peer_established_info(peer)
            .map(|(id, _endpoint)| *id)
            .collect::<SmallVec<[ConnectionId; 10]>>()
            .into_iter();

        EstablishedConnectionIter { pool: self, ids }
    }

    /// Returns an iterator for information on all pending incoming connections.
    pub fn iter_pending_incoming(&self) -> impl Iterator<Item = IncomingInfo<'_>> {
        self.iter_pending_info()
            .filter_map(|(_, ref endpoint, _)| {
                match endpoint {
                    ConnectedPoint::Listener { local_addr, send_back_addr } => {
                        Some(IncomingInfo { local_addr, send_back_addr })
                    },
                    ConnectedPoint::Dialer { .. } => None,
                }
            })
    }

    /// Returns an iterator for information on all pending outgoing connections.
    pub fn iter_pending_outgoing(&self) -> impl Iterator<Item = OutgoingInfo<'_, TPeerId>> {
        self.iter_pending_info()
            .filter_map(|(_, ref endpoint, ref peer_id)| {
                match endpoint {
                    ConnectedPoint::Listener { .. } => None,
                    ConnectedPoint::Dialer { address } =>
                        Some(OutgoingInfo { address, peer_id: peer_id.as_ref() }),
                }
            })
    }

    /// Returns an iterator over all connection IDs and associated endpoints
    /// of established connections to `peer` known to the pool.
    pub fn iter_peer_established_info(&self, peer: &TPeerId)
        -> impl Iterator<Item = (&ConnectionId, &ConnectedPoint)> + fmt::Debug + '_
    {
        match self.established.get(peer) {
            Some(conns) => Either::Left(conns.iter()),
            None => Either::Right(std::iter::empty())
        }
    }

    /// Returns an iterator over all pending connection IDs together
    /// with associated endpoints and expected peer IDs in the pool.
    pub fn iter_pending_info(&self)
        -> impl Iterator<Item = (&ConnectionId, &ConnectedPoint, &Option<TPeerId>)> + '_
    {
        self.pending.iter().map(|(id, (endpoint, info))| (id, endpoint, info))
    }

    /// Returns an iterator over all connected peers, i.e. those that have
    /// at least one established connection in the pool.
    pub fn iter_connected<'a>(&'a self) -> impl Iterator<Item = &'a TPeerId> + 'a {
        self.established.keys()
    }

    /// Polls the connection pool for events.
    ///
    /// > **Note**: We use a regular `poll` method instead of implementing `Stream`,
    /// > because we want the `Pool` to stay borrowed if necessary.
    pub fn poll<'a>(&'a mut self, cx: &mut Context) -> Poll<
        PoolEvent<'a, TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>
    > where
        TConnInfo: ConnectionInfo<PeerId = TPeerId> + Clone,
        TPeerId: Clone
    {
        loop {
            let item = match self.manager.poll(cx) {
                Poll::Ready(item) => item,
                Poll::Pending => return Poll::Pending,
            };

            match item {
                manager::Event::PendingConnectionError { id, error, handler } => {
                    if let Some((endpoint, peer)) = self.pending.remove(&id) {
                        return Poll::Ready(PoolEvent::PendingConnectionError {
                            id,
                            endpoint,
                            error,
                            handler,
                            peer,
                            pool: self
                        })
                    }
                },
                manager::Event::ConnectionError { id, connected, error } => {
                    let num_established =
                        if let Some(conns) = self.established.get_mut(connected.peer_id()) {
                            conns.remove(&id);
                            u32::try_from(conns.len()).unwrap()
                        } else {
                            0
                        };
                    if num_established == 0 {
                        self.established.remove(connected.peer_id());
                    }
                    return Poll::Ready(PoolEvent::ConnectionError {
                        id, connected, error, num_established, pool: self
                    })
                },
                manager::Event::ConnectionEstablished { entry } => {
                    let id = entry.id();
                    if let Some((endpoint, peer)) = self.pending.remove(&id) {
                        // Check connection limit.
                        let established = &self.established;
                        let current = || established.get(entry.connected().peer_id())
                                            .map_or(0, |conns| conns.len());
                        if let Err(e) = self.limits.check_established(current) {
                            let connected = entry.close();
                            let num_established = u32::try_from(e.current).unwrap();
                            return Poll::Ready(PoolEvent::ConnectionError {
                                id,
                                connected,
                                error: ConnectionError::ConnectionLimit(e),
                                num_established,
                                pool: self,
                            })
                        }
                        // Peer ID checks must already have happened. See `add_pending`.
                        if cfg!(debug_assertions) {
                            if &self.local_id == entry.connected().peer_id() {
                                panic!("Unexpected local peer ID for remote.");
                            }
                            if let Some(peer) = peer {
                                if &peer != entry.connected().peer_id() {
                                    panic!("Unexpected peer ID mismatch.");
                                }
                            }
                        }
                        // Add the connection to the pool.
                        let peer = entry.connected().peer_id().clone();
                        let conns = self.established.entry(peer).or_default();
                        let num_established = NonZeroU32::new(u32::try_from(conns.len() + 1).unwrap())
                            .expect("n + 1 is always non-zero; qed");
                        conns.insert(id, endpoint);
                        match self.get(id) {
                            Some(PoolConnection::Established(connection)) =>
                                return Poll::Ready(PoolEvent::ConnectionEstablished {
                                    connection, num_established
                                }),
                            _ => unreachable!("since `entry` is an `EstablishedEntry`.")
                        }
                    }
                },
                manager::Event::ConnectionEvent { entry, event } => {
                    let id = entry.id();
                    match self.get(id) {
                        Some(PoolConnection::Established(connection)) =>
                            return Poll::Ready(PoolEvent::ConnectionEvent {
                                connection,
                                event,
                            }),
                        _ => unreachable!("since `entry` is an `EstablishedEntry`.")
                    }
                }
            }
        }
    }

}

/// A connection in a [`Pool`].
pub enum PoolConnection<'a, TInEvent, TConnInfo, TPeerId> {
    Pending(PendingConnection<'a, TInEvent, TConnInfo, TPeerId>),
    Established(EstablishedConnection<'a, TInEvent, TConnInfo, TPeerId>),
}

/// A pending connection in a [`Pool`].
pub struct PendingConnection<'a, TInEvent, TConnInfo, TPeerId> {
    entry: manager::PendingEntry<'a, TInEvent, TConnInfo>,
    pending: &'a mut FnvHashMap<ConnectionId, (ConnectedPoint, Option<TPeerId>)>,
}

impl<TInEvent, TConnInfo, TPeerId>
    PendingConnection<'_, TInEvent, TConnInfo, TPeerId>
{
    /// Returns the local connection ID.
    pub fn id(&self) -> ConnectionId {
        self.entry.id()
    }

    /// Returns the (expected) identity of the remote peer, if known.
    pub fn peer_id(&self) -> &Option<TPeerId> {
        &self.pending.get(&self.entry.id()).expect("`entry` is a pending entry").1
    }

    /// Returns information about this endpoint of the connection.
    pub fn endpoint(&self) -> &ConnectedPoint {
        &self.pending.get(&self.entry.id()).expect("`entry` is a pending entry").0
    }

    /// Aborts the connection attempt, closing the connection.
    pub fn abort(self) {
        self.pending.remove(&self.entry.id());
        self.entry.abort();
    }
}

/// An established connection in a [`Pool`].
pub struct EstablishedConnection<'a, TInEvent, TConnInfo, TPeerId> {
    entry: manager::EstablishedEntry<'a, TInEvent, TConnInfo>,
    established: &'a mut FnvHashMap<TPeerId, FnvHashMap<ConnectionId, ConnectedPoint>>,
}

impl<TInEvent, TConnInfo, TPeerId> fmt::Debug
for EstablishedConnection<'_, TInEvent, TConnInfo, TPeerId>
where
    TInEvent: fmt::Debug,
    TConnInfo: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("EstablishedConnection")
            .field("entry", &self.entry)
            .finish()
    }
}

impl<TInEvent, TConnInfo, TPeerId>
    EstablishedConnection<'_, TInEvent, TConnInfo, TPeerId>
{
    pub fn connected(&self) -> &Connected<TConnInfo> {
        self.entry.connected()
    }

    /// Returns information about the connected endpoint.
    pub fn endpoint(&self) -> &ConnectedPoint {
        &self.entry.connected().endpoint
    }

    /// Returns connection information obtained from the transport.
    pub fn info(&self) -> &TConnInfo {
        &self.entry.connected().info
    }
}

impl<TInEvent, TConnInfo, TPeerId>
    EstablishedConnection<'_, TInEvent, TConnInfo, TPeerId>
where
    TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    TPeerId: Eq + Hash + Clone,
{
    /// Returns the local connection ID.
    pub fn id(&self) -> ConnectionId {
        self.entry.id()
    }

    /// Returns the identity of the connected peer.
    pub fn peer_id(&self) -> &TPeerId {
        self.info().peer_id()
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
    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context) -> Poll<Result<(),()>> {
        self.entry.poll_ready_notify_handler(cx)
    }

    /// Closes the connection, returning the connection information.
    pub fn close(self) -> Connected<TConnInfo> {
        let id = self.entry.id();
        let info = self.entry.close();

        let empty =
            if let Some(conns) = self.established.get_mut(info.peer_id()) {
                conns.remove(&id);
                conns.is_empty()
            } else {
                false
            };

        if empty {
            self.established.remove(info.peer_id());
        }

        info
    }
}

/// An iterator over established connections in a [`Pool`].
pub struct EstablishedConnectionIter<'a, I, TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId> {
    pool: &'a mut Pool<TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>,
    ids: I
}

// Note: Ideally this would be an implementation of `Iterator`, but that
// requires GATs (cf. https://github.com/rust-lang/rust/issues/44265) and
// a different definition of `Iterator`.
impl<'a, I, TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>
    EstablishedConnectionIter<'a, I, TInEvent, TOutEvent, THandler, TTransErr, THandlerErr, TConnInfo, TPeerId>
where
    I: Iterator<Item = ConnectionId>
{
    /// Obtains the next connection, if any.
    pub fn next<'b>(&'b mut self) -> Option<EstablishedConnection<'b, TInEvent, TConnInfo, TPeerId>>
    {
        while let Some(id) = self.ids.next() {
            if self.pool.manager.is_established(&id) { // (*)
                match self.pool.manager.entry(id) {
                    Some(manager::Entry::Established(entry)) => {
                        let established = &mut self.pool.established;
                        return Some(EstablishedConnection { entry, established })
                    }
                    _ => unreachable!("by (*)")
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
    pub fn into_first<'b>(mut self)
        -> Option<EstablishedConnection<'b, TInEvent, TConnInfo, TPeerId>>
    where 'a: 'b
    {
        while let Some(id) = self.ids.next() {
            if self.pool.manager.is_established(&id) { // (*)
                match self.pool.manager.entry(id) {
                    Some(manager::Entry::Established(entry)) => {
                        let established = &mut self.pool.established;
                        return Some(EstablishedConnection { entry, established })
                    }
                    _ => unreachable!("by (*)")
                }
            }
        }
        None
    }
}

/// The configurable limits of a connection [`Pool`].
#[derive(Debug, Clone, Default)]
pub struct PoolLimits {
    pub max_pending_outgoing: Option<usize>,
    pub max_pending_incoming: Option<usize>,
    pub max_established_per_peer: Option<usize>,
}

impl PoolLimits {
    fn check_established<F>(&self, current: F) -> Result<(), ConnectionLimit>
    where
        F: FnOnce() -> usize
    {
        Self::check(current, self.max_established_per_peer)
    }

    fn check_outgoing<F>(&self, current: F) -> Result<(), ConnectionLimit>
    where
        F: FnOnce() -> usize
    {
        Self::check(current, self.max_pending_outgoing)
    }

    fn check<F>(current: F, limit: Option<usize>) -> Result<(), ConnectionLimit>
    where
        F: FnOnce() -> usize
    {
        if let Some(limit) = limit {
            let current = current();
            if current >= limit {
                return Err(ConnectionLimit { limit, current })
            }
        }
        Ok(())
    }
}
