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

//! High-level structs/traits of the crate.
//!
//! Lies on top of the `kad_server` module.

use bytes::Bytes;
use fnv::FnvHashMap;
use futures::sync::oneshot;
use futures::{self, future, Future, Stream};
use kad_server::{KademliaServerConfig, KademliaServerController, KademliaIncomingRequest, KademliaFindNodeRespond};
use kbucket::{KBucketsPeerId, KBucketsTable, UpdateOutcome};
use libp2p_core::{ConnectionUpgrade, Endpoint, MuxedTransport, PeerId, SwarmController, Transport};
use multiaddr::{AddrComponent, Multiaddr};
use parking_lot::Mutex;
use protocol::Peer;
use query;
use std::collections::hash_map::Entry;
use std::fmt;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use std::slice::Iter as SliceIter;
use std::sync::Arc;
use std::time::Duration;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer;

/// Prototype for a future Kademlia protocol running on a socket.
#[derive(Debug, Clone)]
pub struct KademliaConfig {
    /// Degree of parallelism on the network. Often called `alpha` in technical papers.
    /// No more than this number of remotes will be used at a given time for any given operation.
    // TODO: ^ share this number between operations? or does each operation use `alpha` remotes?
    pub parallelism: u32,
    /// Id of the local peer.
    pub local_peer_id: PeerId,
    /// When contacting a node, duration after which we consider it unresponsive.
    pub timeout: Duration,
}

// Builds a `QueryParams` that fetches information from `$controller`.
//
// Because of lifetime issues and type naming issues, a macro is the most convenient solution.
macro_rules! gen_query_params {
    ($controller:expr) => {{
        let controller = $controller;
        query::QueryParams {
            local_id: $controller.inner.kbuckets.my_id().clone(),
            kbuckets_find_closest: {
                let controller = controller.clone();
                move |addr| controller.inner.kbuckets.find_closest(&addr).collect()
            },
            parallelism: $controller.inner.parallelism,
            find_node: {
                let controller = controller.clone();
                move |addr, searched| {
                    // TODO: rewrite to be more direct
                    Box::new(controller.send(addr, move |ctl| ctl.find_node(&searched)).flatten()) as Box<_>
                }
            },
        }
    }};
}

/// Object that allows one to make queries on the Kademlia system.
#[derive(Debug)]
pub struct KademliaControllerPrototype {
    inner: Arc<Inner>,
}

impl KademliaControllerPrototype {
    /// Creates a new controller from that configuration.
    pub fn new<I>(config: KademliaConfig, initial_peers: I) -> KademliaControllerPrototype
    where I: IntoIterator<Item = PeerId>
    {
        let buckets = KBucketsTable::new(config.local_peer_id.clone(), config.timeout);
        for peer_id in initial_peers {
            let _ = buckets.update(peer_id, ());
        }

        let inner = Arc::new(Inner {
            kbuckets: buckets,
            timer: tokio_timer::wheel().build(),
            connections: Default::default(),
            timeout: config.timeout,
            parallelism: config.parallelism as usize,
        });

        KademliaControllerPrototype { inner: inner }
    }

    /// Turns the prototype into an actual controller by feeding it a swarm controller.
    ///
    /// You must pass to this function the transport to use to dial and obtain 
    /// `KademliaPeerReqStream`, plus a mapping function that will turn the
    /// `KademliaPeerReqStream` into whatever the swarm expects.
    pub fn start<T, K, M>(
        self,
        swarm: SwarmController<T>,
        kademlia_transport: K,
        map: M,
    ) -> (
        KademliaController<T, K, M>,
        Box<Future<Item = (), Error = IoError>>,
    )
    where
        T: Clone + MuxedTransport + 'static, // TODO: 'static :-/
        K: Transport<Output = KademliaPeerReqStream> + Clone + 'static, // TODO: 'static :-/
        M: FnOnce(KademliaPeerReqStream) -> T::Output + Clone + 'static,
    {
        // TODO: initialization

        let controller = KademliaController {
            inner: self.inner.clone(),
            swarm_controller: swarm,
            kademlia_transport,
            map,
        };

        let init_future = {
            let futures: Vec<_> = (0..256)
                .map({
                    let controller = controller.clone();
                    move |n| query::refresh(gen_query_params!(controller.clone()), n)
                })
                .map(|stream| {
                    stream.for_each(|_| Ok(()))
                })
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
        };

        (controller, Box::new(init_future))
    }
}

/// Object that allows one to make queries on the Kademlia system.
#[derive(Debug)]
pub struct KademliaController<T, K, M>
where
    T: MuxedTransport + 'static, // TODO: 'static :-/
{
    inner: Arc<Inner>,
    swarm_controller: SwarmController<T>,
    kademlia_transport: K,
    map: M,
}

impl<T, K, M> Clone for KademliaController<T, K, M>
where
    T: Clone + MuxedTransport + 'static, // TODO: 'static :-/
    K: Clone,
    M: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        KademliaController {
            inner: self.inner.clone(),
            swarm_controller: self.swarm_controller.clone(),
            kademlia_transport: self.kademlia_transport.clone(),
            map: self.map.clone(),
        }
    }
}

impl<T, K, M> KademliaController<T, K, M>
where
    T: Clone + MuxedTransport + 'static, // TODO: 'static :-/
{
    /// Performs an iterative find node query on the network.
    ///
    /// Will query the network for the peers that4 are the closest to `searched_key` and return
    /// the results.
    ///
    /// The algorithm used is a standard Kademlia algorithm. The details are not documented, so
    /// that the implementation is free to modify them.
    #[inline]
    pub fn find_node(
        &self,
        searched_key: PeerId,
    ) -> Box<Stream<Item = query::QueryEvent<Vec<PeerId>>, Error = IoError>>
    where
        K: Transport<Output = KademliaPeerReqStream> + Clone + 'static,
        M: FnOnce(KademliaPeerReqStream) -> T::Output + Clone + 'static,     // TODO: 'static :-/
    {
        let me = self.clone();
        query::find_node(gen_query_params!(me.clone()), searched_key)
    }
}

/// Connection upgrade to the Kademlia protocol.
#[derive(Clone)]
pub struct KademliaUpgrade {
    inner: Arc<Inner>,
    upgrade: KademliaServerConfig,
}

impl KademliaUpgrade {
    /// Builds a connection upgrade from the controller.
    #[inline]
    pub fn from_prototype(proto: &KademliaControllerPrototype) -> Self {
        KademliaUpgrade {
            inner: proto.inner.clone(),
            upgrade: KademliaServerConfig::new(),
        }
    }

    /// Builds a connection upgrade from the controller.
    #[inline]
    pub fn from_controller<T, K, M>(ctl: &KademliaController<T, K, M>) -> Self
    where
        T: MuxedTransport,
    {
        KademliaUpgrade {
            inner: ctl.inner.clone(),
            upgrade: KademliaServerConfig::new(),
        }
    }
}

impl<C> ConnectionUpgrade<C> for KademliaUpgrade
where
    C: AsyncRead + AsyncWrite + 'static,     // TODO: 'static :-/
{
    type Output = KademliaPeerReqStream;
    type Future = Box<Future<Item = Self::Output, Error = IoError>>;
    type NamesIter = iter::Once<(Bytes, ())>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        ConnectionUpgrade::<C>::protocol_names(&self.upgrade)
    }

    #[inline]
    fn upgrade(self, incoming: C, id: (), endpoint: Endpoint, addr: &Multiaddr) -> Self::Future {
        let inner = self.inner;
        let client_addr = addr.clone();

        let peer_id = {
            let mut iter = addr.iter();
            let protocol = iter.next();
            let after_proto = iter.next();
            match (protocol, after_proto) {
                (Some(AddrComponent::P2P(key)), None) | (Some(AddrComponent::IPFS(key)), None) => {
                    match PeerId::from_bytes(key) {
                        Ok(id) => id,
                        Err(_) => {
                            let err = IoError::new(
                                IoErrorKind::InvalidData,
                                "invalid peer ID sent by remote identification",
                            );
                            return Box::new(future::err(err));
                        }
                    }
                }
                _ => {
                    let err =
                        IoError::new(IoErrorKind::InvalidData, "couldn't identify connected node");
                    return Box::new(future::err(err));
                }
            }
        };

        let future = self.upgrade.upgrade(incoming, id, endpoint, addr).map(
            move |(controller, stream)| {
                match inner.connections.lock().entry(client_addr) {
                    Entry::Occupied(mut entry) => {
                        match entry.insert(Connection::Active(controller)) {
                            // If there was already an active connection to this remote, it gets
                            // replaced by the new more recent one.
                            Connection::Active(_old_connection) => {}
                            Connection::Pending(closures) => {
                                let new_ctl = match entry.get_mut() {
                                    &mut Connection::Active(ref mut ctl) => ctl,
                                    _ => unreachable!(
                                        "logic error: an Active enum variant was \
                                         inserted, but reading back didn't give \
                                         an Active"
                                    ),
                                };

                                for mut closure in closures {
                                    closure(new_ctl);
                                }
                            }
                        };
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(Connection::Active(controller));
                    }
                };

                let stream = stream.map(move |query| {
                    match inner.kbuckets.update(peer_id.clone(), ()) {
                        UpdateOutcome::NeedPing(node_to_ping) => {
                            // TODO: do something with this info
                            println!("need to ping {:?}", node_to_ping);
                        }
                        _ => (),
                    }

                    match query {
                        KademliaIncomingRequest::FindNode { searched, responder } => {
                            let mut intermediate: Vec<_> = inner.kbuckets.find_closest(&searched).collect();
                            let my_id = inner.kbuckets.my_id().clone();
                            if let Some(pos) = intermediate
                                .iter()
                                .position(|e| e.distance_with(&searched) >= my_id.distance_with(&searched))
                            {
                                if intermediate[pos] != my_id {
                                    intermediate.insert(pos, my_id);
                                }
                            } else {
                                intermediate.push(my_id);
                            }

                            Some(KademliaPeerReq {
                                requested_peers: intermediate,
                                inner: responder,
                            })
                        },
                        KademliaIncomingRequest::PingPong => {
                            // We updated the k-bucket above.
                            None
                        },
                    }
                }).filter_map(|val| val);

                KademliaPeerReqStream { inner: Box::new(stream) }
            },
        );

        Box::new(future) as Box<_>
    }
}

/// Stream that must be processed for the Kademlia system to work.
///
/// Produces requests for peer information. These requests should be answered for the stream to
/// continue to progress.
pub struct KademliaPeerReqStream {
    inner: Box<Stream<Item = KademliaPeerReq, Error = IoError>>,
}

impl Stream for KademliaPeerReqStream {
    type Item = KademliaPeerReq;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> futures::Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}

/// Request for information about some peers.
pub struct KademliaPeerReq {
    inner: KademliaFindNodeRespond,
    requested_peers: Vec<PeerId>,
}

impl KademliaPeerReq {
    /// Returns a list of the IDs of the peers that were requested.
    #[inline]
    pub fn requested_peers(&self) -> SliceIter<PeerId> {
        self.requested_peers.iter()
    }

    /// Responds to the request.
    #[inline]
    pub fn respond<I>(self, peers: I)
        where I: IntoIterator<Item = Peer>
    {
        self.inner.respond(peers);
    }
}

// Inner struct shared throughout the Kademlia system.
#[derive(Debug)]
struct Inner {
    // The remotes are identified by their public keys.
    kbuckets: KBucketsTable<PeerId, ()>,

    // Timer used for building the timeouts.
    timer: tokio_timer::Timer,

    // Same as in the config.
    timeout: Duration,

    // Same as in the config.
    parallelism: usize,

    // List of open connections with remotes.
    //
    // Since the keys are the nodes' multiaddress, it is expected that each node only has one
    // multiaddress. This should be the case if the user uses the identify transport that
    // automatically maps peer IDs to multiaddresses.
    // TODO: is it correct to use FnvHashMap with a Multiaddr? needs benchmarks
    connections: Mutex<FnvHashMap<Multiaddr, Connection>>,
}

// Current state of a connection to a specific multiaddr.
//
// There is no `Inactive` entry, as an inactive connection corresponds to no entry in the
// `connections` hash map.
enum Connection {
    // The connection has already been opened and is ready to be controlled through the given
    // controller.
    Active(KademliaServerController),

    // The connection is in the process of being opened. Any closure added to this `Vec` will be
    // executed on the controller once it is available.
    // Once the connection is open, it will be replaced with `Active`.
    // TODO: should be FnOnce once Rust allows that
    Pending(Vec<Box<FnMut(&mut KademliaServerController)>>),
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Connection::Active(_) => write!(f, "Connection::Active"),
            Connection::Pending(_) => write!(f, "Connection::Pending"),
        }
    }
}

impl<T, K, M> KademliaController<T, K, M>
where
    T: Clone + MuxedTransport + 'static, // TODO: 'static :-/
    K: Transport<Output = KademliaPeerReqStream> + Clone + 'static,      // TODO: 'static
    M: FnOnce(KademliaPeerReqStream) -> T::Output + Clone + 'static,     // TODO: 'static :-/
{
    #[inline]
    fn send<F, FRet>(
        &self,
        addr: Multiaddr,
        and_then: F,
    ) -> Box<Future<Item = FRet, Error = IoError>>
    where
        F: FnOnce(&KademliaServerController) -> FRet + 'static,
        FRet: 'static,
    {
        let mut lock = self.inner.connections.lock();

        let pending_list = match lock.entry(addr.clone()) {
            Entry::Occupied(entry) => {
                match entry.into_mut() {
                    &mut Connection::Pending(ref mut list) => list,
                    &mut Connection::Active(ref mut ctrl) => {
                        // If we have an active connection, entirely short-circuit the function.
                        let output = future::ok(and_then(ctrl));
                        return Box::new(output) as Box<_>;
                    }
                }
            }
            Entry::Vacant(entry) => {
                // Need to open a connection.
                let map = self.map.clone();
                match self.swarm_controller
                    .dial(addr, self.kademlia_transport.clone().map(move |out, _, _| map(out)))
                {
                    Ok(()) => (),
                    Err(_addr) => {
                        let fut = future::err(IoError::new(
                            IoErrorKind::InvalidData,
                            "unsupported multiaddress",
                        ));
                        return Box::new(fut) as Box<_>;
                    }
                }
                match entry.insert(Connection::Pending(Vec::with_capacity(1))) {
                    &mut Connection::Pending(ref mut list) => list,
                    _ => unreachable!("we just inserted a Pending variant"),
                }
            }
        };

        let (tx, rx) = oneshot::channel();
        let mut tx = Some(tx);
        let mut and_then = Some(and_then);
        pending_list.push(Box::new(move |ctrl: &mut KademliaServerController| {
            let and_then = and_then
                .take()
                .expect("Programmer error: 'pending' closure was called multiple times");
            let tx = tx.take()
                .expect("Programmer error: 'pending' closure was called multiple times");
            let ret = and_then(ctrl);
            let _ = tx.send(ret);
        }) as Box<_>);

        let future = rx.map_err(|_| IoErrorKind::ConnectionAborted.into());
        let future = self.inner.timer.timeout(future, self.inner.timeout);
        Box::new(future) as Box<_>
    }
}
