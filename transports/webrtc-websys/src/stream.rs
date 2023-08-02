//! The Substream over the Connection
use self::framed_dc::FramedDc;
use bytes::Bytes;
use futures::{AsyncRead, AsyncWrite, Sink, SinkExt, StreamExt};
use libp2p_webrtc_utils::proto::{Flag, Message};
use libp2p_webrtc_utils::substream::{
    state::{Closing, State},
    MAX_DATA_LEN,
};
use log::debug;
use send_wrapper::SendWrapper;
use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use web_sys::{
    RtcDataChannel, RtcDataChannelInit, RtcDataChannelState, RtcDataChannelType, RtcPeerConnection,
};

mod drop_listener;
mod framed_dc;
mod poll_data_channel;

pub(crate) use drop_listener::DropListener;

// Max message size that can be sent to the DataChannel
const MAX_MESSAGE_SIZE: u32 = 16 * 1024;

// How much can be buffered to the DataChannel at once
const MAX_BUFFERED_AMOUNT: u32 = 16 * 1024 * 1024;

// How long time we wait for the 'bufferedamountlow' event to be emitted
const BUFFERED_AMOUNT_LOW_TIMEOUT: u32 = 30 * 1000;

#[derive(Debug)]
pub struct DataChannelConfig {
    negotiated: bool,
    id: u16,
    binary_type: RtcDataChannelType,
}

impl Default for DataChannelConfig {
    fn default() -> Self {
        Self {
            negotiated: false,
            id: 0,
            /// We set our default to Arraybuffer
            binary_type: RtcDataChannelType::Arraybuffer, // Blob is the default in the Browser,
        }
    }
}

/// Builds a Data Channel with selected options and given peer connection
///
///
impl DataChannelConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn negotiated(&mut self, negotiated: bool) -> &mut Self {
        self.negotiated = negotiated;
        self
    }

    /// Set a custom id for the Data Channel
    pub fn id(&mut self, id: u16) -> &mut Self {
        self.id = id;
        self
    }

    // Set the binary type for the Data Channel
    // TODO: We'll likely never use this, all channels are created with Arraybuffer?
    // pub fn binary_type(&mut self, binary_type: RtcDataChannelType) -> &mut Self {
    //     self.binary_type = binary_type;
    //     self
    // }

    /// Opens a WebRTC DataChannel from [RtcPeerConnection] with the selected [DataChannelConfig]
    /// We can create `ondatachannel` future before building this
    /// then await it after building this
    pub fn create_from(&self, peer_connection: &RtcPeerConnection) -> RtcDataChannel {
        const LABEL: &str = "";

        let dc = match self.negotiated {
            true => {
                let mut data_channel_dict = RtcDataChannelInit::new();
                data_channel_dict.negotiated(true).id(self.id);
                debug!("Creating negotiated DataChannel");
                peer_connection
                    .create_data_channel_with_data_channel_dict(LABEL, &data_channel_dict)
            }
            false => {
                debug!("Creating NON negotiated DataChannel");
                peer_connection.create_data_channel(LABEL)
            }
        };
        dc.set_binary_type(self.binary_type);
        dc
    }
}

/// Substream over the Connection
pub struct WebRTCStream {
    inner: SendWrapper<StreamInner>,
    state: State,
    read_buffer: Bytes,
}

impl WebRTCStream {
    /// Create a new Substream
    pub fn new(channel: RtcDataChannel) -> Self {
        Self {
            inner: SendWrapper::new(StreamInner::new(channel)),
            read_buffer: Bytes::new(),
            state: State::Open,
        }
    }

    /// Return the underlying RtcDataChannel
    pub fn channel(&self) -> &RtcDataChannel {
        self.inner.io.as_ref()
    }
}

struct StreamInner {
    io: FramedDc,
    // channel: RtcDataChannel,
    // onclose_fut: CbFuture<()>,
    state: State,
    // Resolves when the data channel is opened.
    // onopen_fut: CbFuture<()>,
    // onmessage_fut: CbFuture<Vec<u8>>, // incoming_data: FusedJsPromise,
    // message_queue: Vec<u8>,
    // ondatachannel_fut: CbFuture<RtcDataChannel>,
}

impl StreamInner {
    pub fn new(channel: RtcDataChannel) -> Self {
        Self {
            io: framed_dc::new(channel),
            state: State::Open, // always starts open
        }
    }

    // fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
    //     // If we have leftovers from a previous read, then use them.
    //     // Otherwise read new data.
    //     let data = match self.read_leftovers.take() {
    //         Some(data) => data,
    //         None => {
    //             match ready!(self.poll_reader_read(cx))? {
    //                 Some(data) => data,
    //                 // EOF
    //                 None => return Poll::Ready(Ok(0)),
    //             }
    //         }
    //     };

    //     if data.byte_length() == 0 {
    //         return Poll::Ready(Ok(0));
    //     }

    //     let out_len = data.byte_length().min(buf.len() as u32);
    //     data.slice(0, out_len).copy_to(&mut buf[..out_len as usize]);

    //     let leftovers = data.slice(out_len, data.byte_length());

    //     if leftovers.byte_length() > 0 {
    //         self.read_leftovers = Some(leftovers);
    //     }

    //     Poll::Ready(Ok(out_len as usize))
    // }
}

impl AsyncRead for WebRTCStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            self.inner.state.read_barrier()?;

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

        // Kick the can down the road to inner.io.poll_ready_unpin
        // ready!(self.inner.io.poll_ready_unpin(cx))?;

        // let data = ready!(self.inner.onmessage_fut.poll_unpin(cx));
        // debug!("poll_read, {:?}", data);
        // let data_len = data.len();
        // let buf_len = buf.len();
        // debug!("poll_read [data, buffer]: [{:?}, {}]", data_len, buf_len);
        // let len = std::cmp::min(data_len, buf_len);
        // buf[..len].copy_from_slice(&data[..len]);
        // Poll::Ready(Ok(len))
    }
}

impl AsyncWrite for WebRTCStream {
    /// Attempt to write bytes from buf into the object.
    /// On success, returns Poll::Ready(Ok(num_bytes_written)).
    /// If the object is not ready for writing,
    /// the method returns Poll::Pending and
    /// arranges for the current task (via cx.waker().wake_by_ref())
    /// to receive a notification when the object becomes writable
    /// or is closed.
    ///
    /// In WebRTC DataChannels, we can always write to the channel
    /// so we don't need to poll the channel for writability
    /// as long as the channel is open
    ///
    /// So we need access to the channel or peer_connection,
    /// and the State of the Channel (DataChannel.readyState)
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
        debug!("poll_closing");

        loop {
            match self.state.close_write_barrier()? {
                Some(Closing::Requested) => {
                    ready!(self.inner.io.poll_ready_unpin(cx))?;

                    debug!("Sending FIN flag");

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
                    // TODO: implement drop_notifier
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

        // match ready!(self.inner.io.poll_close_unpin(cx)) {
        //     Ok(()) => {
        //         debug!("poll_close, Ok(())");
        //         self.state.close_write_message_sent();
        //         Poll::Ready(Ok(()))
        //     }
        //     Err(e) => {
        //         debug!("poll_close, Err({:?})", e);
        //         Poll::Ready(Err(io::Error::new(
        //             io::ErrorKind::Other,
        //             "poll_close failed",
        //         )))
        //     }
        // }
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
