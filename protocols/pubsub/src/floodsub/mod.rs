/// Implements a `ProtocolHandler` for the floodsub protocol.
pub mod handler;
/// Configures floodsub and coding and decoding from rpc_proto.rs.
pub mod protocol;

// Implements `Floodsub`, a high level `NetworkBehaviour`.
mod layer;
// Generated via `protoc --rust_out . rpc.proto && sudo chown $USER:$USER *.rs` from rpc.proto.
mod rpc_proto;
// Implements `Topic` for peers to publish and subscribe to.
mod topic;