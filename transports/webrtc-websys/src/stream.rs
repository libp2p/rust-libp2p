//! The WebRTC [Stream] over the Connection
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{AsyncRead, AsyncWrite};
use send_wrapper::SendWrapper;
use web_sys::RtcDataChannel;

use self::poll_data_channel::PollDataChannel;

mod poll_data_channel;

/// A stream over a WebRTC connection.
///
/// Backed by a WebRTC data channel.
pub struct Stream {
    /// Wrapper for the inner stream to make it Send
    inner: SendWrapper<libp2p_webrtc_utils::Stream<PollDataChannel>>,
}

pub(crate) type DropListener = SendWrapper<libp2p_webrtc_utils::DropListener<PollDataChannel>>;

impl Stream {
    pub(crate) fn new(data_channel: RtcDataChannel) -> (Self, DropListener) {
        let (inner, drop_listener) =
            libp2p_webrtc_utils::Stream::new(PollDataChannel::new(data_channel));

        (
            Self {
                inner: SendWrapper::new(inner),
            },
            SendWrapper::new(drop_listener),
        )
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_close(cx)
    }
}
