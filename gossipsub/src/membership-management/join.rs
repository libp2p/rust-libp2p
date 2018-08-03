use libp2p_kad::{
    high_level::KadSystemConfig,
    kad_server::KadConnecConfig
};
use libp2p_core::{
    peer_id::PeerId,
    swarm::swarm,
    Transport
};
use time::Duration;

use futures::Future;
use libp2p_ping::Ping;

use constants::{
    ALPHA,
    KBUCKETS_TIMEOUT,
    REQUEST_TIMEOUT,
};
/// Join overlay
/// 
/// Obtain initial contact nodes via rendevous with DHT provider records
 
// See https://github.com/libp2p/rust-libp2p/blob/8e07c18178ac43cad3fa8974a243a98d9bc8b896/kad/src/lib.rs#L21.

//! "Kademlia protocol. Allows peer discovery, records store and records fetch.
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
//! - You can perform queries using the `KadSystem`."
//!

let sample_peer_id = to_peer_id(ed25519_generated());

// KadSystemConfig
// https://github.com/libp2p/rust-libp2p/blob/7507e0bfd9f11520f2d6291120f1b68d0afce80a/kad/src/high_level.rs#L36
let kad_system_config = KadSystemConfig {
    /// Sources: http://xlattice.sourceforge.net/components/protocol/kademlia/specs.html
    /// https://www.kth.se/social/upload/516479a5f276545d6a965080/3-kademlia.pdf
    /// http://www.scs.stanford.edu/%7Edm/home/papers/kpos.pdf (p. 3, col. 2)
    parallelism: ALPHA,
    local_peer_id: sample_peer_id,
    known_initial_peers: vec![],
    /// tRefresh in Kademlia implementations, sources:
    /// http://xlattice.sourceforge.net/components/protocol/kademlia/specs.html#refresh
    /// https://www.kth.se/social/upload/516479a5f276545d6a965080/3-kademlia.pdf
    /// 1 hour
    kbuckets_timeout: Duration.hour(KBUCKETS_TIMEOUT),
    /// go gossipsub uses 1 s:
    /// https://github.com/libp2p/go-floodsub/pull/67/files#diff-013da88fee30f5c765f693797e8b358dR30
    /// However, https://www.rabbitmq.com/heartbeats.html#heartbeats-timeout uses 60 s, and
    /// https://gist.github.com/gubatron/cd9cfa66839e18e49846#routing-table uses 15 minutes.
    /// Let's make a conservative selection and choose 15 minutes for an alpha release.
    request_timeout: Duration.minutes(REQUEST_TIMEOUT),
}

// KadConnecConfig
// In https://github.com/libp2p/rust-libp2p/blob/master/kad/src/kad_server.rs
let kad_connec_config = KadConnecConfig.new()

// For reference, quoted verbatim, could delete:
// "Create a swarm that upgrades incoming connections with the `KadConnecConfig`.
// https://github.com/libp2p/rust-libp2p/blob/4592d1b21ef8bd41a8176ad651feb1aa6cb1b377/core/src/swarm.rs#L28-L43

// /// "Creates a swarm.
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
//     F: IntoFuture<Item = (), Error = IoError>,"

/// "[KadConnecConfig] implements `ConnectionUpgrade`. On a successful upgrade, produces a `KadConnecController`
/// and a `Future`. The controller lets you send queries to the remote and receive answers, while
/// the `Future` must be driven to completion in order for things to work."
/// 
/// See https://github.com/libp2p/rust-libp2p/blob/master/core/README.md#swarm, plus 
/// https://github.com/libp2p/rust-libp2p/tree/master/core#the-transport-trait, 
/// https://github.com/libp2p/rust-libp2p/tree/master/core#connection-upgrades

// Note: this line is probably incorrect as KadConnecConfig doesn't implement the Transport trait.
// We need to create an object that implements the Transport trait before passing it to the swarm.
// Pretty sure that KadConnecConfig is an actual protocol:
// https://github.com/libp2p/rust-libp2p/tree/master/core#actual-protocols
// I don't think this is right: I think kad_connec_config.upgrade is the upgraded transport,
// KadConnecConfig::Future.
// I think we can use `with_upgrade`, see
// https://github.com/libp2p/rust-libp2p/tree/master/core#actual-protocols
// for details. No, we can't, KadConnecConfig and ConnectionUpgrade don't implement Transport.
// But creating a swarm requires passing an object that implement the Transport trait.

let mut core = tokio_core::reactor::Core::new().unwrap();

let kad_connec_config_transport = kad_connec_config.with_dummy_muxing();

let (swarm_controller, swarm_future) = libp2p_core::swarm(kad_connec_config_transport,
        Ping, |(mut pinger, service), client_addr| {
    pinger.ping().map_err(|_| panic!())
        .select(service).map_err(|_| panic!())
        .map(|_| ())
});

// The `swarm_controller` can then be used to do some operations.
swarm_controller.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap());

// Runs until everything is finished.
tokio_current_thread::block_on_all(swarm_future).unwrap();
// wrong: let swarm_inst = kad_connec_config.upgrade();
// wrong: let swarm_inst = |kad_peer_controller| kad_connec_config.upgrade();

// "Build a `KadSystem` from the `KadSystemConfig`. This requires passing a closure that provides
// the Kademlia controller of a peer."
// `KadConnecController` is an output of KadConnecConfig.upgrade and I think "passing a
// closure that provides x" means that the closure has been previously defined, and that we call the closure like 
// a function with x as an input.
let kad_system_build = |kad_peer_controller| swarm_inst;
// Not sure if this above is right, test!

// "You can perform queries using the `KadSystem`."

// "Send a GETNODE message in order to obtain an up-to-date view of the overlay from the passive list of a 
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

// TODO: tests!