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

//! Implements the floodsub protocol, see also the:
//! [spec](https://github.com/libp2p/specs/tree/master/pubsub).

use libp2p_core::PeerId;

pub mod protocol;

mod layer;
mod topic;

mod rpc_proto {
    include!(concat!(env!("OUT_DIR"), "/floodsub.pb.rs"));
}

pub use self::layer::{Floodsub, FloodsubEvent};
pub use self::protocol::{FloodsubMessage, FloodsubRpc};
pub use self::topic::Topic;

/// Configuration options for the Floodsub protocol.
#[derive(Debug, Clone)]
pub struct FloodsubConfig {
    /// Peer id of the local node. Used for the source of the messages that we publish.
    pub local_peer_id: PeerId,

    /// `true` if messages published by local node should be propagated as messages received from
    /// the network, `false` by default.
    pub subscribe_local_messages: bool,
}

impl FloodsubConfig {
    pub fn new(local_peer_id: PeerId) -> Self {
        Self {
            local_peer_id,
            subscribe_local_messages: false,
        }
    }
}
