mod signaling;
mod transport;
mod stream;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/signaling.rs"));
}

pub(crate) use signaling::SignalingProtocol;

pub use self::{
    signaling::{Signaling, SIGNALING_PROTOCOL_ID},
    transport::BrowserTransport,
    stream::ProtobufStream
};
