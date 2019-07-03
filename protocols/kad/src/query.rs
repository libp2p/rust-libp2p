// Copyright 2019 Parity Technologies (UK) Ltd.
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

mod peers;

use peers::PeersIterState;
use peers::closest::{ClosestPeersIter, ClosestPeersIterConfig};
use peers::fixed::FixedPeersIter;

use crate::K_VALUE;
use crate::kbucket::{Key, KeyBytes};
use crate::handler::KademliaHandlerIn;
use either::Either;
use fnv::FnvHashMap;
use libp2p_core::PeerId;
use std::time::Duration;
use wasm_timer::Instant;

/// A `QueryPool` provides an aggregate state machine for driving `Query`s to completion.
///
/// Internally, a `Query` is in turn driven by an underlying `QueryPeerIter`
/// that determines the peer selection strategy, i.e. the order in which the
/// peers involved in the query should be contacted.
pub struct QueryPool<TInner> {
    next_id: usize,
    config: QueryConfig,
    queries: FnvHashMap<QueryId, Query<TInner>>,
}

/// The observable states emitted by [`QueryPool::poll`].
pub enum QueryPoolState<'a, TInner> {
    /// The pool is idle, i.e. there are no queries to process.
    Idle,
    /// At least one query is waiting for results. `Some(request)` indicates
    /// that a new request is now being waited on.
    Waiting(Option<(&'a Query<TInner>, PeerId)>),
    /// A query has finished.
    Finished(Query<TInner>),
    /// A query has timed out.
    Timeout(Query<TInner>)
}

impl<TInner> QueryPool<TInner> {
    /// Creates a new `QueryPool` with the given configuration.
    pub fn new(config: QueryConfig) -> Self {
        QueryPool {
            next_id: 0,
            config,
            queries: Default::default()
        }
    }

    /// Gets a reference to the `QueryConfig` used by the pool.
    pub fn config(&self) -> &QueryConfig {
        &self.config
    }

    /// Returns an iterator over the queries in the pool.
    pub fn iter(&self) -> impl Iterator<Item = &Query<TInner>> {
        self.queries.values()
    }

    /// Returns an iterator that allows modifying each query in the pool.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Query<TInner>> {
        self.queries.values_mut()
    }

    /// Adds a query to the pool that contacts a fixed set of peers.
    pub fn add_fixed<I>(&mut self, peers: I, inner: TInner) -> QueryId
    where
        I: IntoIterator<Item = Key<PeerId>>
    {
        let peers = peers.into_iter().map(|k| k.into_preimage()).collect::<Vec<_>>();
        let parallelism = self.config.replication_factor;
        let peer_iter = QueryPeerIter::Fixed(FixedPeersIter::new(peers, parallelism));
        self.add(peer_iter, inner)
    }

    /// Adds a query to the pool that iterates towards the closest peers to the target.
    pub fn add_iter_closest<T, I>(&mut self, target: T, peers: I, inner: TInner) -> QueryId
    where
        T: Into<KeyBytes>,
        I: IntoIterator<Item = Key<PeerId>>
    {
        let cfg = ClosestPeersIterConfig {
            num_results: self.config.replication_factor,
            .. ClosestPeersIterConfig::default()
        };
        let peer_iter = QueryPeerIter::Closest(ClosestPeersIter::with_config(cfg, target, peers));
        self.add(peer_iter, inner)
    }

    fn add(&mut self, peer_iter: QueryPeerIter, inner: TInner) -> QueryId {
        let id = QueryId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let query = Query::new(id, peer_iter, inner);
        self.queries.insert(id, query);
        id
    }

    /// Returns a reference to a query with the given ID, if it is in the pool.
    pub fn get(&self, id: &QueryId) -> Option<&Query<TInner>> {
        self.queries.get(id)
    }

    /// Returns a mutablereference to a query with the given ID, if it is in the pool.
    pub fn get_mut(&mut self, id: &QueryId) -> Option<&mut Query<TInner>> {
        self.queries.get_mut(id)
    }

    /// Polls the pool to advance the queries.
    pub fn poll(&mut self, now: Instant) -> QueryPoolState<TInner> {
        let mut finished = None;
        let mut timeout = None;
        let mut waiting = None;

        for (&query_id, query) in self.queries.iter_mut() {
            query.started = query.started.or(Some(now));
            match query.next(now) {
                PeersIterState::Finished => {
                    finished = Some(query_id);
                    break
                }
                PeersIterState::Waiting(Some(peer_id)) => {
                    let peer = peer_id.into_owned();
                    waiting = Some((query_id, peer));
                    break
                }
                PeersIterState::Waiting(None) | PeersIterState::WaitingAtCapacity => {
                    let elapsed = now - query.started.unwrap_or(now);
                    if elapsed >= self.config.timeout {
                        timeout = Some(query_id);
                        break
                    }
                }
            }
        }

        if let Some((query_id, peer_id)) = waiting {
            let query = self.queries.get(&query_id).expect("s.a.");
            return QueryPoolState::Waiting(Some((query, peer_id)))
        }

        if let Some(query_id) = finished {
            let query = self.queries.remove(&query_id).expect("s.a.");
            return QueryPoolState::Finished(query)
        }

        if let Some(query_id) = timeout {
            let query = self.queries.remove(&query_id).expect("s.a.");
            return QueryPoolState::Timeout(query)
        }

        if self.queries.is_empty() {
            return QueryPoolState::Idle
        } else {
            return QueryPoolState::Waiting(None)
        }
    }
}

/// Unique identifier for an active query.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct QueryId(usize);

/// The configuration for queries in a `QueryPool`.
#[derive(Clone)]
pub struct QueryConfig {
    pub timeout: Duration,
    pub replication_factor: usize,
}

impl Default for QueryConfig {
    fn default() -> Self {
        QueryConfig {
            timeout: Duration::from_secs(60),
            replication_factor: K_VALUE
        }
    }
}

/// A query in a `QueryPool`.
pub struct Query<TInner> {
    /// The unique ID of the query.
    id: QueryId,
    /// The peer iterator that drives the query state.
    peer_iter: QueryPeerIter,
    /// The instant when the query started (i.e. began waiting for the first
    /// result from a peer).
    started: Option<Instant>,
    /// The opaque inner query state.
    pub inner: TInner,
}

/// The peer selection strategies that can be used by queries.
enum QueryPeerIter {
    Closest(ClosestPeersIter),
    Fixed(FixedPeersIter)
}

impl<TInner> Query<TInner> {
    /// Creates a new query without starting it.
    fn new(id: QueryId, peer_iter: QueryPeerIter, inner: TInner) -> Self {
        Query { id, inner, peer_iter, started: None }
    }

    /// Gets the unique ID of the query.
    pub fn id(&self) -> QueryId {
        self.id
    }

    /// Informs the query that the attempt to contact `peer` failed.
    pub fn on_failure(&mut self, peer: &PeerId) {
        match &mut self.peer_iter {
            QueryPeerIter::Closest(iter) => iter.on_failure(peer),
            QueryPeerIter::Fixed(iter) => iter.on_failure(peer)
        }
    }

    /// Informs the query that the attempt to contact `peer` succeeded,
    /// possibly resulting in new peers that should be incorporated into
    /// the query, if applicable.
    pub fn on_success<I>(&mut self, peer: &PeerId, new_peers: I)
    where
        I: IntoIterator<Item = PeerId>
    {
        match &mut self.peer_iter {
            QueryPeerIter::Closest(iter) => iter.on_success(peer, new_peers),
            QueryPeerIter::Fixed(iter) => iter.on_success(peer)
        }
    }

    /// Checks whether the query is currently waiting for a result from `peer`.
    pub fn is_waiting(&self, peer: &PeerId) -> bool {
        match &self.peer_iter {
            QueryPeerIter::Closest(iter) => iter.is_waiting(peer),
            QueryPeerIter::Fixed(iter) => iter.is_waiting(peer)
        }
    }

    /// Advances the state of the underlying peer iterator.
    fn next(&mut self, now: Instant) -> PeersIterState {
        match &mut self.peer_iter {
            QueryPeerIter::Closest(iter) => iter.next(now),
            QueryPeerIter::Fixed(iter) => iter.next()
        }
    }

    /// Finishes the query prematurely.
    ///
    /// A finished query immediately stops yielding new peers to contact and will be
    /// reported by [`QueryPool::poll`] via [`QueryPoolState::Finished`].
    pub fn finish(&mut self) {
        match &mut self.peer_iter {
            QueryPeerIter::Closest(iter) => iter.finish(),
            QueryPeerIter::Fixed(iter) => iter.finish()
        }
    }

    /// Consumes the query, producing the final `QueryResult`.
    pub fn into_result(self) -> QueryResult<TInner, impl Iterator<Item = PeerId>> {
        let peers = match self.peer_iter {
            QueryPeerIter::Closest(iter) => Either::Left(iter.into_result()),
            QueryPeerIter::Fixed(iter) => Either::Right(iter.into_result())
        };
        QueryResult { inner: self.inner, peers }
    }
}

/// The result of a `Query`.
pub struct QueryResult<TInner, TPeers> {
    /// The opaque inner query state.
    pub inner: TInner,
    /// The successfully contacted peers.
    pub peers: TPeers
}

