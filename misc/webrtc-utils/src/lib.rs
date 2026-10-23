mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub use self::webrtc_pb::{Message, message::Flag};
}

mod fingerprint;
pub mod noise;
pub mod sdp;
mod stream;
mod transport;

pub use fingerprint::{Fingerprint, SHA256};
pub use stream::{
    DEFAULT_MAX_MESSAGE_SIZE, DropListener, MAX_MSG_LEN, MIN_MESSAGE_SIZE, Stream, StreamConfig,
};
pub use transport::parse_webrtc_dial_addr;
