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

use fnv::FnvHashSet;
use futures::{future, Future, IntoFuture, stream, Stream};
use kad_server::KadConnecController;
use kbucket::{KBucketsTable, KBucketsPeerId};
use libp2p_core::PeerId;
use multiaddr::Multiaddr;
use protocol;
use rand;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use std::time::Duration;
use tokio_timer::Timeout;

/// Prototype for a future Kademlia protocol running on a socket.
#[derive(Debug, Clone)]
pub struct KadSystemConfig<I> {
    /// Degree of parallelism on the network. Often called `alpha` in technical papers.
    /// No more than this number of remotes will be used at a given time for any given operation.
    // TODO: ^ share this number between operations? or does each operation use `alpha` remotes?
    pub parallelism: u32,
    /// Id of the local peer.
    pub local_peer_id: PeerId,
    /// List of peers initially known.
    pub known_initial_peers: I,
    /// Duration after which a node in the k-buckets needs to be pinged again.
    pub kbuckets_timeout: Duration,
    /// When contacting a node, duration after which we consider it unresponsive.
    pub request_timeout: Duration,
}

/// System that drives the whole Kademlia process.
pub struct KadSystem {
    // The actual DHT.
    kbuckets: KBucketsTable<PeerId, ()>,
    // Same as in the config.
    parallelism: u32,
    // Same as in the config.
    request_timeout: Duration,
}

/// Event that happens during a query.
#[derive(Debug, Clone)]
pub enum KadQueryEvent<TOut> {
    /// Learned about new mutiaddresses for the given peers.
    NewKnownMultiaddrs(Vec<(PeerId, Vec<Multiaddr>)>),
    /// Finished the processing of the query. Contains the result.
    Finished(TOut),
}

impl KadSystem {
    /// Starts a new Kademlia system.
    ///
    /// Also produces a `Future` that drives a Kademlia initialization process.
    /// This future should be driven to completion by the caller.
    pub fn start<'a, F, Fut>(config: KadSystemConfig<impl Iterator<Item = PeerId>>, access: F)
        -> (KadSystem, impl Future<Item = (), Error = IoError> + 'a)
        where F: FnMut(&PeerId) -> Fut + Send + Clone + 'a,
            Fut: IntoFuture<Item = KadConnecController, Error = IoError> + 'a,
            Fut::Future: Send,
    {
        let system = KadSystem::without_init(config);
        let init_future = system.perform_initialization(access);
        (system, init_future)
    }

    /// Same as `start`, but doesn't perform the initialization process.
    pub fn without_init(config: KadSystemConfig<impl Iterator<Item = PeerId>>) -> KadSystem {
        let kbuckets = KBucketsTable::new(config.local_peer_id.clone(), config.kbuckets_timeout);
        for peer in config.known_initial_peers {
            let _ = kbuckets.update(peer, ());
        }

        let system = KadSystem {
            kbuckets: kbuckets,
            parallelism: config.parallelism,
            request_timeout: config.request_timeout,
        };

        system
    }

    /// Starts an initialization process.
    pub fn perform_initialization<'a, F, Fut>(&self, access: F) -> impl Future<Item = (), Error = IoError> + 'a
        where F: FnMut(&PeerId) -> Fut + Send + Clone + 'a,
            Fut: IntoFuture<Item = KadConnecController, Error = IoError>  + 'a,
            Fut::Future: Send,
    {
        let futures: Vec<_> = (0..256)      // TODO: 256 is arbitrary
            .map(|n| {
                refresh(n, access.clone(), &self.kbuckets,
                        self.parallelism as usize, self.request_timeout)
            })
            .map(|stream| stream.for_each(|_| Ok(())))
            .collect();

        future::loop_fn(futures, |futures| {
            if futures.is_empty() {
                let fut = future::ok(future::Loop::Break(()));
                return future::Either::A(fut);
            }

            let fut = future::select_all(futures)
                .map_err(|(err, _, _)| err)
                .map(|(_, _, rest)| future::Loop::Continue(rest));
            future::Either::B(fut)
        })
    }

    /// Updates the k-buckets with the specific peer.
    ///
    /// Should be called whenever we receive a message from a peer.
    pub fn update_kbuckets(&self, peer: PeerId) {
        // TODO: ping system
        let _ = self.kbuckets.update(peer, ());
    }

    /// Returns the local peer ID, as passed in the configuration.
    pub fn local_peer_id(&self) -> &PeerId {
        self.kbuckets.my_id()
    }

    /// Finds the known nodes closest to `id`, ordered by distance.
    pub fn known_closest_peers(&self, id: &PeerId) -> impl Iterator<Item = PeerId> {
        self.kbuckets.find_closest_with_self(id)
    }

    /// Starts a query for an iterative `FIND_NODE` request.
    pub fn find_node<'a, F, Fut>(&self, searched_key: PeerId, access: F)
        -> impl Stream<Item = KadQueryEvent<Vec<PeerId>>, Error = IoError> + 'a
    where F: FnMut(&PeerId) -> Fut + Send + 'a,
        Fut: IntoFuture<Item = KadConnecController, Error = IoError>  + 'a,
        Fut::Future: Send,
    {
        query(access, &self.kbuckets, searched_key, self.parallelism as usize,
              20, self.request_timeout)  // TODO: arbitrary const
    }
}

// Refreshes a specific bucket by performing an iterative `FIND_NODE` on a random ID of this
// bucket.
//
// Returns a dummy no-op future if `bucket_num` is out of range.
fn refresh<'a, F, Fut>(bucket_num: usize, access: F, kbuckets: &KBucketsTable<PeerId, ()>,
                        parallelism: usize, request_timeout: Duration)
    -> impl Stream<Item = KadQueryEvent<()>, Error = IoError> + 'a
where F: FnMut(&PeerId) -> Fut + Send + 'a,
    Fut: IntoFuture<Item = KadConnecController, Error = IoError> + 'a,
    Fut::Future: Send,
{
    let peer_id = match gen_random_id(kbuckets.my_id(), bucket_num) {
        Ok(p) => p,
        Err(()) => {
            let stream = stream::once(Ok(KadQueryEvent::Finished(())));
            return Box::new(stream) as Box<Stream<Item = _, Error = _> + Send>;
        },
    };

    let stream = query(access, kbuckets, peer_id, parallelism, 20, request_timeout)        // TODO: 20 is arbitrary
        .map(|event| {
            match event {
                KadQueryEvent::NewKnownMultiaddrs(peers) => KadQueryEvent::NewKnownMultiaddrs(peers),
                KadQueryEvent::Finished(_) => KadQueryEvent::Finished(()),
            }
        });
    Box::new(stream) as Box<Stream<Item = _, Error = _> + Send>
}

// Generates a random `PeerId` that belongs to the given bucket.
//
// Returns an error if `bucket_num` is out of range.
fn gen_random_id(my_id: &PeerId, bucket_num: usize) -> Result<PeerId, ()> {
    let my_id_len = my_id.as_bytes().len();

    // TODO: this 2 is magic here ; it is the length of the hash of the multihash
    let bits_diff = bucket_num + 1;
    if bits_diff > 8 * (my_id_len - 2) {
        return Err(());
    }

    let mut random_id = [0; 64];
    for byte in 0..my_id_len {
        match byte.cmp(&(my_id_len - bits_diff / 8 - 1)) {
            Ordering::Less => {
                random_id[byte] = my_id.as_bytes()[byte];
            }
            Ordering::Equal => {
                let mask: u8 = (1 << (bits_diff % 8)) - 1;
                random_id[byte] = (my_id.as_bytes()[byte] & !mask) | (rand::random::<u8>() & mask);
            }
            Ordering::Greater => {
                random_id[byte] = rand::random();
            }
        }
    }

    let peer_id = PeerId::from_bytes(random_id[..my_id_len].to_owned())
        .expect("randomly-generated peer ID should always be valid");
    Ok(peer_id)
}

// Generic query-performing function.
fn query<'a, F, Fut>(
    access: F,
    kbuckets: &KBucketsTable<PeerId, ()>,
    searched_key: PeerId,
    parallelism: usize,
    num_results: usize,
    request_timeout: Duration,
) -> impl Stream<Item = KadQueryEvent<Vec<PeerId>>, Error = IoError> + 'a
where F: FnMut(&PeerId) -> Fut + 'a,
      Fut: IntoFuture<Item = KadConnecController, Error = IoError> + 'a,
      Fut::Future: Send,
{
    debug!("Start query for {:?} ; num results = {}", searched_key, num_results);

    // State of the current iterative process.
    struct State<'a, F> {
        // At which stage we are.
        stage: Stage,
        // The `access` parameter.
        access: F,
        // Final output of the iteration.
        result: Vec<PeerId>,
        // For each open connection, a future with the response of the remote.
        // Note that don't use a `SmallVec` here because `select_all` produces a `Vec`.
        current_attempts_fut: Vec<Box<Future<Item = Vec<protocol::KadPeer>, Error = IoError> + Send + 'a>>,
        // For each open connection, the peer ID that we are connected to.
        // Must always have the same length as `current_attempts_fut`.
        current_attempts_addrs: SmallVec<[PeerId; 32]>,
        // Nodes that need to be attempted.
        pending_nodes: Vec<PeerId>,
        // Peers that we tried to contact but failed.
        failed_to_contact: FnvHashSet<PeerId>,
    }

    // General stage of the state.
    #[derive(Copy, Clone, PartialEq, Eq)]
    enum Stage {
        // We are still in the first step of the algorithm where we try to find the closest node.
        FirstStep,
        // We are contacting the k closest nodes in order to fill the list with enough results.
        SecondStep,
        // The results are complete, and the next stream iteration will produce the outcome.
        FinishingNextIter,
        // We are finished and the stream shouldn't return anything anymore.
        Finished,
    }

    let initial_state = State {
        stage: Stage::FirstStep,
        access: access,
        result: Vec::with_capacity(num_results),
        current_attempts_fut: Vec::new(),
        current_attempts_addrs: SmallVec::new(),
        pending_nodes: kbuckets.find_closest(&searched_key).collect(),
        failed_to_contact: Default::default(),
    };

    // Start of the iterative process.
    let stream = stream::unfold(initial_state, move |mut state| -> Option<_> {
        match state.stage {
            Stage::FinishingNextIter => {
                let result = mem::replace(&mut state.result, Vec::new());
                debug!("Query finished with {} results", result.len());
                state.stage = Stage::Finished;
                let future = future::ok((Some(KadQueryEvent::Finished(result)), state));
                return Some(future::Either::A(future));
            },
            Stage::Finished => {
                return None;
            },
            _ => ()
        };

        let searched_key = searched_key.clone();

        // Find out which nodes to contact at this iteration.
        let to_contact = {
            let wanted_len = if state.stage == Stage::FirstStep {
                parallelism.saturating_sub(state.current_attempts_fut.len())
            } else {
                num_results.saturating_sub(state.current_attempts_fut.len())
            };
            let mut to_contact = SmallVec::<[_; 16]>::new();
            while to_contact.len() < wanted_len && !state.pending_nodes.is_empty() {
                // Move the first element of `pending_nodes` to `to_contact`, but ignore nodes that
                // are already part of the results or of a current attempt or if we failed to
                // contact it before.
                let peer = state.pending_nodes.remove(0);
                if state.result.iter().any(|p| p == &peer) {
                    continue;
                }
                if state.current_attempts_addrs.iter().any(|p| p == &peer) {
                    continue;
                }
                if state.failed_to_contact.iter().any(|p| p == &peer) {
                    continue;
                }
                to_contact.push(peer);
            }
            to_contact
        };

        debug!("New query round ; {} queries in progress ; contacting {} new peers",
               state.current_attempts_fut.len(),
               to_contact.len());

        // For each node in `to_contact`, start an RPC query and a corresponding entry in the two
        // `state.current_attempts_*` fields.
        for peer in to_contact {
            let searched_key2 = searched_key.clone();
            let current_attempt = (state.access)(&peer)
                .into_future()
                .and_then(move |controller| {
                    controller.find_node(&searched_key2)
                });
            let with_deadline = Timeout::new(current_attempt, request_timeout)
                .map_err(|err| {
                    if let Some(err) = err.into_inner() {
                        err
                    } else {
                        IoError::new(IoErrorKind::ConnectionAborted, "kademlia request timeout")
                    }
                });
            state.current_attempts_addrs.push(peer.clone());
            state
                .current_attempts_fut
                .push(Box::new(with_deadline) as Box<_>);
        }
        debug_assert_eq!(
            state.current_attempts_addrs.len(),
            state.current_attempts_fut.len()
        );

        // Extract `current_attempts_fut` so that we can pass it to `select_all`. We will push the
        // values back when inside the loop.
        let current_attempts_fut = mem::replace(&mut state.current_attempts_fut, Vec::new());
        if current_attempts_fut.is_empty() {
            // If `current_attempts_fut` is empty, then `select_all` would panic. It happens
            // when we have no additional node to query.
            debug!("Finishing query early because no additional node available");
            state.stage = Stage::FinishingNextIter;
            let future = future::ok((None, state));
            return Some(future::Either::A(future));
        }

        // This is the future that continues or breaks the `loop_fn`.
        let future = future::select_all(current_attempts_fut.into_iter()).then(move |result| {
            let (message, trigger_idx, other_current_attempts) = match result {
                Err((err, trigger_idx, other_current_attempts)) => {
                    (Err(err), trigger_idx, other_current_attempts)
                }
                Ok((message, trigger_idx, other_current_attempts)) => {
                    (Ok(message), trigger_idx, other_current_attempts)
                }
            };

            // Putting back the extracted elements in `state`.
            let remote_id = state.current_attempts_addrs.remove(trigger_idx);
            debug_assert!(state.current_attempts_fut.is_empty());
            state.current_attempts_fut = other_current_attempts;

            // `message` contains the reason why the current future was woken up.
            let closer_peers = match message {
                Ok(msg) => msg,
                Err(err) => {
                    trace!("RPC query failed for {:?}: {:?}", remote_id, err);
                    state.failed_to_contact.insert(remote_id);
                    return future::ok((None, state));
                }
            };

            // Inserting the node we received a response from into `state.result`.
            // The code is non-trivial because `state.result` is ordered by distance and is limited
            // by `num_results` elements.
            if let Some(insert_pos) = state.result.iter().position(|e| {
                e.distance_with(&searched_key) >= remote_id.distance_with(&searched_key)
            }) {
                if state.result[insert_pos] != remote_id {
                    if state.result.len() >= num_results {
                        state.result.pop();
                    }
                    state.result.insert(insert_pos, remote_id);
                }
            } else if state.result.len() < num_results {
                state.result.push(remote_id);
            }

            // The loop below will set this variable to `true` if we find a new element to put at
            // the top of the result. This would mean that we have to continue looping.
            let mut local_nearest_node_updated = false;

            // Update `state` with the actual content of the message.
            let mut new_known_multiaddrs = Vec::with_capacity(closer_peers.len());
            for mut peer in closer_peers {
                // Update the peerstore with the information sent by
                // the remote.
                {
                    let multiaddrs = mem::replace(&mut peer.multiaddrs, Vec::new());
                    trace!("Reporting multiaddresses for {:?}: {:?}", peer.node_id, multiaddrs);
                    new_known_multiaddrs.push((peer.node_id.clone(), multiaddrs));
                }

                if peer.node_id.distance_with(&searched_key)
                    <= state.result[0].distance_with(&searched_key)
                {
                    local_nearest_node_updated = true;
                }

                if state.result.iter().any(|ma| ma == &peer.node_id) {
                    continue;
                }

                // Insert the node into `pending_nodes` at the right position, or do not
                // insert it if it is already in there.
                if let Some(insert_pos) = state.pending_nodes.iter().position(|e| {
                    e.distance_with(&searched_key) >= peer.node_id.distance_with(&searched_key)
                }) {
                    if state.pending_nodes[insert_pos] != peer.node_id {
                        state.pending_nodes.insert(insert_pos, peer.node_id.clone());
                    }
                } else {
                    state.pending_nodes.push(peer.node_id.clone());
                }
            }

            if state.result.len() >= num_results
                || (state.stage != Stage::FirstStep && state.current_attempts_fut.is_empty())
            {
                state.stage = Stage::FinishingNextIter;

            } else {
                if !local_nearest_node_updated {
                    trace!("Loop didn't update closer node ; jumping to step 2");
                    state.stage = Stage::SecondStep;
                }
            }

            future::ok((Some(KadQueryEvent::NewKnownMultiaddrs(new_known_multiaddrs)), state))
        });

        Some(future::Either::B(future))
    }).filter_map(|val| val);

    // Boxing the stream is not necessary, but we do it in order to improve compilation time.
    Box::new(stream) as Box<_>
}
