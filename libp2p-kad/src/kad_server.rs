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

//! Contains a `ConnectionUpgrade` that makes it possible to send requests and receive responses
//! from nodes after the upgrade.
//!
//! # Usage
//!
//! - Implement the `KadServerInterface` trait on something clonable (usually an `Arc`).
//! - Create a `KademliaServerConfig` object from that interface. This struct implements
//!   `ConnectionUpgrade`.
//! - Update a connection through that `KademliaServerConfig`. The output yields you a
//!   `KademliaServerController` and a future that must be driven to completion. The controller
//!   allows you to perform queries and receive responses.
//!
//! The `KademliaServerController` is usually extracted and stored in some sort of hash map in an
//! `Arc` in order to be available whenever we need to request something from a node.

use bytes::Bytes;
use error::KadError;
use fnv::FnvHashMap;
use futures::{self, future, stream, Future, Sink, Stream};
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

/// Interface that this server system uses to communicate with the rest of the system.
pub trait KadServerInterface: Clone {
	/// The `Peerstore` object where the query will load and store information about nodes.
	type Peerstore: Peerstore;
	/// The record store to use for `FIND_VALUE` queries.
	type RecordStore;

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
#[derive(Debug, Clone)]
pub struct KademliaServerController {
	inner: mpsc::UnboundedSender<(KadMsg, oneshot::Sender<KadMsg>)>,
}

impl KademliaServerController {
	/// Sends a `FIND_NODE` query to the node and sends back the response.
	pub fn find_node(&self, searched_key: &PeerId)
					 -> Box<Future<Item = Vec<Peer>, Error = IoError>>
	{
		let message = protocol::KadMsg::FindNodeReq {
			key: searched_key.clone().into_bytes(),
			cluster_level: 10, // TODO: correct value
		};

		let (tx, rx) = oneshot::channel();

		match self.inner.unbounded_send((message, tx)) {
			Ok(()) => (),
			Err(_) => {
				let fut = future::err(IoError::new(IoErrorKind::ConnectionAborted,
								         		   "connection to remote has aborted"));
				return Box::new(fut) as Box<_>
			}
		};

		let future = rx
			.map_err(|_| {
				IoError::new(IoErrorKind::ConnectionAborted, "connection to remote has aborted")
			})
			.and_then(|msg| {
				match msg {
					KadMsg::FindNodeRes { closer_peers, .. } => Ok(closer_peers),
					_ => {
						Err(IoError::new(IoErrorKind::InvalidData,
										"invalid message received by the remote"))
					}
				}
			});

		Box::new(future) as Box<_>
	}
}

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
