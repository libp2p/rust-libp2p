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

//! Implementation of the libp2p-specific [Kademlia](https://github.com/libp2p/specs/blob/master/kad-dht/README.md) protocol.
//!
//! # Important Discrepancies
//!
//! - **Peer Discovery with Identify** In other libp2p implementations, the
//! [Identify](https://github.com/libp2p/specs/tree/master/identify) protocol might be seen as a core protocol. Rust-libp2p
//! tries to stay as generic as possible, and does not make this assumption.
//! This means that the Identify protocol must be manually hooked up to Kademlia through calls
//! to [`Kademlia::add_address`].
//! If you choose not to use the Identify protocol, and do not provide an alternative peer
//! discovery mechanism, a Kademlia node will not discover nodes beyond the network's
//! [boot nodes](https://docs.libp2p.io/concepts/glossary/#boot-node). Without the Identify protocol,
//! existing nodes in the kademlia network cannot obtain the listen addresses
//! of nodes querying them, and thus will not be able to add them to their routing table.

// TODO: we allow dead_code for now because this library contains a lot of unused code that will
//       be useful later for record store
#![allow(dead_code)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod record_priv;
#[deprecated(
    note = "The `record` module will be made private in the future and should not be depended on."
)]
pub mod record {
    pub use super::record_priv::*;
}

mod addresses;
mod behaviour;
mod handler;
mod jobs;
mod kbucket;
mod protocol;
mod query;

mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub use self::dht::pb::{
        mod_Message::{ConnectionType, MessageType, Peer},
        Message, Record,
    };
}

pub use addresses::Addresses;
pub use behaviour::{
    AddProviderContext, AddProviderError, AddProviderOk, AddProviderPhase, AddProviderResult,
    BootstrapError, BootstrapOk, BootstrapResult, GetClosestPeersError, GetClosestPeersOk,
    GetClosestPeersResult, GetProvidersError, GetProvidersOk, GetProvidersResult, GetRecordError,
    GetRecordOk, GetRecordResult, InboundRequest, Mode, NoKnownPeers, PeerRecord, PutRecordContext,
    PutRecordError, PutRecordOk, PutRecordPhase, PutRecordResult, QueryInfo, QueryMut, QueryRef,
    QueryResult, QueryStats, RoutingUpdate,
};
pub use behaviour::{
    Behaviour, KademliaBucketInserts, KademliaCaching, KademliaConfig, KademliaEvent,
    KademliaStoreInserts, ProgressStep, Quorum,
};
pub use kbucket::{Distance as KBucketDistance, EntryView, KBucketRef, Key as KBucketKey};
pub use protocol::KadConnectionType;
pub use query::QueryId;
pub use record_priv::{store, Key as RecordKey, ProviderRecord, Record};

use libp2p_swarm::StreamProtocol;
use std::num::NonZeroUsize;

/// The `k` parameter of the Kademlia specification.
///
/// This parameter determines:
///
///   1) The (fixed) maximum number of nodes in a bucket.
///   2) The (default) replication factor, which in turn determines:
///       a) The number of closer peers returned in response to a request.
///       b) The number of closest peers to a key to search for in an iterative query.
///
/// The choice of (1) is fixed to this constant. The replication factor is configurable
/// but should generally be no greater than `K_VALUE`. All nodes in a Kademlia
/// DHT should agree on the choices made for (1) and (2).
///
/// The current value is `20`.
pub const K_VALUE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(20) };

/// The `α` parameter of the Kademlia specification.
///
/// This parameter determines the default parallelism for iterative queries,
/// i.e. the allowed number of in-flight requests that an iterative query is
/// waiting for at a particular time while it continues to make progress towards
/// locating the closest peers to a key.
///
/// The current value is `3`.
pub const ALPHA_VALUE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(3) };

pub const PROTOCOL_NAME: StreamProtocol = protocol::DEFAULT_PROTO_NAME;

/// Constant shared across tests for the [`Multihash`](libp2p_core::multihash::Multihash) type.
#[cfg(test)]
const SHA_256_MH: u64 = 0x12;
