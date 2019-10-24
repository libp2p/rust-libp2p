use std::borrow::Cow;
use std::time::Duration;

/// Configuration parameters that define the performance of the gossipsub network.
#[derive(Debug, Clone)]
pub struct GossipsubConfig {
    /// The protocol id to negotiate this protocol.
    pub protocol_id: Cow<'static, [u8]>,

    // Overlay network parameters.
    /// Number of heartbeats to keep in the `memcache`.
    pub history_length: usize,

    /// Number of past heartbeats to gossip about.
    pub history_gossip: usize,

    /// Target number of peers for the mesh network (D in the spec).
    pub mesh_n: usize,

    /// Minimum number of peers in mesh network before adding more (D_lo in the spec).
    pub mesh_n_low: usize,

    /// Maximum number of peers in mesh network before removing some (D_high in the spec).
    pub mesh_n_high: usize,

    /// Number of peers to emit gossip to during a heartbeat (D_lazy in the spec).
    pub gossip_lazy: usize,

    /// Initial delay in each heartbeat.
    pub heartbeat_initial_delay: Duration,

    /// Time between each heartbeat.
    pub heartbeat_interval: Duration,

    /// Time to live for fanout peers.
    pub fanout_ttl: Duration,

    /// The maximum byte size for each gossip.
    pub max_transmit_size: usize,

    /// Flag determining if gossipsub topics are hashed or sent as plain strings.
    pub hash_topics: bool,

    /// When set to `true`, prevents automatic forwarding of all received messages. This setting
    /// allows a user to validate the messages before propagating them to their peers. If set to
    /// true, the user must manually call `propagate_message()` on the behaviour to forward message
    /// once validated.
    pub manual_propagation: bool, 
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
            manual_propagation: false,
        }
    }
}

pub struct GossipsubConfigBuilder {
    /// The protocol id to negotiate this protocol.
    protocol_id: Cow<'static, [u8]>,

    /// Number of heartbeats to keep in the `memcache`.
    history_length: usize,

    /// Number of past heartbeats to gossip about.
    history_gossip: usize,

    /// Target number of peers for the mesh network (D in the spec).
    mesh_n: usize,

    /// Minimum number of peers in mesh network before adding more (D_lo in the spec).
    mesh_n_low: usize,

    /// Maximum number of peers in mesh network before removing some (D_high in the spec).
    mesh_n_high: usize,

    /// Number of peers to emit gossip to during a heartbeat (D_lazy in the spec).
    gossip_lazy: usize,

    /// Initial delay in each heartbeat.
    heartbeat_initial_delay: Duration,

    /// Time between each heartbeat.
    heartbeat_interval: Duration,

    /// Time to live for fanout peers.
    fanout_ttl: Duration,

    /// The maximum byte size for each message.
    max_transmit_size: usize,

    /// Flag determining if gossipsub topics are hashed or sent as plain strings.
    pub hash_topics: bool,

    /// Manually propagate messages to peers.
    pub manual_propagation: bool,
}

impl Default for GossipsubConfigBuilder {
    fn default() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder {
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
            hash_topics: false,
            manual_propagation: true,
        }
    }
}

impl GossipsubConfigBuilder {
    // set default values
    pub fn new() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder::default()
    }

    pub fn protocol_id(&mut self, protocol_id: impl Into<Cow<'static, [u8]>>) -> &mut Self {
        self.protocol_id = protocol_id.into();
        self
    }

    pub fn history_length(&mut self, history_length: usize) -> &mut Self {
        assert!(
            history_length >= self.history_gossip,
            "The history_length must be greater than or equal to the history_gossip length"
        );
        self.history_length = history_length;
        self
    }

    pub fn history_gossip(&mut self, history_gossip: usize) -> &mut Self {
        assert!(
            self.history_length >= history_gossip,
            "The history_length must be greater than or equal to the history_gossip length"
        );
        self.history_gossip = history_gossip;
        self
    }

    pub fn mesh_n(&mut self, mesh_n: usize) -> &mut Self {
        assert!(
            self.mesh_n_low <= mesh_n && mesh_n <= self.mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        self.mesh_n = mesh_n;
        self
    }

    pub fn mesh_n_low(&mut self, mesh_n_low: usize) -> &mut Self {
        assert!(
            mesh_n_low <= self.mesh_n && self.mesh_n <= self.mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        self.mesh_n_low = mesh_n_low;
        self
    }

    pub fn mesh_n_high(&mut self, mesh_n_high: usize) -> &mut Self {
        assert!(
            self.mesh_n_low <= self.mesh_n && self.mesh_n <= mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        self.mesh_n_high = mesh_n_high;
        self
    }

    pub fn gossip_lazy(&mut self, gossip_lazy: usize) -> &mut Self {
        self.gossip_lazy = gossip_lazy;
        self
    }

    pub fn heartbeat_initial_delay(&mut self, heartbeat_initial_delay: Duration) -> &mut Self {
        self.heartbeat_initial_delay = heartbeat_initial_delay;
        self
    }
    pub fn heartbeat_interval(&mut self, heartbeat_interval: Duration) -> &mut Self {
        self.heartbeat_interval = heartbeat_interval;
        self
    }
    pub fn fanout_ttl(&mut self, fanout_ttl: Duration) -> &mut Self {
        self.fanout_ttl = fanout_ttl;
        self
    }
    pub fn max_transmit_size(&mut self, max_transmit_size: usize) -> &mut Self {
        self.max_transmit_size = max_transmit_size;
        self
    }

    pub fn hash_topics(&mut self, hash_topics: bool) -> &mut Self {
        self.hash_topics = hash_topics;
        self
    }

    pub fn manual_propagation(&mut self, manual_propagation: bool) -> &mut Self {
        self.manual_propagation = manual_propagation;
        self
    }

    pub fn build(&self) -> GossipsubConfig {
        GossipsubConfig {
            protocol_id: self.protocol_id.clone(),
            history_length: self.history_length,
            history_gossip: self.history_gossip,
            mesh_n: self.mesh_n,
            mesh_n_low: self.mesh_n_low,
            mesh_n_high: self.mesh_n_high,
            gossip_lazy: self.gossip_lazy,
            heartbeat_initial_delay: self.heartbeat_initial_delay,
            heartbeat_interval: self.heartbeat_interval,
            fanout_ttl: self.fanout_ttl,
            max_transmit_size: self.max_transmit_size,
            hash_topics: self.hash_topics,
            manual_propagation: self.manual_propagation,
        }
    }
}
