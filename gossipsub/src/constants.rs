// This is free and unencumbered software released into the public domain.

// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.

// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.

/// Sources: http://xlattice.sourceforge.net/components/protocol/kademlia/specs.html
/// https://www.kth.se/social/upload/516479a5f276545d6a965080/3-kademlia.pdf
/// http://www.scs.stanford.edu/%7Edm/home/papers/kpos.pdf (p. 3, col. 2)
pub const ALPHA: u32 = 3;

/// tRefresh in Kademlia implementations, sources:
/// http://xlattice.sourceforge.net/components/protocol/kademlia/specs.html#refresh
/// https://www.kth.se/social/upload/516479a5f276545d6a965080/3-kademlia.pdf
/// 1 hour, 3600 s
pub const KBUCKETS_TIMEOUT: u64 = 3600;

/// go gossipsub uses 1 s:
/// https://github.com/libp2p/go-floodsub/pull/67/files#diff-013da88fee30f5c765f693797e8b358dR30
/// However, https://www.rabbitmq.com/heartbeats.html#heartbeats-timeout uses 60 s, and
/// https://gist.github.com/gubatron/cd9cfa66839e18e49846#routing-table uses 15 minutes (15*60 s).
/// Let's make a conservative selection and choose 15 minutes for an alpha release.
pub const REQUEST_TIMEOUT: u64 = 750;

pub const OVERLAY_SIZE: usize = 10_000;
pub const C_RAND: u8 =  4;
pub const C_NEAR: u8 = 3;
pub const ACTIVE_LIST_SIZE: u8 = 7;
pub const PASSIVE_LIST_SIZE: u8 = 42;