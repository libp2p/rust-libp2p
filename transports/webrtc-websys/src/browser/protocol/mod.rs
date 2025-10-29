pub(crate) mod pb {
    include!(concat!(env!("OUT_DIR"), "/signaling.rs"));
}

pub(crate) mod protocol;
pub(crate) mod signaling;
pub(crate) mod upgrade;

pub use protocol::{SIGNALING_PROTOCOL_ID, SIGNALING_STREAM_PROTOCOL};
pub use signaling::{ConnectionCallbacks, ProtocolHandler, Signaling};
pub use upgrade::SignalingProtocolUpgrade;
