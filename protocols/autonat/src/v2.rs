//! The second version of the autonat protocol.
//!
//! The implementation follows the [libp2p spec](https://github.com/libp2p/specs/blob/03718ef0f2dea4a756a85ba716ee33f97e4a6d6c/autonat/autonat-v2.md).
//!
//! The new version fixes the issues of the first version:
//! - The server now always dials back over a newly allocated port. This greatly reduces the risk of
//!   false positives that often occurred in the first version, when the clinet-server connection
//!   occurred over a hole-punched port.
//! - The server protects against DoS attacks by requiring the client to send more data to the
//!   server then the dial back puts on the client, thus making the protocol unatractive for an
//!   attacker.
//!
//! The protocol is separated into two parts:
//! - The client part, which is implemented in the `client` module. (The client is the party that
//!   wants to check if it is reachable from the outside.)
//! - The server part, which is implemented in the `server` module. (The server is the party
//!   performing reachability checks on behalf of the client.)
//!
//! The two can be used together.

use libp2p_swarm::StreamProtocol;

pub mod client;
pub(crate) mod protocol;
pub mod server;

pub(crate) mod generated {
    #![allow(unreachable_pub)]
    include!("v2/generated/mod.rs");
}

pub(crate) const DIAL_REQUEST_PROTOCOL: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-request");
pub(crate) const DIAL_BACK_PROTOCOL: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-back");

type Nonce = u64;
