//! The WebRTC [Stream] over the Connection
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{AsyncRead, AsyncWrite};
use send_wrapper::SendWrapper;
use web_sys::{RtcDataChannel, RtcDataChannelInit, RtcDataChannelType, RtcPeerConnection};

use self::poll_data_channel::PollDataChannel;

mod poll_data_channel;

/// The Browser Default is Blob, so we must set ours to Arraybuffer explicitly
const ARRAY_BUFFER_BINARY_TYPE: RtcDataChannelType = RtcDataChannelType::Arraybuffer;

/// Builder for DataChannel
#[derive(Default, Debug)]
pub(crate) struct RtcDataChannelBuilder {
    negotiated: bool,
}

/// Builds a Data Channel with selected options and given peer connection
///
/// The default config is used in most cases, except when negotiating a Noise handshake
impl RtcDataChannelBuilder {
    /// Sets the DataChannel to be used for the Noise handshake
    /// Defaults to false
    pub(crate) fn negotiated(&mut self, negotiated: bool) -> &mut Self {
        self.negotiated = negotiated;
        self
    }

    /// Builds the WebRTC DataChannel from [RtcPeerConnection] with the given configuration
    pub(crate) fn build_with(&self, peer_connection: &RtcPeerConnection) -> RtcDataChannel {
        const LABEL: &str = "";

        let dc = match self.negotiated {
            true => {
                let mut data_channel_dict = RtcDataChannelInit::new();
                data_channel_dict.negotiated(true).id(0); // id is only ever set to zero when negotiated is true
                peer_connection
                    .create_data_channel_with_data_channel_dict(LABEL, &data_channel_dict)
            }
            false => peer_connection.create_data_channel(LABEL),
        };
        dc.set_binary_type(ARRAY_BUFFER_BINARY_TYPE); // Hardcoded here, it's the only type we use
        dc
    }
}

/// Stream over the Connection
pub struct Stream {
    /// Wrapper for the inner stream to make it Send
    inner: SendWrapper<libp2p_webrtc_utils::Stream<PollDataChannel>>,
}

pub(crate) type DropListener = libp2p_webrtc_utils::DropListener<PollDataChannel>;

impl Stream {
    pub(crate) fn new(data_channel: RtcDataChannel) -> (Self, DropListener) {
        let (inner, drop_listener) =
            libp2p_webrtc_utils::Stream::new(PollDataChannel::new(data_channel));

        (
            Self {
                inner: SendWrapper::new(inner),
            },
            drop_listener,
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
