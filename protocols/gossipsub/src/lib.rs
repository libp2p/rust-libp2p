// Copyright 2020 Sigma Prime Pty Ltd.
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

//! Implementation of the [Gossipsub](https://github.com/libp2p/specs/blob/master/pubsub/gossipsub/README.md) protocol.
//!
//! Gossipsub is a P2P pubsub (publish/subscription) routing layer designed to extend upon
//! floodsub and meshsub routing protocols.
//!
//! # Overview
//!
//! *Note: The gossipsub protocol specifications
//! (<https://github.com/libp2p/specs/tree/master/pubsub/gossipsub>) provide an outline for the
//! routing protocol. They should be consulted for further detail.*
//!
//! Gossipsub  is a blend of meshsub for data and randomsub for mesh metadata. It provides bounded
//! degree and amplification factor with the meshsub construction and augments it using gossip
//! propagation of metadata with the randomsub technique.
//!
//! The router maintains an overlay mesh network of peers on which to efficiently send messages and
//! metadata.  Peers use control messages to broadcast and request known messages and
//! subscribe/unsubscribe from topics in the mesh network.
//!
//! # Important Discrepancies
//!
//! This section outlines the current implementation's potential discrepancies from that of other
//! implementations, due to undefined elements in the current specification.
//!
//! - **Topics** -  In gossipsub, topics configurable by the `hash_topics` configuration parameter.
//!   Topics are of type [`TopicHash`]. The current go implementation uses raw utf-8 strings, and
//!   this is default configuration in rust-libp2p. Topics can be hashed (SHA256 hashed then base64
//!   encoded) by setting the `hash_topics` configuration parameter to true.
//!
//! - **Sequence Numbers** - A message on the gossipsub network is identified by the source
//!   [`PeerId`](libp2p_identity::PeerId) and a nonce (sequence number) of the message. The sequence
//!   numbers in this implementation are sent as raw bytes across the wire. They are 64-bit
//!   big-endian unsigned integers. When messages are signed, they are monotonically increasing
//!   integers starting from a random value and wrapping around u64::MAX. When messages are
//!   unsigned, they are chosen at random. NOTE: These numbers are sequential in the current go
//!   implementation.
//!
//! # Peer Discovery
//!
//! Gossipsub does not provide peer discovery by itself. Peer discovery is the process by which
//! peers in a p2p network exchange information about each other among other reasons to become
//! resistant against the failure or replacement of the
//! [boot nodes](https://docs.libp2p.io/reference/glossary/#boot-node) of the network.
//!
//! Peer
//! discovery can e.g. be implemented with the help of the [Kademlia](https://github.com/libp2p/specs/blob/master/kad-dht/README.md) protocol
//! in combination with the [Identify](https://github.com/libp2p/specs/tree/master/identify) protocol. See the
//! Kademlia implementation documentation for more information.
//!
//! # Using Gossipsub
//!
//! ## Gossipsub Config
//!
//! The [`Config`] struct specifies various network performance/tuning configuration
//! parameters. Specifically it specifies:
//!
//! [`Config`]: struct.Config.html
//!
//! This struct implements the [`Default`] trait and can be initialised via
//! [`Config::default()`].
//!
//!
//! ## Behaviour
//!
//! The [`Behaviour`] struct implements the [`libp2p_swarm::NetworkBehaviour`] trait allowing it to
//! act as the routing behaviour in a [`libp2p_swarm::Swarm`]. This struct requires an instance of
//! [`PeerId`](libp2p_identity::PeerId) and [`Config`].
//!
//! [`Behaviour`]: struct.Behaviour.html

//! ## Example
//!
//! For an example on how to use gossipsub, see the [chat-example](https://github.com/libp2p/rust-libp2p/tree/master/examples/chat).

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod backoff;
mod behaviour;
mod config;
mod error;
mod gossip_promises;
mod handler;
mod mcache;
mod metrics;
mod peer_score;
mod protocol;
mod rpc;
mod rpc_proto;
mod subscription_filter;
mod time_cache;
mod topic;
mod transform;
mod types;

pub use self::{
    behaviour::{Behaviour, Event, MessageAuthenticity},
    config::{Config, ConfigBuilder, ValidationMode, Version},
    error::{ConfigBuilderError, PublishError, SubscriptionError, ValidationError},
    metrics::Config as MetricsConfig,
    peer_score::{
        score_parameter_decay, score_parameter_decay_with_base, PeerScoreParams,
        PeerScoreThresholds, TopicScoreParams,
    },
    subscription_filter::{
        AllowAllSubscriptionFilter, CallbackSubscriptionFilter, CombinedSubscriptionFilters,
        MaxCountSubscriptionFilter, RegexSubscriptionFilter, TopicSubscriptionFilter,
        WhitelistSubscriptionFilter,
    },
    topic::{Hasher, Topic, TopicHash},
    transform::{DataTransform, IdentityTransform},
    types::{FailedMessages, Message, MessageAcceptance, MessageId, RawMessage},
};

#[deprecated(note = "Will be removed from the public API.")]
pub type Rpc = self::types::Rpc;

pub type IdentTopic = Topic<self::topic::IdentityHash>;
pub type Sha256Topic = Topic<self::topic::Sha256Hash>;
