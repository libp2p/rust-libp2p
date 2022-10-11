use crate::message_proto::Message;
use crate::substream::MAX_MSG_LEN;
use asynchronous_codec::Framed;
use std::sync::Arc;
use tokio_util::compat::Compat;
use tokio_util::compat::TokioAsyncReadCompatExt;
use webrtc::data::data_channel::{DataChannel, PollDataChannel};

pub type FramedDC = Framed<Compat<PollDataChannel>, prost_codec::Codec<Message>>;

pub fn new(data_channel: Arc<DataChannel>) -> FramedDC {
    let mut inner = PollDataChannel::new(data_channel);

    // TODO: default buffer size is too small to fit some messages. Possibly remove once
    // https://github.com/webrtc-rs/webrtc/issues/273 is fixed.
    inner.set_read_buf_capacity(8192 * 10);

    Framed::new(inner.compat(), prost_codec::Codec::new(MAX_MSG_LEN))
}
