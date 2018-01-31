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

use bytes::Bytes;
use error::KadError;
use fnv::FnvHashMap;
use futures::{self, future, stream, Future, IntoFuture, Sink, Stream};
use futures::sync::{mpsc, oneshot};
use kbucket::{KBucketsPeerId, KBucketsTable, UpdateOutcome};
use libp2p_identify::{IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};
use libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
use libp2p_ping::{Ping, Pinger};
use libp2p_swarm::{self, Endpoint, MuxedTransport, OrUpgrade, SwarmController, UpgradeExt};
use libp2p_swarm::{ConnectionUpgrade, UpgradedNode};
use libp2p_swarm::transport::EitherSocket;
use multiaddr::Multiaddr;
use parking_lot::Mutex;
use protocol::{self, KademliaProtocolConfig, KadMsg};
use query::{self, QueryInterface};
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

/// Interface that the query uses to communicate with the rest of the system.
pub trait KadServerInterface: Clone {
	/// The `Peerstore` object where the query will load and store information about nodes.
	type Peerstore: Peerstore + Clone;
	/// The record store to use for `FIND_VALUE` queries.
	type RecordStore: Clone;
	/// Future that yields the Kademlia message that a remote will answer as part of a sent
	/// message.
	type Outcome: Future<Item = protocol::KadMsg, Error = ()>;

	/// Returns the peer ID of the local node.
	fn local_id(&self) -> &PeerId;

	/// Updates an entry in the K-Buckets. Called whenever that peer sends us a message.
	fn kbuckets_update(&self, peer: PeerId);

	/// Finds the nodes closest to a peer ID.
	fn kbuckets_find_closest(&self, addr: &PeerId) -> Vec<PeerId>;

	/// Grants access to the peerstore to use for this query.
	fn peer_store(&self) -> Self::Peerstore;

	/// Grants access to the recordstore to use for this query.
	fn record_store(&self) -> Self::RecordStore;
}

/// Configuration for a Kademlia server.
///
/// Implements `ConnectionUpgrade`. On a successful upgrade, produces a `KademliaServerController`
/// and a `Future`. The controller lets you send queries to the remote and receive answers, while
/// the `Future` must be driven to completion in order for things to work.
#[derive(Debug, Clone)]
pub struct KademliaServerConfig<I> {
	raw_proto: KademliaProtocolConfig,
	interface: I,
}

impl<I> KademliaServerConfig<I> {
	/// Builds a configuration object for an upcoming Kademlia server.
	#[inline]
	pub fn new(interface: I) -> Self {
		KademliaServerConfig {
			raw_proto: KademliaProtocolConfig,
			interface: interface,
		}
	}
}

impl<C, I> ConnectionUpgrade<C> for KademliaServerConfig<I>
where
	C: AsyncRead + AsyncWrite + 'static, // TODO: 'static :-/
	I: KadServerInterface + 'static,
{
	type Output = (KademliaServerController, Box<Future<Item = (), Error = IoError>>);
	type Future = Box<Future<Item = Self::Output, Error = IoError>>;
	type NamesIter = iter::Once<(Bytes, ())>;
	type UpgradeIdentifier = ();

	#[inline]
	fn protocol_names(&self) -> Self::NamesIter {
        ConnectionUpgrade::<C>::protocol_names(&self.raw_proto)
	}

	#[inline]
	fn upgrade(self, incoming: C, id: (), endpoint: Endpoint, addr: &Multiaddr) -> Self::Future {
		let interface = self.interface;
		let future = self.raw_proto.upgrade(incoming, id, endpoint, addr)
            .map(move |connec| {
				let (tx, rx) = mpsc::unbounded();
                let future = kademlia_handler(connec, rx, interface);
				let controller = KademliaServerController { inner: tx };
				(controller, future)
            });

		Box::new(future) as Box<_>
	}
}

/// Allows sending Kademlia requests and receiving responses.
pub struct KademliaServerController {
	inner: mpsc::UnboundedSender<(KadMsg, oneshot::Sender<KadMsg>)>,
}

/*impl<'a, R, P, T> KademliaServerConfig<R, P, T>
where
	T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
	P: Peerstore + Clone + 'a,
	R: Clone + 'a,
{
	/// Starts the Kademlia DHT system.
	///
	/// This function returns a tuple that consists in a *controller*, and a future that will drive
	/// the Kademlia process forward. The controller can be used to give instructions to the
	/// Kademlia system.
	pub fn build(
		self,
	) -> (
		KademliaSwarmController<R, P, T>,
		Box<Future<Item = (), Error = IoError> + 'a>,
	) {
		let buckets = KBucketsTable::new(self.local_peer_id.clone(), self.timeout);
		for peer_id in self.peer_store.clone().peers() {
			let _ = buckets.update(peer_id, ());
		}

		let shared = Arc::new(Inner {
			kbuckets: buckets,
			timer: tokio_timer::wheel().build(),
			record_store: self.record_store,
			peer_store: self.peer_store,
			connections: Default::default(),
		});

		let server_upgrades = WithSome::<
			_,
			Option<Arc<Mutex<Option<mpsc::UnboundedReceiver<(KadMsg, oneshot::Sender<KadMsg>)>>>>>,
		>(protocol::KademliaProtocolConfig, None)
			.or_upgrade(IdentifyProtocolConfig)
			.or_upgrade(Ping);

		let (swarm_controller, swarm_future) = {
			let shared = shared.clone();
			libp2p_swarm::swarm(
				self.transport,
				server_upgrades,
				move |upgrade, client_addr, swarm| {
					let shared = shared.clone();
					connection_handler(upgrade, client_addr, shared, swarm)
				},
			)
		};

		let controller = KademliaSwarmController {
			swarm: swarm_controller,
			parallelism: self.parallelism,
			cycles_duration: self.cycles_duration,
			timeout: self.timeout,
			shared: shared.clone(),
		};

		let initialization_process = {
			let controller2 = controller.clone();
			controller
				.find_node(self.local_peer_id.clone())
				.and_then(move |_| {
					let controller2 = controller2.clone();
					let iter = shared
						.clone()
						.kbuckets
						.buckets()
						.enumerate()
						.skip_while(|&(_, ref b)| b.num_entries() == 0)
						.map(|(n, _)| n)
						.collect::<Vec<_>>()
						.into_iter()
						.map(move |bucket_num| query::refresh(controller2.clone(), bucket_num));
					future::join_all(iter)
				})
		};

		let swarm_future = swarm_future
			.select(initialization_process.map(move |_| ()))
			.map_err(|(e, _)| e)
			.and_then(|(_, n)| n);
		(controller, Box::new(swarm_future) as Box<_>)
	}
}
*/

#[derive(Debug)]
struct Inner<R, P> {
	// The remotes are identified by their public keys.
	kbuckets: KBucketsTable<PeerId, ()>,

	// Timer used for building the timeouts.
	timer: tokio_timer::Timer,

	// Same fields as `KademliaServerConfig`.
	record_store: R,
	peer_store: P,

	// List of open connections with remotes.
	// TODO: is it correct to use FnvHashMap? needs benchmarks
	connections:
		Mutex<FnvHashMap<Multiaddr, mpsc::UnboundedSender<(KadMsg, oneshot::Sender<KadMsg>)>>>,
}

/*impl<R, P, T> QueryInterface for KademliaSwarmController<R, P, T>
where
	P: Peerstore + Clone,
	R: Clone,
	T: MuxedTransport + Clone + 'static,
{
	type Peerstore = P;
	type RecordStore = R;
	type Outcome = tokio_timer::Timeout<
		future::MapErr<oneshot::Receiver<KadMsg>, fn(futures::Canceled) -> ()>,
	>;

	#[inline]
	fn local_id(&self) -> &PeerId {
		self.shared.kbuckets.my_id()
	}

	#[inline]
	fn kbuckets_update(&self, peer: PeerId) {
		// TODO: is this the right place for this check?
		if &peer == self.shared.kbuckets.my_id() {
			return;
		}

		match self.shared.kbuckets.update(peer, ()) {
			UpdateOutcome::NeedPing(node_to_ping) => {
				// TODO: return this info somehow
				println!("need to ping {:?}", node_to_ping);
			}
			_ => (),
		}
	}

	#[inline]
	fn kbuckets_find_closest(&self, addr: &PeerId) -> Vec<PeerId> {
		self.shared.kbuckets.find_closest(addr).collect()
	}

	#[inline]
	fn peer_store(&self) -> Self::Peerstore {
		self.shared.peer_store.clone()
	}

	#[inline]
	fn record_store(&self) -> Self::RecordStore {
		self.shared.record_store.clone()
	}

	#[inline]
	fn parallelism(&self) -> usize {
		self.parallelism as usize
	}

	#[inline]
	fn cycle_duration(&self) -> Duration {
		self.cycles_duration
	}

	#[inline]
	fn send(&self, addr: Multiaddr, message: KadMsg) -> Self::Outcome {
		let mut lock = self.shared.connections.lock();
		let sender = lock.entry(addr.clone()).or_insert_with(move || {
			let (tx, rx) = mpsc::unbounded();
			let upgrade = WithSome(
				protocol::KademliaProtocolConfig,
				Some(Arc::new(Mutex::new(Some(rx)))),
			).map_upgrade(EitherSocket::First)
				.map_upgrade(EitherSocket::First);
			self.swarm.dial_to_handler(addr, upgrade); // TODO: how to handle errs?
			tx
		});

		// TODO: empty outcome if the message doesn't expect an answer

		let (resp_tx, resp_rx) = oneshot::channel();
		let _ = sender.unbounded_send((message, resp_tx));
		self.shared
			.timer
			.timeout(resp_rx.map_err(|_| ()), self.timeout)
	}
}

impl<R, P, T> KademliaSwarmController<R, P, T>
    where T: MuxedTransport + Clone + 'static,      // TODO: 'static :-/
          SwarmController<T, OrUpgrade<OrUpgrade<protocol::KademliaProtocolConfig, IdentifyProtocolConfig>, Ping>>: Clone,
          P: Peerstore + Clone,
          R: Clone,
{
    /// Adds a new address to listen on. The processing of this address will be handled by the
    /// future that was returned when starting Kademlia.
    #[inline]
    pub fn listen_on(&self, addr: Multiaddr) -> Result<Multiaddr, Multiaddr> {
        self.swarm.listen_on(addr)
    }

    /// Stores a value in the DHT.
    pub fn store<'a, K, V>(&self, key: K, value: V) -> Box<Future<Item = (), Error = IoError> + 'a>
        where K: Into<PeerId>,
              V: Into<Vec<u8>>,
              P: 'a,
              R: 'a,
    {
        let key = key.into();
        let me = self.clone();

        let future = self.find_node(key.clone())
            .and_then(move |closest_nodes| {
                let closest_nodes = closest_nodes
                    .into_iter()
                    .flat_map(|node| {
                        // TODO: this means that we send a message to all the multiaddrs of a peer
                        me.shared.peer_store.clone()
                            .peer(&node)
                            .into_iter()
                            .flat_map(|n| n.addrs())
                    })
                    .map(|addr| {
                        let message = KadMsg::PutValue {
                            key: key.as_bytes().to_vec(),       // TODO: meh
                            record: ::protobuf_structs::record::Record::new(),      // FIXME:
                        };

                        me.send(addr, message)
                            // Ignore errors if sending failed.
                            .then(|r| Ok(r))
                    })
                    .collect::<Vec<_>>();

                future::loop_fn(closest_nodes, |closest_nodes| {
                    if closest_nodes.is_empty() {
                        return future::Either::A(future::ok(future::Loop::Break(())));
                    }

                    let fut = future::select_all(closest_nodes)
                        .map_err(|(err, _, _)| err)
                        .map(|(_, _, rest)| future::Loop::Continue(rest));
                    future::Either::B(fut)
                })
            });

        Box::new(future) as Box<_>
    }

    /// Finds the nodes that are the closest to a given key.
    pub fn find_node<'a, K>(&self, searched_key: K) -> Box<Future<Item = Vec<PeerId>, Error = IoError> + 'a>
        where K: Into<PeerId>,
              P: 'a,
              R: 'a,
    {
        query::find_node(self.clone(), searched_key.into())
    }
}

type UpgradeResult<R> = EitherSocket<
	EitherSocket<
		(
			Box<
				protocol::KadStreamSink<
					SinkError = KadError,
					Error = KadError,
					Item = KadMsg,
					SinkItem = KadMsg,
				>
					+ 'static,
			>,
			Option<Arc<Mutex<Option<mpsc::UnboundedReceiver<(KadMsg, oneshot::Sender<KadMsg>)>>>>>,
		),
		IdentifyOutput<R>,
	>,
	(Pinger, Box<Future<Item = (), Error = IoError> + 'static>),
>;

// Handles an incoming connection on the swarm.
fn connection_handler<'a, T, R, P>(
	upgrade: UpgradeResult<T::RawConn>,
	client_addr: Multiaddr,
	shared: Arc<Inner<R, P>>,
	swarm: SwarmType<T>,
) -> Box<Future<Item = (), Error = IoError> + 'a>
where
	T: MuxedTransport + Clone,
	P: Peerstore + Clone + 'a,
	R: Clone + 'a,
{
	match upgrade {
		EitherSocket::First(EitherSocket::First((kad_bistream, rx))) => {
			let rx = match rx {
				Some(rx) => rx.lock().take().unwrap(),
				None => {
					let (tx, rx) = mpsc::unbounded();
					shared.connections.lock().insert(client_addr.clone(), tx);
					rx
				}
			};

			kademlia_handler(kad_bistream, rx, shared)
		}
		EitherSocket::First(EitherSocket::Second(identify)) => {
			println!("identify protocol opened");
			if let IdentifyOutput::RemoteInfo { info, .. } = identify {
				println!("identify {:?}", info);
				let id = PeerId::from_public_key(&info.public_key); // TODO: get a PeerId directly?
				shared
					.peer_store
					.clone()
					.peer_or_create(&id)
					.add_addr(client_addr, Duration::from_secs(3600)); // TODO: configurable
			} else {
				println!("requested");
			}

			Box::new(Ok(()).into_future()) as Box<Future<Item = (), Error = IoError>>
		}
		EitherSocket::Second((ping_ctrl, ping_future)) => {
			println!("ping opened");
			Box::new(ping_future) as Box<Future<Item = (), Error = IoError>>
		}
	}
}*/

// Handles a newly-opened Kademlia stream with a remote peer.
//
// Takes a `Stream` and `Sink` of Kademlia messages representing the connection to the client,
// plus a `Receiver` that will receive messages to transmit to that connection, plus an `Inner`
// that will be used to answer the messages.
//
// Returns a `Future` that must be resolved in order for progress to work. It will never yield any
// item but will propagate any error generated by the connection. If the `Receiver` closes, no
// error is generated.
fn kademlia_handler<'a, S, I>(
	kad_bistream: S,
	rx: mpsc::UnboundedReceiver<(KadMsg, oneshot::Sender<KadMsg>)>,
	interface: I,
) -> Box<Future<Item = (), Error = IoError> + 'a>
where
	S: Stream<Item = KadMsg, Error = KadError> + Sink<SinkItem = KadMsg, SinkError = KadError> + 'a,
	I: KadServerInterface + Clone + 'a,
{
	let (kad_sink, kad_stream) = kad_bistream
		.sink_map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
		.map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
		.split();

	// We combine `kad_stream` and `rx` into one so that the loop wakes up whenever either
	// generates something.
	let messages = rx.map(|(m, o)| (m, Some(o)))
		.map_err(|_| unreachable!())
		.select(kad_stream.map(|m| (m, None)));

	// Loop forever.
	let future = future::loop_fn(
		(kad_sink, messages, Vec::new()),
		move |(kad_sink, messages, mut send_back_queue)| {
			// The `send_back_queue` is a queue of `UnboundedSender`s in the correct order.
			// Whenever we send a message to the remote and this message expects a response, we
			// push the sender to the end of `send_back_queue`. Whenever a remote sends us a
			// response, we pop the first element of `send_back_queue`.
			let interface = interface.clone();
			messages
				.into_future()
				.map_err(|(err, _)| err)
				.and_then(move |(message, rest)| {
					if let Some((_, None)) = message {
						// If we received a message from the remote (as opposed to a message from
						// `rx`) then we update the k-buckets.
						// TODO:
					}

					match message {
						None => {
							// Both the connection stream and `rx` are empty, so we break the loop.
							let future = future::ok(future::Loop::Break(()));
							Box::new(future) as Box<Future<Item = _, Error = _>>
						}
						Some((message @ KadMsg::PutValue { .. }, Some(_))) => {
							// A `PutValue` message has been received on `rx`. Contrary to other
							// types of messages, this one doesn't expect any answer and therefore
							// we drop the sender.
							let future = kad_sink.send(message).map(move |kad_sink| {
								future::Loop::Continue((kad_sink, rest, send_back_queue))
							});
							Box::new(future) as Box<_>
						}
						Some((message, Some(send_back))) => {
							// Any other message has been received on `rx`. Send it to the remote.
							let future = kad_sink.send(message).map(move |kad_sink| {
								send_back_queue.push(send_back);
								future::Loop::Continue((kad_sink, rest, send_back_queue))
							});
							Box::new(future) as Box<_>
						}
						Some((message, None)) => {
							// Message received by the remote.
							match message {
								message @ KadMsg::Ping => {
									// TODO: annoying to implement
									unimplemented!()
								}
								message @ KadMsg::GetProvidersRes { .. }
								| message @ KadMsg::FindNodeRes { .. }
								| message @ KadMsg::GetValueRes { .. } => {
									if !send_back_queue.is_empty() {
										let send_back = send_back_queue.remove(0);
										send_back.send(message);
										let future = future::ok(future::Loop::Continue((
											kad_sink,
											rest,
											send_back_queue,
										)));
										return Box::new(future) as Box<_>;
									} else {
										let future = future::err(IoErrorKind::InvalidData.into());
										return Box::new(future) as Box<_>;
									}
								}
								KadMsg::FindNodeReq { key, .. } => {
									let message = KadMsg::FindNodeRes {
										cluster_level: 10,		// TODO:
										closer_peers: vec![protocol::Peer {
											node_id: interface.local_id().clone(),
											multiaddrs: vec![],
											connection_ty: protocol::ConnectionType::Connected,
										}],		// TODO:
									};

									let future = kad_sink.send(message).map(move |kad_sink| {
										future::Loop::Continue((kad_sink, rest, send_back_queue))
									});

									Box::new(future) as Box<_>
								}
								other => {
									// TODO:
									unimplemented!("unimplemented msg {:?}", other)
								}
							}
						}
					}
				})
		},
	);

	Box::new(future) as Box<Future<Item = (), Error = IoError>>
}
