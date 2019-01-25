// Overlay parameters

/// The target number of peers in the mesh to gossip to and from.
pub const TARGET_MESH_DEGREE: u32 = 6;
/// Low water mark for the mesh degree, any lower and it could take longer to
/// find messages.
pub const LOW_WM_MESH_DEGREE: u32 = 4;
/// High water mark for the mesh degree, any higher and it could be too
/// much for bandwidth (particularly for low-end devices).
pub const HIGH_WM_MESH_DEGREE: u32 = 12;

// Gossip parameters
/// Length of gossip history
pub const GOSSIP_HIST_LEN: u32 = 5;

/// We get message IDs from up to (but not including) this index in the
/// `MCache's` history window.
pub const HISTORY_GOSSIP: u32 = 3;

/// The length of the total message history.
pub const MSG_HIST_LEN: u32 = 120;
/// The duration of messages in the seen cache, in seconds.
pub const SEEN_MSGS_CACHE: u32 = 120;

/// In milliseconds
pub const HEARTBEAT_INITIAL_DELAY: u32 = 100;
/// The interval at which to run the heartbeat procedure, in seconds
pub const HEARTBEAT_INTERVAL: u32 = 1;

/// The time to live for fanout in seconds.
pub const FANOUT_TTL: u32 = 60;
