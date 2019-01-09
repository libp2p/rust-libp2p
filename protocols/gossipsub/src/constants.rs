// Overlay parameters
pub const TARGET_MESH_DEGREE: u32 = 6;
pub const LOW_WM_MESH_DEGREE: u32 = 4;      // low water mark for mesh degree
pub const HIGH_WM_MESH_DEGREE: u32 = 12;    // high water mark for mesh degree

// Gossip parameters
pub const GOSSIP_HIST_LEN: u32 = 5;         // length of gossip history
pub const HISTORY_GOSSIP: u32 = 3;

pub const MSG_HIST_LEN: u32 = 120;          // length of total message history
pub const SEEN_MSGS_CACHE: u32 = 120;

// hearbeat interval
pub const HEARTBEAT_INITIAL_DELAY: u32 = 100; // milliseconds
pub const HEARTBEAT_INTERVAL: u32 = 1;   // seconds.

pub const FANOUT_TTL: u32 = 60; // seconds
