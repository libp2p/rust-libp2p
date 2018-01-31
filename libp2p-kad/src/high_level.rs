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

use bytes::Bytes;
use error::KadError;
use fnv::FnvHashMap;
use futures::{self, future, stream, Future, IntoFuture, Sink, Stream};
use futures::sync::{mpsc, oneshot};
use kad_server::{KademliaServerController, KademliaServerConfig, KadServerInterface};
use kbucket::{KBucketsPeerId, KBucketsTable, UpdateOutcome};
use libp2p_identify::{IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};
use libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
use libp2p_ping::{Ping, Pinger};
use libp2p_swarm::{self, Endpoint, MuxedTransport, OrUpgrade, SwarmController, UpgradeExt};
use libp2p_swarm::{ConnectionUpgrade, UpgradedNode};
use libp2p_swarm::transport::EitherSocket;
use multiaddr::Multiaddr;
use parking_lot::Mutex;
use protocol::{self, KademliaProtocolConfig, KadMsg, Peer};
use smallvec::SmallVec;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use std::mem;
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer;
use with_some::WithSome;

/// Prototype for a future Kademlia protocol running on a socket.
#[derive(Debug, Clone)]
pub struct KademliaConfig<P, R> {
	/// Degree of parallelism on the network. Often called `alpha` in technical papers.
	/// No more than this number of remotes will be used at a given time for any given operation.
	// TODO: ^ share this number between operations? or does each operation use `alpha` remotes?
	pub parallelism: u32,
	/// Used to load and store data requests of peers. TODO: say that must implement the `Recordstore` trait.
	pub record_store: R,
	/// Used to load and store information about peers.
	pub peer_store: P,
	/// Id of the local peer.
	pub local_peer_id: PeerId,
	/// The Kademlia system uses cycles. This is the duration of one cycle.
	pub cycles_duration: Duration,
	/// When pinging a node, duration after which we consider that it doesn't respond.
	pub timeout: Duration,
}

/// Object that allows one to make queries on the Kademlia system.
#[derive(Debug)]
pub struct KademliaControllerPrototype<P, R> {
    inner: Arc<Inner<P, R>>,
}

impl<P, R> KademliaControllerPrototype<P, R> {
    pub fn start<T, C>(self, swarm: SwarmController<T, C>) -> KademliaController<P, R>
        where T: MuxedTransport,
              C: ConnectionUpgrade<T::RawConn>,
    {
        // TODO: initialization

        KademliaController {
            inner: self.inner.clone(),
        }
    }
}

/// Object that allows one to make queries on the Kademlia system.
#[derive(Debug)]
pub struct KademliaController<P, R> {
    inner: Arc<Inner<P, R>>,
}

impl<P, R> Clone for KademliaController<P, R> {
    #[inline]
    fn clone(&self) -> Self {
        KademliaController {
            inner: self.inner.clone()
        }
    }
}

impl<P, R> KademliaController<P, R>
    where P: Peerstore + Clone,
          R: Clone,
{
    /// Creates a new controller from that configuration.
    pub fn new(config: KademliaConfig<P, R>) -> KademliaController<P, R> {
		let buckets = KBucketsTable::new(config.local_peer_id.clone(), config.timeout);
		for peer_id in config.peer_store.clone().peers() {
			let _ = buckets.update(peer_id, ());
		}

		let inner = Arc::new(Inner {
			kbuckets: buckets,
			timer: tokio_timer::wheel().build(),
			record_store: config.record_store,
			peer_store: config.peer_store,
			connections: Default::default(),
            timeout: config.timeout,
		});

        KademliaController {
            inner: inner,
        }
    }
}

/// Connection upgrade to the Kademlia protocol.
#[derive(Debug, Clone)]
pub struct KademliaUpgrade<P, R> {
    inner: Arc<Inner<P, R>>,
    upgrade: KademliaServerConfig<Arc<Inner<P, R>>>,
}

impl<P, R> KademliaUpgrade<P, R> {
    /// Builds a connection upgrade from the controller.
    #[inline]
    pub fn new(proto: &KademliaControllerPrototype<P, R>) -> Self {
        KademliaUpgrade {
            inner: proto.inner.clone(),
            upgrade: KademliaServerConfig::new(proto.inner.clone()),
        }
    }
}

impl<C, P, R> ConnectionUpgrade<C> for KademliaUpgrade<P, R>
where
	C: AsyncRead + AsyncWrite + 'static, // TODO: 'static :-/
    P: Peerstore + Clone + 'static,     // TODO: 'static :-/
    R: 'static          // TODO: 'static :-/
{
	type Output = Box<Future<Item = (), Error = IoError>>;
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

        let future = self.upgrade
            .upgrade(incoming, id, endpoint, addr)
            .map(move |(controller, future)| {
                inner.connections.lock().insert(client_addr, controller);
                future
            });

        Box::new(future) as Box<_>
	}
}

// Inner struct shared throughout the Kademlia system.
#[derive(Debug)]
struct Inner<P, R> {
	// The remotes are identified by their public keys.
	kbuckets: KBucketsTable<PeerId, ()>,

	// Timer used for building the timeouts.
	timer: tokio_timer::Timer,

    // The timeout of the responses.
    timeout: Duration,

	// Same fields as `KademliaConfig`.
	record_store: R,
	peer_store: P,

	// List of open connections with remotes.
	// TODO: is it correct to use FnvHashMap with a Multiaddr? needs benchmarks
	connections: Mutex<FnvHashMap<Multiaddr, KademliaServerController>>,
}

impl<P, R> KadServerInterface for Arc<Inner<P, R>>
    where P: Peerstore + Clone,     // TODO: wrong
{
	type Peerstore = P;
	type RecordStore = R;

    #[inline]
	fn local_id(&self) -> &PeerId {
        self.kbuckets.my_id()
    }

    #[inline]
	fn kbuckets_update(&self, peer: PeerId) {
		// TODO: is this the right place for this check?
		if &peer == self.kbuckets.my_id() {
			return;
		}

		match self.kbuckets.update(peer, ()) {
			UpdateOutcome::NeedPing(node_to_ping) => {
				// TODO: return this info somehow
				println!("need to ping {:?}", node_to_ping);
			}
			_ => (),
		}
    }

	fn kbuckets_find_closest(&self, addr: &PeerId) -> Vec<PeerId> {
		self.kbuckets.find_closest(addr).collect()
    }

	fn peer_store(&self) -> Self::Peerstore {
        unimplemented!()        // TODO:
    }

	fn record_store(&self) -> Self::RecordStore {
        unimplemented!()        // TODO:
    }
}

// Sends a message to a specific multiaddress, and returns a response with a timeout.
#[inline]
fn send<P, R, F, Fu>(inner: &Inner<P, R>, addr: Multiaddr, closure: F)
                     -> tokio_timer::Timeout<Fu::Future>
    where F: FnOnce(&KademliaServerController) -> Fu,
        Fu: IntoFuture,
        Fu::Error: From<tokio_timer::TimeoutError<Fu::Future>>
{
    let mut lock = inner.connections.lock();

    let controller = lock.entry(addr.clone()).or_insert_with(move || {
        // TODO:
        //inner.swarm.dial_to_handler(addr, upgrade); // TODO: how to handle errs?
        unimplemented!()
    });

    let future = closure(controller);
    inner.timer.timeout(future.into_future(), inner.timeout)
}
