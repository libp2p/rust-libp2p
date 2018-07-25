// This is free and unencumbered software released into the public domain.

// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.

// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.

// For more information, please refer to <http://unlicense.org/>

extern crate libp2p_floodsub;
extern crate libp2p_core;
extern crate libp2p_kad;
extern crate time;

pub usemod rpc_proto;
mod topic;
mod constants;

// gossipsub is an extension of floodsub so it seems to make sense to use everything in floodsub.
pub use floodsub::*;

use libp2p_kad::{
    high_level::KadSystemConfig,
    kad_server::KadConnecConfig
};
use libp2p_core::{
    peer_id::PeerId,
    swarm::swarm
};
use time::Duration;

/// Membership management
/// 
/// Join overlay
/// 
/// Obtain initial contact nodes via rendevous with DHT provider records
 
// See https://github.com/libp2p/rust-libp2p/blob/8e07c18178ac43cad3fa8974a243a98d9bc8b896/kad/src/lib.rs#L21.

//! Kademlia protocol. Allows peer discovery, records store and records fetch.
//!
//! # Usage
//!
//! Usage is done in the following steps:
//!
//! - Build a `KadSystemConfig` and a `KadConnecConfig` object that contain the way you want the
//!   Kademlia protocol to behave.
//!
//! - Create a swarm that upgrades incoming connections with the `KadConnecConfig`.
//!
//! - Build a `KadSystem` from the `KadSystemConfig`. This requires passing a closure that provides
//!   the Kademlia controller of a peer.
//!
//! - You can perform queries using the `KadSystem`.
//!

// TODO: tests!

let sample_peer_id = to_peer_id(ed25519_generated());

// KadSystemConfig
// https://github.com/libp2p/rust-libp2p/blob/7507e0bfd9f11520f2d6291120f1b68d0afce80a/kad/src/high_level.rs#L36
let kad_system_config = KadSystemConfig {
    /// Sources: http://xlattice.sourceforge.net/components/protocol/kademlia/specs.html
    /// https://www.kth.se/social/upload/516479a5f276545d6a965080/3-kademlia.pdf
    /// http://www.scs.stanford.edu/%7Edm/home/papers/kpos.pdf (p. 3, col. 2)
    parallelism: 3,
    local_peer_id: sample_peer_id,
    known_initial_peers: vec![],
    /// tRefresh in Kademlia implementations, sources:
    /// http://xlattice.sourceforge.net/components/protocol/kademlia/specs.html#refresh
    /// https://www.kth.se/social/upload/516479a5f276545d6a965080/3-kademlia.pdf
    /// 1 hour
    kbuckets_timeout: Duration.hour(1),
    /// go gossipsub uses 1 s:
    /// https://github.com/libp2p/go-floodsub/pull/67/files#diff-013da88fee30f5c765f693797e8b358dR30
    /// However, https://www.rabbitmq.com/heartbeats.html#heartbeats-timeout uses 60 s, and
    /// https://gist.github.com/gubatron/cd9cfa66839e18e49846#routing-table uses 15 minutes.
    /// Let's make a conservative selection and choose 15 minutes for an alpha release.
    request_timeout: Duration.minutes(15),
}

// KadConnecConfig
// In https://github.com/libp2p/rust-libp2p/blob/master/kad/src/kad_server.rs
kad_connec_config = KadConnecConfig.new()

// Create a swarm that upgrades incoming connections with the `KadConnecConfig`.
// https://github.com/libp2p/rust-libp2p/blob/4592d1b21ef8bd41a8176ad651feb1aa6cb1b377/core/src/swarm.rs#L28-L43

// /// Creates a swarm.
// ///
// /// Requires an upgraded transport, and a function or closure that will turn the upgrade into a
// /// `Future` that produces a `()`.
// ///
// /// Produces a `SwarmController` and an implementation of `Future`. The controller can be used to
// /// control, and the `Future` must be driven to completion in order for things to work.
// ///
// pub fn swarm<T, H, F>(
//     transport: T,
//     handler: H,
// ) -> (SwarmController<T>, SwarmFuture<T, H, F::Future>)
// where
//     T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
//     H: FnMut(T::Output, Box<Future<Item = Multiaddr, Error = IoError>>) -> F,
//     F: IntoFuture<Item = (), Error = IoError>,

swarm_inst = swarm(kad_connec_config)

// Build a `KadSystem` from the `KadSystemConfig`. This requires passing a closure that provides
// the Kademlia controller of a peer.
// 
// You can perform queries using the `KadSystem`.


// Send a GETNODE message in order to obtain an up-to-date view of the overlay from the passive list of a 
// subscribed node regardless of age of Provider records.

// Once an up-to-date passive view of the overlay has been
// obtained, the node proceeds to join.

// In order to join, it picks `C_rand` nodes at random and sends
// `JOIN` messages to them with some initial TTL set as a design parameter.

// The `JOIN` message propagates with a random walk until a node is willing
// to accept it or the TTL expires. Upon receiving a `JOIN` message, a node Q
// evaluates it with the following criteria:
// - Q tries to open a connection to P. If the connection cannot be opened (e.g. because of NAT),
//   then it checks the TTL of the message.
//   If it is 0, the request is dropped, otherwise Q decrements the TTL and forwards
//   the message to a random node in its active list.
// - If the TTL of the request is 0 or if the size of Q's active list is less than `A`,
//   it accepts the join, adds P to its active list and sends a `NEIGHBOR` message.
// - Otherwise it decrements the TTL and forwards the message to a random node
//   in its active list.

// When Q accepts P as a new neighbor, it also sends a `FORWARDJOIN`
// message to a random node in its active list. The `FORWARDJOIN`
// propagates with a random walk until its TTL is 0, while being added to
// the passive list of the receiving nodes.

// If P fails to join because of connectivity issues, it decrements the
// TTL and tries another starting node. This is repeated until a TTL of zero
// reuses the connection in the case of NATed hosts.

// Once the first links have been established, P then needs to increase
// its active list size to `A` by connecting to more nodes.  This is
// accomplished by ordering the subscriber list by RTT and picking the
// nearest nodes and sending `NEIGHBOR` requests.  The neighbor requests
// may be accepted by `NEIGHBOR` message and rejected by a `DISCONNECT`
// message.

// Upon receiving a `NEIGHBOR` request a node Q evaluates it with the
// following criteria:
// - If the size of Q's active list is less than A, it accepts the new
//   node.
// - If P does not have enough active links (less than `C_rand`, as specified in the message),
//   it accepts P as a random neighbor.
// - Otherwise Q takes an RTT measurement to P.
//   If it's closer than any near neighbors by a factor of alpha, then
//   it evicts the near neighbor if it has enough active links and accepts
//   P as a new near neighbor.
// - Otherwise the request is rejected.

// Note that during joins, the size of the active list for some nodes may
// end up being larger than `A`. Similarly, P may end up with fewer links
// than `A` after an initial join. This follows [3] and tries to minimize
// fluttering in joins, leaving the active list pruning for the
// stabilization period of the protocol.

// ### Leaving the Overlay

// In order to unsubscribe, the node can just leave the overlay by
// sending `DISCONNECT` messages to its active neighbors.  References to
// the node in the various passive lists scattered across the overlay
// will be lazily pruned over time by the passive view management
// component of the protocol.

// Optimization TODO: In order to facilitate fast clean up of departing nodes, we can also
// introduce a `LEAVE` message that eagerly propagates across the
// network.  A node that wants to unsubscribe from the topic, emits a
// `LEAVE` to its active list neighbors in place of `DISCONNECT`.  Upon
// receiving a `LEAVE`, a node removes the node from its active list
// _and_ passive lists. If the node was removed from one of the lists or
// if the TTL is greater than zero, then the `LEAVE` is propagated
// further across the active list links. This will ensure a random
// diffusion through the network that would clean most of the active
// lists eagerly, at the cost of some bandwidth.

// ### Active View Management

// Optimizations TODO: The active list is generally managed reactively: failures are detected
// by TCP, either when a message is sent or when the connection is detected
// as closed.

// In addition to the reactive management strategy, the active list has
// stabilization and optimization components that run periodically with a
// randomized timer, and also serve as failure detectors. The
// stabilization component attempts to prune active lists that are larger
// than A, say because of a slew of recent joins, and grow active lists
// that are smaller than A because of some failures or previous inability
// to neighbor with enough nodes.

// When a node detects that its active list is too large, it queries the neighbors
// for their active lists.
// - If some neighbors have more than `C_rand` random neighbors, then links can be dropped
//   with a `DISCONNECT` message until the size of the active list is A again.
// - If the list is still too large, then it checks the active lists for neighbors that
//   are connected with each other. In this case, one of the links can be dropped
//   with a `DISCONNECT` message.
// - If the list is still too large, then we cannot safely drop connections and it will
//   remain that large until the next stabilization period.

// When a node detects that its active list is too small, then it tries
// to open more connections by picking nodes from its passive list, as
// described in the Join section.

// The optimization component tries to optimize the `C_near` connections
// by replacing links with closer nodes. In order to do so, it takes RTT
// samples from active list nodes and maintains a smoothed running
// average. The neighbors are reordered by RTT and the closest ones are
// considered the near nodes. It then checks the RTT samples of passive
// list nodes and selects the closest node.  If the RTT is smaller by a
// factor of alpha than a near neighbor and it has enough random
// neighbors, then it disconnects and adopts the new node from the
// passive list as a neighbor.

// ### Passive View Management

// The passive list is managed cyclically, as per [2]. Periodically, with
// a randomized timer, each node performs a passive list shuffle with one
// of its active neighbors. The purpose of the shuffle is to update the
// passive lists of the nodes involved. The node that initiates the shuffle
// creates an exchange list that contains its id, `k_a` peers from its
// active list and `k_p` peers from its passive list, where `k_a` and
// `k_p` are protocol parameters (unspecified in [2]). It then sends a
// `SHUFFLE` request to a random neighbor, which is propagated with a
// random walk with an associated TTL.  If the TTL is greater than 0 and
// the number of nodes in the receiver's active list is greater than 1,
// then it propagates the request further. Otherwise, it selects nodes
// from its passive list at random, sends back a `SHUFFLEREPLY` and
// replaces them with the shuffle contents. The originating node
// receiving the `SHUFFLEREPLY` also replaces nodes in its passive list
// with the contents of the message. Care should be taken for issues 
// with transitive connectivity due to NAT. If
// a node cannot connect to the originating node for a `SHUFFLEREPLY`,
// then it should not perform the shuffle. Similarly, the originating
// node could time out waiting for a shuffle reply and try with again
// with a lower TTL, until a TTL of zero reuses the connection in the
// case of NATed hosts.

// In addition to shuffling, proximity awareness and leave cleanup
// requires that we compute RTT samples and check connectivity to nodes
// in the passive list.  Periodically, the node selects some nodes from
// its passive list at random and tries to open a connection if it
// doesn't already have one. It then checks that the peer is still
// subscribed to the overlay. If the connection attempt is successful and
// the node is still subscribed to the topic, it then updates the RTT
// estimate for the peer in the list with a ping. Otherwise, it removes
// it from the passive list for cleanup.

// ## Broadcast Protocol

// ### Broadcast State

// Once it has joined the overlay, the node starts its main broadcast logic
// loop. The loop receives messages to publish from the application, messages
// published from other nodes, and with notifications from the management
// protocol about new active neighbors and disconnections.

// The state of the broadcast loop consists of two sets of peers, the eager
// and lazy lists, with the eager list initialized to the initial neighbors
// and the lazy list empty. The loop also maintains a time-based cache of
// recent messages, together with a queue of lazy message notifications.
// In addition to the cache, it maintains a list of missing messages
// known by lazy gossip but not yet received through the multicast tree.

// ### Message Propagation and Multicast Tree Construction

// When a node publishes a message, it broadcasts a `GOSSIP` message with
// a hopcount of 1 to all its eager peers, adds the message to the cache,
// and adds the message id to the lazy notification queue.

// When a node receives a `GOSSIP` message from a neighbor, first it
// checks its cache to see if it has already seen this message. If the
// message is in the cache, it prunes the edge of the multicast graph by
// sending a `PRUNE` message to the peer, removing the peer from the
// eager list, and adding it to the lazy list.

// If the node hasn't seen the message before, it delivers the message to
// the application and then adds the peer to the eager list and proceeds
// to broadcast. The hopcount is incremented and then the node forwards
// it to its eager peers, excluding the source. It also adds the message
// to the cache, and pushes the message id to the lazy notification queue.

// The loop runs a short periodic timer, with a period in the order of
// 0.1s for gossiping message summaries. Every time it fires, the node
// flushes the lazy notification queue with all the recently received
// message ids in an `IHAVE` message to its lazy peers.  The `IHAVE`
// notifications summarize recent messages the node has seen and have not
// propagated through the eager links.

// ### Multicast Tree Repair

// When a failure occurs, at least one multicast tree branch is affected,
// as messages are not transmitted by eager push.  The `IHAVE` messages
// exchanged through lazy gossip are used both to recover missing messages
// but also to provide a quick mechanism to heal the multicast tree.

// When a node receives an `IHAVE` message for unknown messages, it
// simply marks the messages as missing and places them to the missing
// message queue. It then starts a timer and waits to receive the message
// with eager push before the timer expires. The timer duration is a
// protocol parameter that should be configured considering the diameter
// of the overlay and the target recovery latency. A more realistic
// implementation is to use a persistent timer heartbeat to check for
// missing messages periodically, marking on first touch and considered
// missing on the second timer touch.

// When a message is detected as missing, the node selects the first
// `IHAVE` announcement it has seen for the missing message and sends a
// `GRAFT` message to the peer, piggybacking other missing messages. The
// `GRAFT` message serves a dual purpose: it triggers the transmission of
// the missing messages and at the same time adds the link to the
// multicast tree, healing it.

// Upon receiving a `GRAFT` message, a node adds the peer to the eager
// list and transmits the missing messages from its cache as `GOSSIP`.
// Note that the message is not removed from the missing list until it is
// received as a response to a `GRAFT`. If the message has not been
// received by the next timer tick, say because the grafted peer has
// also failed, then another graft is attempted and so on, until enough
// ticks have elapsed to consider the message lost.

// ### Multicast Tree Optimization (TODO)

// The multicast tree is constructed lazily, following the path of the
// first published message from some source. Therefore, the tree may not
// directly take advantage of new paths that may appear in the overlay as
// a result of new nodes/links. The overlay may also be suboptimal for
// all but the first source.

// To overcome these limitations and adapt the overlay to multiple
// sources, the authors in [1] propose an optimization: every time a
// message is received, it is checked against the missing list and the
// hopcount of messages in the list. If the eager transmission hopcount
// exceeds the hopcount of the lazy transmission, then the tree is
// candidate for optimization. If the tree were optimal, then the
// hopcount for messages received by eager push should be less than or
// equal to the hopcount of messages propagated by lazy push. Thus the
// eager link can be replaced by the lazy link and result to a shorter
// tree.

// To promote stability in the tree, the authors in [1] suggest that this
// optimization be performed only if the difference in hopcount is greater
// than a threshold value. This value is a design parameter that affects
// the overall stability of the tree: the lower the value, the more
// easier the protocol will try to optimize the tree by exchanging
// links. But if the threshold value is too low, it may result in
// fluttering with multiple active sources. Thus, the value should be
// higher and closer to the diameter of the tree to avoid constant
// changes.

// ### Active View Changes

// The active peer list is maintained by the Membership Management protocol:
// nodes may be removed because of failure or overlay reorganization, and new
// nodes may be added to the list because of new connections. The Membership
// Management protocol communicates these changes to the broadcast loop via
// `NeighborUp` and `NeighborDown` notifications.

// When a new node is added to the active list, the broadcast loop receives
// a `NeighborUp` notifications, it simply adds the node to the eager peer
// list. On the other hand, when a node is removed with a `NeighborDown`
// notificaiton, the loop has to consider if the node was an eager or lazy
// peer. If the node was a lazy peer, it doesn't need to do anything as the
// departure does not affect the multicast tree. If the node was an eager peer
// however, the loss of that edge may result in a disconnected tree.

// There are two strategies in reaction to the loss of an eager peer. The
// first one is to do nothing, and wait for lazy push to repair the tree
// naturally with `IHAVE` messages in the next message broadcast. This
// might result in delays propagating the next few messages but is
// advocated by the authors in [1]. TODO: An alternative is to eagerly repair
// the tree by promoting lazy peers to eager with empty `GRAFT` messages
// and let the protocol prune duplicate paths naturally with `PRUNE`
// messages in the next message transmission. This may have a bit of
// bandwidth cost, but it is perhaps more appropriate for applications
// that value latency minimization which is the case for many IPFS
// applications.

// ## Protocol Messages

// A quick summary of referenced protocol messages and their payload.
// All messages are assumed to be enclosed in a suitable envelope and have
// a source and monotonic sequence id.


// ```
// ;; Initial node discovery
// GETNODES {}

// NODES {
//  peers []peer.ID
//  ttl int
// }

// ;; Topic querying (membership check for passive view management)
// GETTOPICS {}

// TOPICS {
//  topics []topic.ID
// }

// ;; Membership Management protocol
// JOIN {
//  peer peer.ID
//  ttl int
// }

// FORWARDJOIN {
//  peer peer.ID
//  ttl int
// }

// NEIGHBOR {
//  peers []peer.ID
// }

// DISCONNECT {}

// LEAVE {
//  source peer.ID
//  ttl int
// }

// SHUFFLE {
//  peer peer.ID
//  peers []peer.ID
//  ttl int
// }

// SHUFFLEREPLY {
//  peers []peer.ID
// }

// ;; Broadcast protocol
// GOSSIP {
//  source peer.ID
//  hops int
//  msg []bytes
// }

// IHAVE {
//  summary []MessageSummary
// }

// MessageSummary {
//  id message.ID
//  hops int
// }

// PRUNE {}

// GRAFT {
//  msgs []message.ID
// }

// ```

// ## Differences from Plumtree/HyParView

// There are some noteworthy differences in the protocol described and
// the published Plumtree/HyParView protocols. There might be some more
// differences in minor details, but this document is written from a
// practical implementer's point of view.

// Membership Management protocol:
// - The node views are managed with proximity awareness. The HyParView protocol
//   has no provisions for proximity, these come from GoCast's implementation
//   of proximity aware overlays; but note that we don't use UDP for RTT measurements
//   and the increased `C_rand` to increase fault-tolerance at the price of some optimization.
// - Joining nodes don't get to get all A connections by kicking out extant nodes,
//   as this would result in overlay instability in periods of high churn. Instead, nodes
//   ensure that the first few links are created even if they oversubscribe their fanout, but they
//   don't go out of their way to create remaining links beyond the necessary `C_rand` links.
//   Nodes later bring the active list to balance with a stabilization protocol.
//   Also noteworthy is that only `C_rand` `JOIN` messages are propagated with a random walk; the
//   remaining joins are considered near joins and handled with normal `NEIGHBOR` requests.
//   In short, the Join protocol is reworked, with the influence of GoCast.
// - There is no active view stabilization/optimization protocol in HyParView. This is very
//   much influenced from GoCast, where the protocol allows oversubscribing and later drops
//   extraneous connections and replaces nodes for proximity optimization.
// - `NEIGHBOR` messages play a dual role in the proposed protocol implementation, as they can
//   be used for establishing active links and retrieving membership lists.
// - There is no connectivity check in HyParView and retires with reduced TTLs, but this
//   is incredibly important in a world full of NAT.
// - There is no `LEAVE` provision in HyParView.

// Broadcast protocol:
// - `IHAVE` messages are aggregated and lazily pushed via a background timer. Plumtree eagerly
//   pushes `IHAVE` messages, which is wasteful and loses the opportunity for aggregation.
//   The authors do suggest lazy aggregation as a possible optimization nonetheless.
// - `GRAFT` messages similarly aggregate multiple message requests.
// - Missing messages and overlay repair are managed by a single background timer instead of
//   of creating timers left and right for every missing message; that's impractical from an
//   implementation point of view, at least in Go.
// - There is no provision for eager overlay repair on `NeighborDown` messages in Plumtree.
