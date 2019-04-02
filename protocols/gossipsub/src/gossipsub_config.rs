use std::time::Duration;

/// Configuration parameters that define the performance of the gossipsub network.
#[derive(Debug, Clone)]
pub struct GossipsubConfig {
    /// Overlay network parameters.
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
    pub max_gossip_size: usize,
    /// Timeout before the protocol handler terminates the stream.
    pub inactivity_timeout: Duration,
}

impl Default for GossipsubConfig {
    fn default() -> GossipsubConfig {
        GossipsubConfig {
            history_length: 5,
            history_gossip: 3,
            mesh_n: 6,
            mesh_n_low: 4,
            mesh_n_high: 12,
            gossip_lazy: 6, // default to mesh_n
            heartbeat_initial_delay: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            fanout_ttl: Duration::from_secs(60),
            max_gossip_size: 2048,
            inactivity_timeout: Duration::from_secs(60),
        }
    }
}

pub struct GossipsubConfigBuilder {
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
    /// The maximum byte size for each gossip.
    max_gossip_size: usize,
    /// The inactivity time before a peer is disconnected.
    inactivity_timeout: Duration,
}

impl Default for GossipsubConfigBuilder {
    fn default() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder {
            history_length: 5,
            history_gossip: 3,
            mesh_n: 6,
            mesh_n_low: 4,
            mesh_n_high: 12,
            gossip_lazy: 6, // default to mesh_n
            heartbeat_initial_delay: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            fanout_ttl: Duration::from_secs(60),
            max_gossip_size: 2048,
            inactivity_timeout: Duration::from_secs(60),
        }
    }
}

impl GossipsubConfigBuilder {
    // set default values
    pub fn new() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder::default()
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
    pub fn max_gossip_size(&mut self, max_gossip_size: usize) -> &mut Self {
        self.max_gossip_size = max_gossip_size;
        self
    }

    pub fn inactivity_timeout(&mut self, inactivity_timeout: Duration) -> &mut Self {
        self.inactivity_timeout = inactivity_timeout;
        self
    }

    pub fn build(&self) -> GossipsubConfig {
        GossipsubConfig {
            history_length: self.history_length,
            history_gossip: self.history_gossip,
            mesh_n: self.mesh_n,
            mesh_n_low: self.mesh_n_low,
            mesh_n_high: self.mesh_n_high,
            gossip_lazy: self.gossip_lazy,
            heartbeat_initial_delay: self.heartbeat_initial_delay,
            heartbeat_interval: self.heartbeat_interval,
            fanout_ttl: self.fanout_ttl,
            max_gossip_size: self.max_gossip_size,
            inactivity_timeout: self.inactivity_timeout,
        }
    }
}
