#![doc = include_str!("../README.md")]

mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub use self::webrtc::pb::{mod_Message::Flag, Message};
}

pub mod error;
pub mod fingerprint;
pub mod noise;
pub mod sdp;
mod stream;
pub mod transport;

pub use error::Error;
pub use stream::{DropListener, Stream};
pub use transport::parse_webrtc_dial_addr;
