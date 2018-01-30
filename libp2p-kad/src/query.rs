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

use fnv::{FnvHashMap, FnvHashSet};
use futures::{future, stream, Future, IntoFuture};
use futures::sync::{mpsc, oneshot};
use kbucket::{KBucketsPeerId, KBucketsTable};
use libp2p_identify::IdentifyProtocolConfig;
use libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
use libp2p_ping::Ping;
use libp2p_swarm::{ConnectionUpgrade, UpgradedNode};
use libp2p_swarm::{Endpoint, MuxedTransport, OrUpgrade, SwarmController, UpgradeExt};
use libp2p_swarm::transport::EitherSocket;
use multiaddr::Multiaddr;
use parking_lot::Mutex;
use protocol;
use rand;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Timer;

/// Interface that the query uses to communicate with the rest of the system.
pub trait QueryInterface: Clone {
	/// The `Peerstore` object where the query will load and store information about nodes.
	type Peerstore: Peerstore + Clone;
	/// The record store to use for `FIND_VALUE` queries.
	type RecordStore: Clone;
	/// Future that yields the Kademlia message that a remote will answer as part of a sent
	/// message.
	type Outcome: Future<Item = protocol::KadMsg, Error = ()>;

	/// Returns the peer ID of the local node.
	fn local_id(&self) -> &PeerId;

	/// Updates an entry in the K-Buckets.
	fn kbuckets_update(&self, peer: PeerId);

	/// Finds the nodes closest to a peer ID.
	fn kbuckets_find_closest(&self, addr: &PeerId) -> Vec<PeerId>;

	/// Grants access to the peerstore to use for this query.
	fn peer_store(&self) -> Self::Peerstore;

	/// Grants access to the recordstore to use for this query.
	fn record_store(&self) -> Self::RecordStore;

	/// Returns the level of parallelism wanted for this query.
	fn parallelism(&self) -> usize;

	/// Duration between two waves of queries.
	fn cycle_duration(&self) -> Duration;

	/// Sends a message to a given multiaddress (opening a connection if necessary), and produces
	/// a `Future` that will yield the response. If the message doesn't expect a response, then
	/// the `Future` should not be processed and should yield an error.
	///
	/// The `Future` should have an appropriate timeout inside of it.
	fn send(&self, Multiaddr, protocol::KadMsg) -> Self::Outcome;
}

/// Starts a query for a `FIND_NODE` request.
#[inline]
pub fn find_node<'a, I>(
	query_interface: I,
	searched_key: PeerId,
) -> Box<Future<Item = Vec<PeerId>, Error = IoError> + 'a>
where
	I: QueryInterface + 'a,
{
	query(query_interface, searched_key)
}

/// Refreshes a specific bucket by performing a `FIND_NODE` on a random ID of this bucket.
pub fn refresh<'a, I>(
	query_interface: I,
	bucket_num: usize,
) -> Box<Future<Item = (), Error = IoError> + 'a>
where
	I: QueryInterface + 'a,
{
	let peer_id = match gen_random_id(&query_interface, bucket_num) {
		Some(p) => p,
		None => return Box::new(future::ok(())),
	};

	println!(
		"refreshing bucket {:?} with peer {:?}",
		bucket_num,
		&peer_id.as_bytes()[..20]
	);

	let future = find_node(query_interface, peer_id).map(|_| ());
	Box::new(future) as Box<_>
}

fn gen_random_id<I>(query_interface: &I, bucket_num: usize) -> Option<PeerId>
where
	I: ?Sized + QueryInterface,
{
	// TODO: there is a design issue here, due to the fact that peer IDs are multihashes and not
	//       byte arrays

	let my_id = query_interface.local_id();
	let my_id_len = my_id.as_bytes().len();

	// TODO: this 2 is magic here ; it is the length of the hash of the multihash
	let bits_diff = bucket_num + 1;
	if bits_diff > 8 * (my_id_len - 2) {
		return None;
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
	Some(peer_id)
}

// Here is the algorithm for node finding, used by this code:
//
// - We start by picking the N closest known nodes from the k-buckets and put them in a
//   list of pending nodes, ordered by distance with the searched key. N is generally 20.
//
// - We also keep a list of multiaddresses that we have tried but failed to connect to.
//
// - And finally we have a list of successfully-contacted nodes for the final result.
//
// - Start iterating:
//
//   - Pick the first alpha nodes of this pending list, and choose from the peerstore
//     a multiaddress to contact for each of them. If we already have a active connection
//     to one of the multiaddresses, use it. Otherwise just use the first known one that
//     isn't in the list of failed addresses. If no multiaddress is available, remove this
//     node from the pending list.
//
//   - Send a `FIND_NODE` query to the chosen multiaddresses, opening a connection if
//     necessary.
//
//   - Whenever one of the queries is answered, if the multiaddress has error or timed out, remove
//     it from the peerstore and add it to the list of failed attempts.
//     If it has been successfully contacted, put the node in the list of the successfully
//     contacted nodes. We write the peer ids and multiaddresses reported by the remote into the
//     list of pending nodes, but also into the k-buckets and into the peerstore.
//
//   - At a specific interval named the "cycle duration", we spawn a new wave of `alpha` queries
//     even if the previous ones haven't finished.
//     TODO: ^ not implemented
//
//   - If the list of sucessfully contacted nodes contains more than N elements, or if
//     the closest node in this list hasn't been updated during the latest iteration, stop
//     iterating and return the result.
//     TODO: ^ wrong
//
fn query<'a, I>(
	query_interface: I,
	searched_key: PeerId,
) -> Box<Future<Item = Vec<PeerId>, Error = IoError> + 'a>
where
	I: QueryInterface + 'a,
{
	let parallelism = query_interface.parallelism();

	// State of the current iterative process.
	struct State<'a> {
		// Final output of the iteration.
		result: Vec<PeerId>,
		// For each open connection, a future with the response of the remote.
		current_attempts_fut:
			Vec<Box<Future<Item = Result<protocol::KadMsg, ()>, Error = ()> + 'a>>, // TODO: the Error can be !
		// For each open connection, the multiaddress that we are connected to.
		// Must always have the same length as `current_attempts_fut`.
		current_attempts_addrs: Vec<(PeerId, Multiaddr)>,
		// Nodes that need to be attempted.
		pending_nodes: Vec<PeerId>,
		// Multiaddresses that we tried to contact but failed.
		failed_to_contact: FnvHashSet<Multiaddr>,
	}

	let initial_state = State {
		result: Vec::new(),
		current_attempts_fut: Vec::new(),
		current_attempts_addrs: Vec::new(),
		pending_nodes: query_interface.kbuckets_find_closest(&searched_key),
		failed_to_contact: Default::default(),
	};

	let stream = future::loop_fn(initial_state, move |mut state| {
		let searched_key = searched_key.clone();
		let query_interface = query_interface.clone();
		let query_interface2 = query_interface.clone();

		let to_contact = state
			.pending_nodes
			.iter()
			.filter(|peer| !state.result.iter().any(|p| &p == peer))
			.filter_map(|node| {
				query_interface
                    .peer_store().clone()
                    .peer(node)
                    .into_iter()
                    .flat_map(|peer| peer.addrs())       // TODO: choose from open_connections if any
                    .filter(|ma| !state.failed_to_contact.contains(ma))
                    .filter(|ma| !state.current_attempts_addrs.iter().any(|f| &f.1 == ma))
                    .filter(|ma| { let s = format!("{:?}", ma); state.result.is_empty() || (!s.contains("127.0.0.1") && !s.contains("::1")) })  // TODO: hack
                    .next()
                    .map(|ma| (node.clone(), ma))
			})
			.take(parallelism.saturating_sub(state.current_attempts_fut.len()))
			.collect::<Vec<_>>();
		// TODO: loop in case there are less than `parallelism` nodes, so that we can try
		//       multiple multiaddrs simultaneously

		for (peer, multiaddr) in to_contact {
			let message = protocol::KadMsg::FindNodeReq {
				key: searched_key.clone().into_bytes(),
				cluster_level: 10, // TODO: correct value
			};

			println!("contacting {:?}", multiaddr);
			let resp_rx = query_interface.send(multiaddr.clone(), message);
			state
				.current_attempts_addrs
				.push((peer.clone(), multiaddr.clone()));
			let current_attempt = resp_rx
                .map_err(|_| ())        // TODO: better error?
                .then(|res| Ok(res));
			state
				.current_attempts_fut
				.push(Box::new(current_attempt) as Box<_>);
		}

		debug_assert_eq!(
			state.current_attempts_addrs.len(),
			state.current_attempts_fut.len()
		);

		//
		let current_attempts_fut = mem::replace(&mut state.current_attempts_fut, Vec::new());
		if current_attempts_fut.is_empty() {
			// If `current_attempts_fut` is empty, then `select_all` would panic. It attempts
			// when we have no additional node to query.
			let future = future::ok(future::Loop::Break(state));
			return future::Either::A(future);
		}

		let future = future::select_all(current_attempts_fut.into_iter()).and_then(
			move |(message, trigger_idx, other_current_attempts)| {
				let (remote_id, remote_addr) = state.current_attempts_addrs.remove(trigger_idx);
				state.current_attempts_fut.extend(other_current_attempts);

				let closer_peers = match message {
					Ok(protocol::KadMsg::FindNodeRes { closer_peers, .. }) => {
						// Received a message.
						println!("success msg from {:?} a.k.a. {:?}", remote_addr, remote_id);
						println!("num closer peers: {:?}", closer_peers.len());
						closer_peers
					}
					Ok(_other_msg) => {
						// TODO: add to failed to contact
						println!("wrong msg from {:?} {:?}", remote_id, remote_addr);
						return Ok(future::Loop::Continue(state));
					}
					Err(_) => {
						println!(
							"timeout when contacting {:?} a.k.a. {:?}",
							remote_addr, remote_id
						);
						if let Some(mut peer) =
							query_interface.peer_store().clone().peer(&remote_id)
						{
							peer.set_addr_ttl(remote_addr.clone(), Duration::new(0, 0));
						}
						state.failed_to_contact.insert(remote_addr);
						return Ok(future::Loop::Continue(state));
					}
				};

				// Update the result with the node that sent the result.
				if let Some(insert_pos) = state.result.iter().position(|e| {
					e.distance_with(&searched_key) >= remote_id.distance_with(&searched_key)
				}) {
					if state.result[insert_pos] != remote_id {
						state.result.insert(insert_pos, remote_id);
					}
				} else {
					state.result.push(remote_id);
				}

				let mut nearest_node_updated = false;

				for mut peer in closer_peers {
					// Update the k-buckets and peerstore with the information sent by
					// the remote.
					{
						let valid_multiaddrs = peer.multiaddrs
							.drain(..)
							.filter(|addr| !state.failed_to_contact.iter().any(|e| e == addr));

						query_interface2.kbuckets_update(peer.node_id.clone());
						query_interface2
							.peer_store()
							.clone()
							.peer_or_create(&peer.node_id)
							.add_addrs(valid_multiaddrs, Duration::from_secs(3600)); // TODO: which TTL?
					}

					if peer.node_id.distance_with(&searched_key)
						<= state.result[0].distance_with(&searched_key)
					{
						nearest_node_updated = true;
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

				if nearest_node_updated && state.result.len() < 20 {
					// TODO: right value
					Ok(future::Loop::Continue(state))
				} else {
					Ok(future::Loop::Break(state))
				}
			},
		);
		future::Either::B(future)
	});

	let stream = stream
		.map(|state| state.result)
		.map_err(|v| -> IoError { unreachable!() }); // TODO:

	Box::new(stream) as Box<_>
}
