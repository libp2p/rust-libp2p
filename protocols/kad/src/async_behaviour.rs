use std::{collections::HashMap, task::Poll};

use futures::{channel::mpsc, StreamExt};
use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};

use crate::{
    kbucket, store, AddProviderResult, Behaviour, BootstrapResult, Event, GetClosestPeersResult,
    GetProvidersResult, GetRecordResult, NoKnownPeers, PutRecordResult, QueryId, QueryResult,
    QueryStats, Quorum, Record, RecordKey,
};

/// The results of Kademlia queries (strongly typed) and only
/// those initiated by the user.
pub struct AsyncQueryResult<T> {
    pub id: QueryId,
    pub result: T,
    pub stats: QueryStats,
}
impl<T> AsyncQueryResult<T> {
    fn map<Out>(self, f: impl Fn(T) -> Out) -> AsyncQueryResult<Out> {
        AsyncQueryResult {
            id: self.id,
            stats: self.stats,
            result: f(self.result),
        }
    }
}

type UnboundedQueryResultSender<T> = mpsc::UnboundedSender<AsyncQueryResult<T>>;

enum QueryResultSender {
    Bootstrap(UnboundedQueryResultSender<BootstrapResult>),
    GetClosestPeers(UnboundedQueryResultSender<GetClosestPeersResult>),
    GetProviders(UnboundedQueryResultSender<GetProvidersResult>),
    StartProviding(UnboundedQueryResultSender<AddProviderResult>),
    GetRecord(UnboundedQueryResultSender<GetRecordResult>),
    PutRecord(UnboundedQueryResultSender<PutRecordResult>),
}

/// A handle to receive [`AsyncQueryResult`].
#[must_use = "Streams do nothing unless polled."]
pub struct AsyncQueryResultStream<T>(mpsc::UnboundedReceiver<AsyncQueryResult<T>>);
impl<T> futures::Stream for AsyncQueryResultStream<T> {
    type Item = AsyncQueryResult<T>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.0.poll_next_unpin(cx)
    }
}

/// A wrapper of [`Behaviour`] allowing to easily track
/// [`QueryResult`] of user initiated Kademlia queries.
///
/// For each queries like [`Behaviour::bootstrap`], [`Behaviour::get_closest_peers`], etc
/// a corresponding method ([`AsyncBehaviour::bootstrap_async`], [`AsyncBehaviour::get_closest_peers_async`])
/// is available, allowing the developer to be notified from a typed [`AsyncQueryResultStream`]
/// instead from the normal [`Event::OutboundQueryProgressed`] event.
///
/// If a [`QueryResult`] has no corresponding [`AsyncQueryResultStream`] or
/// if the corresponding one has been dropped, it will simply be forwarded to the `Swarm`
/// with an [`Event::OutboundQueryProgressed`] like nothing happen.
///
/// For more information, see [`Behaviour`].
pub struct AsyncBehaviour<TStore> {
    inner: Behaviour<TStore>,
    query_result_senders: HashMap<QueryId, QueryResultSender>,
}

impl<TStore> std::ops::Deref for AsyncBehaviour<TStore> {
    type Target = Behaviour<TStore>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<TStore> std::ops::DerefMut for AsyncBehaviour<TStore> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<TStore> AsyncBehaviour<TStore>
where
    TStore: store::RecordStore + Send + 'static,
{
    pub fn new(inner: Behaviour<TStore>) -> Self {
        Self {
            inner,
            query_result_senders: Default::default(),
        }
    }

    fn handle_inner_event(&mut self, event: Event) -> Option<<Self as NetworkBehaviour>::ToSwarm> {
        match event {
            Event::OutboundQueryProgressed {
                id,
                result,
                stats,
                step,
            } => {
                fn do_send<T>(
                    sender: &UnboundedQueryResultSender<T>,
                    id: QueryId,
                    result: T,
                    stats: QueryStats,
                ) -> Option<AsyncQueryResult<T>> {
                    match sender.unbounded_send(AsyncQueryResult { id, result, stats }) {
                        Ok(_) => {
                            // The event has been successfully sent into the channel so there is no
                            // need to forward it backup to the swarm.
                            None
                        }
                        Err(err) => {
                            // The receiver is closed. This is probably normal (the user got what he needed and dropped the receiver).
                            // So we don't log anything but we still forward this event back up to the swarm.
                            Some(err.into_inner())
                        }
                    }
                }

                let Some(sender) = self.query_result_senders.get(&id) else {
                    // This query was either not triggered by the user or the receiver has been dropped and removed
                    // so we simply forward it back up to the swarm like nothing happened.
                    return Some(Event::OutboundQueryProgressed {
                        id,
                        result,
                        stats,
                        step,
                    });
                };
                let event = match (result, sender) {
                    (QueryResult::Bootstrap(result), QueryResultSender::Bootstrap(sender)) => {
                        do_send(sender, id, result, stats).map(|qr| qr.map(QueryResult::Bootstrap))
                    }
                    (
                        QueryResult::GetClosestPeers(result),
                        QueryResultSender::GetClosestPeers(sender),
                    ) => do_send(sender, id, result, stats)
                        .map(|qr| qr.map(QueryResult::GetClosestPeers)),
                    (
                        QueryResult::GetProviders(result),
                        QueryResultSender::GetProviders(sender),
                    ) => do_send(sender, id, result, stats)
                        .map(|qr| qr.map(QueryResult::GetProviders)),
                    (
                        QueryResult::StartProviding(result),
                        QueryResultSender::StartProviding(sender),
                    ) => do_send(sender, id, result, stats)
                        .map(|qr| qr.map(QueryResult::StartProviding)),
                    (QueryResult::GetRecord(result), QueryResultSender::GetRecord(sender)) => {
                        do_send(sender, id, result, stats).map(|qr| qr.map(QueryResult::GetRecord))
                    }
                    (QueryResult::PutRecord(result), QueryResultSender::PutRecord(sender)) => {
                        do_send(sender, id, result, stats).map(|qr| qr.map(QueryResult::PutRecord))
                    }
                    (result, _) => {
                        unreachable!("Wrong sender type for query {id} : unable to send {result:?}")
                    }
                };

                if let Some(AsyncQueryResult { id, result, stats }) = event {
                    // The receiver was closed so we were unable to send the result.
                    // We remove the sender and forward the event back up to the swarm
                    self.query_result_senders.remove(&id);
                    return Some(Event::OutboundQueryProgressed {
                        id,
                        result,
                        stats,
                        step,
                    });
                }

                if step.last {
                    // This was the last query_result and we just send it successfully.
                    // We remove the sender. Dropping it will close the channel and
                    // the receiver will be notified.
                    self.query_result_senders.remove(&id);
                }

                None
            }
            event => Some(event),
        }
    }

    fn add_query<T>(
        &mut self,
        query_id: QueryId,
        f: impl Fn(UnboundedQueryResultSender<T>) -> QueryResultSender,
    ) -> AsyncQueryResultStream<T> {
        let (tx, rx) = mpsc::unbounded();
        self.query_result_senders.insert(query_id, f(tx));
        AsyncQueryResultStream(rx)
    }

    /// Start a Bootstrap query and capture its results in a typed [`AsyncQueryResultStream`].
    ///
    /// For more information, see [`Behaviour::bootstrap`].
    pub fn bootstrap_async(
        &mut self,
    ) -> Result<AsyncQueryResultStream<BootstrapResult>, NoKnownPeers> {
        let query_id = self.inner.bootstrap()?;
        Ok(self.add_query(query_id, QueryResultSender::Bootstrap))
    }

    /// Start a GetClosestPeers query and capture its results in a typed [`AsyncQueryResultStream`].
    ///
    /// For more information, see [`Behaviour::get_closest_peers`].
    pub fn get_closest_peers_async<K>(
        &mut self,
        key: K,
    ) -> AsyncQueryResultStream<GetClosestPeersResult>
    where
        K: Into<kbucket::Key<K>> + Into<Vec<u8>> + Clone,
    {
        let query_id = self.inner.get_closest_peers(key);
        self.add_query(query_id, QueryResultSender::GetClosestPeers)
    }

    /// Start a GetProviders query and capture its results in a typed [`AsyncQueryResultStream`].
    ///
    /// For more information, see [`Behaviour::get_providers`].
    pub fn get_providers_async(
        &mut self,
        key: RecordKey,
    ) -> AsyncQueryResultStream<GetProvidersResult> {
        let query_id = self.inner.get_providers(key);
        self.add_query(query_id, QueryResultSender::GetProviders)
    }

    /// Start a StartProviding query and capture its results in a typed [`AsyncQueryResultStream`].
    ///
    /// For more information, see [`Behaviour::start_providing`].
    pub fn start_providing_async(
        &mut self,
        key: RecordKey,
    ) -> Result<AsyncQueryResultStream<AddProviderResult>, store::Error> {
        let query_id = self.inner.start_providing(key)?;
        Ok(self.add_query(query_id, QueryResultSender::StartProviding))
    }

    /// Start a GetRecord query and capture its results in a typed [`AsyncQueryResultStream`].
    ///
    /// For more information, see [`Behaviour::get_record`].
    pub fn get_record_async(&mut self, key: RecordKey) -> AsyncQueryResultStream<GetRecordResult> {
        let query_id = self.inner.get_record(key);
        self.add_query(query_id, QueryResultSender::GetRecord)
    }

    /// Start a PutRecord query and capture its results in a typed [`AsyncQueryResultStream`].
    ///
    /// For more information, see [`Behaviour::put_record`].
    pub fn put_record_async(
        &mut self,
        record: Record,
        quorum: Quorum,
    ) -> Result<AsyncQueryResultStream<PutRecordResult>, store::Error> {
        let query_id = self.inner.put_record(record, quorum)?;
        Ok(self.add_query(query_id, QueryResultSender::PutRecord))
    }
}

impl<TStore> NetworkBehaviour for AsyncBehaviour<TStore>
where
    TStore: store::RecordStore + Send + 'static,
{
    type ConnectionHandler = <Behaviour<TStore> as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = <Behaviour<TStore> as NetworkBehaviour>::ToSwarm;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        self.inner.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        while let Poll::Ready(event) = self.inner.poll(cx) {
            if let Some(event) = event.map_out_opt(|e| self.handle_inner_event(e)) {
                return Poll::Ready(event);
            }
        }

        Poll::Pending
    }
}
