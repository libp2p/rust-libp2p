//! A gossiping and subscribing p2p messaging protocol, dials and listens on
//! a random subset of peers in a mesh network. For more details, see the [gossipsub
//! spec](https://github.com/libp2p/specs/tree/master/pubsub/gossipsub).
// TODO: many clones are used, do optimizations wherever feasible.

// TODO: remove before stabilization.
#![allow(unused)]

extern crate custom_error;

extern crate bs58;
extern crate bytes;
extern crate chrono;
extern crate cuckoofilter;
extern crate fnv;
extern crate futures;
extern crate libp2p_core;
extern crate libp2p_floodsub;
extern crate lru_time_cache;
extern crate protobuf;
extern crate rand;
extern crate smallvec;
extern crate tokio_codec;
extern crate tokio_io;
extern crate unsigned_varint;

/// Includes constants to use in a `NetworkBehaviour`.
pub mod constants;

/// Implements a `ProtocolHandler` for the gossipsub protocol. 2nd highest
/// level.
pub mod handler;

pub mod errors;

/// Contains `Mesh`.
pub mod mesh;

/// Contains `MessageCache`.
pub mod mcache;

/// Contains the `Message` types used in `Gossipsub`: messages for arbitrary
/// data to use in applications, subscription messages, and control messages,
/// as well as wrappers.
pub mod message;

/// Contains the `Peers` struct which contains gossipsub and floodsub peers.
pub mod peers;

/// Configures gossipsub and coding and decoding from `rpc_proto.rs.` 2nd
/// lowest level, along with topic. Redefines control messages from
/// `rpc_proto` to avoid depending on protobuf (and potentially use something
/// else in future).
pub mod protocol;

// Implements `Gossipsub`, a high level `NetworkBehaviour`.
mod layer;

// Generated via `protoc --rust_out . rpc.proto && sudo chown $USER:$USER
// *.rs` from rpc.proto. Rules for transport over-the-wire via protobuf.
// Lowest level.
mod rpc_proto;

// Required to be re-implemented from libp2p_floodsub::topic due to
// differences such as how to get a topic from a TopicRep.
mod topic;

pub use self::layer::Gossipsub;
pub use self::message::{GMessage, GossipsubRpc};
pub use self::topic::{TopicMap, Topic, TopicBuilder, TopicHash};
