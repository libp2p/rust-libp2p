//! A gossiping and subscribing p2p messaging protocol, dials and listens on
//! a random subset of peers in a mesh network. For more details, see the [gossipsub
//! spec](https://github.com/libp2p/specs/tree/master/pubsub/gossipsub).

extern crate bs58;
extern crate bytes;
extern crate cuckoofilter;
extern crate fnv;
extern crate futures;
extern crate libp2p_core;
extern crate libp2p_floodsub;
extern crate protobuf;
extern crate rand;
extern crate smallvec;
extern crate tokio_codec;
extern crate tokio_io;
extern crate unsigned_varint;

/// Includes constants to use in a `NetworkBehaviour`.
pub mod constants;

/// Contains the `Message` types used in `Gossipsub`: messages for arbitrary
/// data to use in applications, subscription messages, and control messages,
/// as well as wrappers.
pub mod message;

/// Implements a `ProtocolHandler` for the gossipsub protocol. 2nd highest
/// level.
pub mod handler;

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
