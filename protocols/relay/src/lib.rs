// Copyright 2019 Parity Technologies (UK) Ltd.
// Copyright 2021 Protocol Labs.
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

//! Implementation of libp2p [circuit relay](https://github.com/libp2p/specs/blob/master/relay/README.md) protocol.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod behaviour;
mod copy_future;
mod priv_client;
mod protocol;
pub mod v2;

#[allow(clippy::derive_partial_eq_without_eq)]
mod message_proto {
    include!(concat!(env!("OUT_DIR"), "/message_v2.pb.rs"));
}

pub use behaviour::{Behaviour, CircuitId, Config, Event};
pub use protocol::{HOP_PROTOCOL_NAME, STOP_PROTOCOL_NAME};

/// Types related to the relay protocol inbound.
pub mod inbound {
    pub mod hop {
        pub use crate::protocol::inbound_hop::FatalUpgradeError;
    }
    pub mod stop {
        pub use crate::protocol::inbound_stop::FatalUpgradeError;
    }
}

/// Types related to the relay protocol outbound.
pub mod outbound {
    pub mod hop {
        pub use crate::protocol::outbound_hop::FatalUpgradeError;
    }
    pub mod stop {
        pub use crate::protocol::outbound_stop::FatalUpgradeError;
    }
}

/// Everything related to the relay protocol from a client's perspective.
pub mod client {
    pub use crate::priv_client::{new, Behaviour, Connection, Event, Transport};

    pub mod transport {
        pub use crate::priv_client::transport::Error;
    }
}

// Check that we can safely cast a `usize` to a `u64`.
static_assertions::const_assert! {
    std::mem::size_of::<usize>() <= std::mem::size_of::<u64>()
}

/// The ID of an outgoing / incoming, relay / destination request.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

impl RequestId {
    fn new() -> RequestId {
        RequestId(rand::random())
    }
}
