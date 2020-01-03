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

use crate::protocol::{GossipsubMessage, MessageId};
use std::borrow::Cow;
use std::time::Duration;

/// If the `no_source_id` flag is set, the IDENTITY_SOURCE value is used as the source of the
/// packet.
pub const IDENTITY_SOURCE: [u8; 3] = [0, 1, 0];

/// Configuration parameters that define the performance of the gossipsub network.
#[derive(Clone)]
pub struct GossipsubConfig {
    /// The protocol id to negotiate this protocol (default is `/meshsub/1.0.0`).
    pub protocol_id: Cow<'static, [u8]>,

    // Overlay network parameters.
    /// Number of heartbeats to keep in the `memcache` (default is 5).
    pub history_length: usize,

    /// Number of past heartbeats to gossip about (default is 3).
    pub history_gossip: usize,

    /// Target number of peers for the mesh network (D in the spec, default is 6).
    pub mesh_n: usize,

    /// Minimum number of peers in mesh network before adding more (D_lo in the spec, default is 4).
    pub mesh_n_low: usize,

    /// Maximum number of peers in mesh network before removing some (D_high in the spec, default
    /// is 12).
    pub mesh_n_high: usize,

    /// Number of peers to emit gossip to during a heartbeat (D_lazy in the spec, default is 6).
    pub gossip_lazy: usize,

    /// Initial delay in each heartbeat (default is 5 seconds).
    pub heartbeat_initial_delay: Duration,

    /// Time between each heartbeat (default is 1 second).
    pub heartbeat_interval: Duration,

    /// Time to live for fanout peers (default is 60 seconds).
    pub fanout_ttl: Duration,

    /// The maximum byte size for each gossip (default is 2048 bytes).
    pub max_transmit_size: usize,

    /// Flag determining if gossipsub topics are hashed or sent as plain strings (default is false).
    pub hash_topics: bool,

    /// When set, all published messages will have a 0 source `PeerId` (default is false).
    pub no_source_id: bool,

    /// When set to `true`, prevents automatic forwarding of all received messages. This setting
    /// allows a user to validate the messages before propagating them to their peers. If set to
    /// true, the user must manually call `propagate_message()` on the behaviour to forward message
    /// once validated (default is false).
    pub manual_propagation: bool,

    /// A user-defined function allowing the user to specify the message id of a gossipsub message.
    /// The default value is to concatenate the source peer id with a sequence number. Setting this
    /// parameter allows the user to address packets arbitrarily. One example is content based
    /// addressing, where this function may be set to `hash(message)`. This would prevent messages
    /// of the same content from being duplicated.
    ///
    /// The function takes a `GossipsubMessage` as input and outputs a String to be interpreted as
    /// the message id.
    pub message_id_fn: fn(&GossipsubMessage) -> MessageId,
}

impl Default for GossipsubConfig {
    fn default() -> GossipsubConfig {
        GossipsubConfig {
            protocol_id: Cow::Borrowed(b"/meshsub/1.0.0"),
            history_length: 5,
            history_gossip: 3,
            mesh_n: 6,
            mesh_n_low: 4,
            mesh_n_high: 12,
            gossip_lazy: 6, // default to mesh_n
            heartbeat_initial_delay: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            fanout_ttl: Duration::from_secs(60),
            max_transmit_size: 2048,
            hash_topics: false, // default compatibility with floodsub
            no_source_id: false,
            manual_propagation: false,
            message_id_fn: |message| {
                // default message id is: source + sequence number
                let mut source_string = message.source.to_base58();
                source_string.push_str(&message.sequence_number.to_string());
                MessageId(source_string)
            },
        }
    }
}

pub struct GossipsubConfigBuilder {
    config: GossipsubConfig,
}

impl Default for GossipsubConfigBuilder {
    fn default() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder {
            config: GossipsubConfig::default(),
        }
    }
}

impl GossipsubConfigBuilder {
    // set default values
    pub fn new() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder::default()
    }

    pub fn protocol_id(&mut self, protocol_id: impl Into<Cow<'static, [u8]>>) -> &mut Self {
        self.config.protocol_id = protocol_id.into();
        self
    }

    pub fn history_length(&mut self, history_length: usize) -> &mut Self {
        assert!(
            history_length >= self.config.history_gossip,
            "The history_length must be greater than or equal to the history_gossip length"
        );
        self.config.history_length = history_length;
        self
    }

    pub fn history_gossip(&mut self, history_gossip: usize) -> &mut Self {
        assert!(
            self.config.history_length >= history_gossip,
            "The history_length must be greater than or equal to the history_gossip length"
        );
        self.config.history_gossip = history_gossip;
        self
    }

    pub fn mesh_n(&mut self, mesh_n: usize) -> &mut Self {
        assert!(
            self.config.mesh_n_low <= mesh_n && mesh_n <= self.config.mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        self.config.mesh_n = mesh_n;
        self
    }

    pub fn mesh_n_low(&mut self, mesh_n_low: usize) -> &mut Self {
        assert!(
            mesh_n_low <= self.config.mesh_n && self.config.mesh_n <= self.config.mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        self.config.mesh_n_low = mesh_n_low;
        self
    }

    pub fn mesh_n_high(&mut self, mesh_n_high: usize) -> &mut Self {
        assert!(
            self.config.mesh_n_low <= self.config.mesh_n && self.config.mesh_n <= mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        self.config.mesh_n_high = mesh_n_high;
        self
    }

    pub fn gossip_lazy(&mut self, gossip_lazy: usize) -> &mut Self {
        self.config.gossip_lazy = gossip_lazy;
        self
    }

    pub fn heartbeat_initial_delay(&mut self, heartbeat_initial_delay: Duration) -> &mut Self {
        self.config.heartbeat_initial_delay = heartbeat_initial_delay;
        self
    }
    pub fn heartbeat_interval(&mut self, heartbeat_interval: Duration) -> &mut Self {
        self.config.heartbeat_interval = heartbeat_interval;
        self
    }
    pub fn fanout_ttl(&mut self, fanout_ttl: Duration) -> &mut Self {
        self.config.fanout_ttl = fanout_ttl;
        self
    }
    pub fn max_transmit_size(&mut self, max_transmit_size: usize) -> &mut Self {
        self.config.max_transmit_size = max_transmit_size;
        self
    }

    pub fn hash_topics(&mut self) -> &mut Self {
        self.config.hash_topics = true;
        self
    }

    pub fn no_source_id(&mut self) -> &mut Self {
        self.config.no_source_id = true;
        self
    }

    pub fn manual_propagation(&mut self) -> &mut Self {
        self.config.manual_propagation = true;
        self
    }

    pub fn message_id_fn(&mut self, id_fn: fn(&GossipsubMessage) -> MessageId) -> &mut Self {
        self.config.message_id_fn = id_fn;
        self
    }

    pub fn build(&self) -> GossipsubConfig {
        self.config.clone()
    }
}

impl std::fmt::Debug for GossipsubConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut builder = f.debug_struct("GossipsubConfig");
        let _ = builder.field("protocol_id", &self.protocol_id);
        let _ = builder.field("history_length", &self.history_length);
        let _ = builder.field("history_gossip", &self.history_gossip);
        let _ = builder.field("mesh_n", &self.mesh_n);
        let _ = builder.field("mesh_n_low", &self.mesh_n_low);
        let _ = builder.field("mesh_n_high", &self.mesh_n_high);
        let _ = builder.field("gossip_lazy", &self.gossip_lazy);
        let _ = builder.field("heartbeat_initial_delay", &self.heartbeat_initial_delay);
        let _ = builder.field("heartbeat_interval", &self.heartbeat_interval);
        let _ = builder.field("fanout_ttl", &self.fanout_ttl);
        let _ = builder.field("max_transmit_size", &self.max_transmit_size);
        let _ = builder.field("hash_topics", &self.hash_topics);
        let _ = builder.field("no_source_id", &self.no_source_id);
        let _ = builder.field("manual_propagation", &self.manual_propagation);
        builder.finish()
    }
}
