mod behaviour;
mod codec;
mod handler;
mod protocol;
mod substream;

pub use behaviour::Config;
pub use behaviour::Event;
pub use behaviour::RegisterError;
pub use behaviour::Rendezvous;
pub use codec::ErrorCode;
pub use codec::Registration;

/// If unspecified, rendezvous nodes should assume a TTL of 2h.
///
/// See https://github.com/libp2p/specs/blob/d21418638d5f09f2a4e5a1ceca17058df134a300/rendezvous/README.md#L116-L117.
pub const DEFAULT_TTL: i64 = 60 * 60 * 2;

/// By default, nodes should require a minimum TTL of 2h
///
/// https://github.com/libp2p/specs/tree/master/rendezvous#recommendations-for-rendezvous-points-configurations.
pub const MIN_TTL: i64 = 60 * 60 * 2;

/// By default, nodes should allow a maximum TTL of 72h
///
/// https://github.com/libp2p/specs/tree/master/rendezvous#recommendations-for-rendezvous-points-configurations.
pub const MAX_TTL: i64 = 60 * 60 * 72;
