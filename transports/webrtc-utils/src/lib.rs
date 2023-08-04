#![doc = include_str!("../README.md")]

pub mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub use self::webrtc::pb::{mod_Message::Flag, Message};
}

pub mod stream;
