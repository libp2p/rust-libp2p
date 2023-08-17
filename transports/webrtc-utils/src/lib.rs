#![doc = include_str!("../README.md")]

pub mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub use self::webrtc::pb::{mod_Message::Flag, Message};
}

pub mod error;
pub mod fingerprint;
pub mod noise;
pub mod sdp;
pub mod stream;

pub use error::Error;
