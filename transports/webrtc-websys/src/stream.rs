//! The WebRTC [Stream] over the Connection
use self::framed_dc::FramedDc;
use bytes::Bytes;
use futures::{AsyncRead, AsyncWrite, Sink, SinkExt, StreamExt};
use libp2p_webrtc_utils::proto::{Flag, Message};
use libp2p_webrtc_utils::stream::{
    state::{Closing, State},
    MAX_DATA_LEN,
};
use send_wrapper::SendWrapper;
use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use web_sys::{RtcDataChannel, RtcDataChannelInit, RtcDataChannelType, RtcPeerConnection};

mod drop_listener;
mod framed_dc;
mod poll_data_channel;

pub(crate) use drop_listener::DropListener;

/// The Browser Default is Blob, so we must set ours to Arraybuffer explicitly
const ARRAY_BUFFER_BINARY_TYPE: RtcDataChannelType = RtcDataChannelType::Arraybuffer;

/// Builder for DataChannel
#[derive(Default, Debug)]
pub struct RtcDataChannelBuilder {
    negotiated: bool,
}

/// Builds a Data Channel with selected options and given peer connection
///
/// The default config is used in most cases, except when negotiating a Noise handshake
impl RtcDataChannelBuilder {
    /// Sets the DataChannel to be used for the Noise handshake
    /// Defaults to false
    pub fn negotiated(&mut self, negotiated: bool) -> &mut Self {
        self.negotiated = negotiated;
        self
    }

    /// Builds the WebRTC DataChannel from [RtcPeerConnection] with the given configuration
    pub fn build_with(&self, peer_connection: &RtcPeerConnection) -> RtcDataChannel {
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

/// Substream over the Connection
pub struct Stream {
    /// Wrapper for the inner stream to make it Send
    inner: SendWrapper<StreamInner>,
    /// State of the Stream
    state: State,
    /// Buffer for incoming data
    read_buffer: Bytes,
}

impl Stream {
    /// Create a new WebRTC Substream
    pub fn new(channel: RtcDataChannel) -> Self {
        Self {
            inner: SendWrapper::new(StreamInner::new(channel)),
            read_buffer: Bytes::new(),
            state: State::Open,
        }
    }
}

/// Inner Stream to make Sendable
struct StreamInner {
    io: FramedDc,
}

/// Inner Stream to make Sendable
impl StreamInner {
    pub fn new(channel: RtcDataChannel) -> Self {
        Self {
            io: framed_dc::new(channel),
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            self.state.read_barrier()?;

            // Return buffered data, if any
            if !self.read_buffer.is_empty() {
                let n = std::cmp::min(self.read_buffer.len(), buf.len());
                let data = self.read_buffer.split_to(n);
                buf[0..n].copy_from_slice(&data[..]);

                return Poll::Ready(Ok(n));
            }

            let Self {
                read_buffer,
                state,
                inner,
                ..
            } = &mut *self;

            match ready!(io_poll_next(&mut inner.io, cx))? {
                Some((flag, message)) => {
                    if let Some(flag) = flag {
                        state.handle_inbound_flag(flag, read_buffer);
                    }

                    debug_assert!(read_buffer.is_empty());
                    if let Some(message) = message {
                        *read_buffer = message.into();
                    }
                    // continue to loop
                }
                None => {
                    state.handle_inbound_flag(Flag::FIN, read_buffer);
                    return Poll::Ready(Ok(0));
                }
            }
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        while self.state.read_flags_in_async_write() {
            // TODO: In case AsyncRead::poll_read encountered an error or returned None earlier, we will poll the
            // underlying I/O resource once more. Is that allowed? How about introducing a state IoReadClosed?

            let Self {
                read_buffer,
                state,
                inner,
                ..
            } = &mut *self;

            match io_poll_next(&mut inner.io, cx)? {
                Poll::Ready(Some((Some(flag), message))) => {
                    // Read side is closed. Discard any incoming messages.
                    drop(message);
                    // But still handle flags, e.g. a `Flag::StopSending`.
                    state.handle_inbound_flag(flag, read_buffer)
                }
                Poll::Ready(Some((None, message))) => drop(message),
                Poll::Ready(None) | Poll::Pending => break,
            }
        }

        self.state.write_barrier()?;

        ready!(self.inner.io.poll_ready_unpin(cx))?;

        let n = usize::min(buf.len(), MAX_DATA_LEN);

        Pin::new(&mut self.inner.io).start_send(Message {
            flag: None,
            message: Some(buf[0..n].into()),
        })?;

        Poll::Ready(Ok(n))
    }

    /// Attempt to flush the object, ensuring that any buffered data reach their destination.
    ///
    /// On success, returns Poll::Ready(Ok(())).
    ///
    /// If flushing cannot immediately complete, this method returns Poll::Pending and
    /// arranges for the current task (via cx.waker().wake_by_ref()) to receive a
    /// notification when the object can make progress towards flushing.
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.inner.io.poll_flush_unpin(cx).map_err(Into::into)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        log::trace!("poll_closing");

        loop {
            match self.state.close_write_barrier()? {
                Some(Closing::Requested) => {
                    ready!(self.inner.io.poll_ready_unpin(cx))?;

                    log::debug!("Sending FIN flag");

                    self.inner.io.start_send_unpin(Message {
                        flag: Some(Flag::FIN),
                        message: None,
                    })?;
                    self.state.close_write_message_sent();

                    continue;
                }
                Some(Closing::MessageSent) => {
                    ready!(self.inner.io.poll_flush_unpin(cx))?;

                    self.state.write_closed();

                    // TODO: implement drop_notifier? Drop notifier built into the Browser as 'onclose' event
                    // let _ = self
                    //     .drop_notifier
                    //     .take()
                    //     .expect("to not close twice")
                    //     .send(GracefullyClosed {});

                    return Poll::Ready(Ok(()));
                }
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}

fn io_poll_next(
    io: &mut FramedDc,
    cx: &mut Context<'_>,
) -> Poll<io::Result<Option<(Option<Flag>, Option<Vec<u8>>)>>> {
    match ready!(io.poll_next_unpin(cx))
        .transpose()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
    {
        Some(Message { flag, message }) => Poll::Ready(Ok(Some((flag, message)))),
        None => Poll::Ready(Ok(None)),
    }
}
