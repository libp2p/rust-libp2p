// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use asynchronous_codec::Framed;
use bytes::Bytes;
use futures::prelude::*;
use futures::ready;
use tokio_util::compat::Compat;
use tokio_util::compat::TokioAsyncReadCompatExt;
use webrtc::data::data_channel::DataChannel;
use webrtc::data::data_channel::PollDataChannel;

use state::{Closing, State};
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::message_proto::{message::Flag, Message};

mod state;

/// As long as message interleaving is not supported, the sender SHOULD limit the maximum message size to 16 KB to avoid monopolization.
// Source: <https://www.rfc-editor.org/rfc/rfc8831#name-transferring-user-data-on-a>
const MAX_MSG_LEN: usize = 16384; // 16kiB
/// Length of varint, in bytes.
const VARINT_LEN: usize = 2;
/// Overhead of the protobuf encoding, in bytes.
const PROTO_OVERHEAD: usize = 5;
/// Maximum length of data, in bytes.
const MAX_DATA_LEN: usize = MAX_MSG_LEN - VARINT_LEN - PROTO_OVERHEAD;

/// A substream on top of a WebRTC data channel.
///
/// To be a proper libp2p substream, we need to implement [`AsyncRead`] and [`AsyncWrite`] as well
/// as support a half-closed state which we do by framing messages in a protobuf envelope.
pub struct Substream {
    io: Framed<Compat<PollDataChannel>, prost_codec::Codec<Message>>,
    state: State,
    read_buffer: Bytes,
}

impl Substream {
    /// Constructs a new `Substream`.
    pub(crate) fn new(data_channel: Arc<DataChannel>) -> Self {
        let mut inner = PollDataChannel::new(data_channel);

        // TODO: default buffer size is too small to fit some messages. Possibly remove once
        // https://github.com/webrtc-rs/webrtc/issues/273 is fixed.
        inner.set_read_buf_capacity(8192 * 10);

        Self {
            io: Framed::new(inner.compat(), prost_codec::Codec::new(MAX_MSG_LEN)),
            state: State::Open,
            read_buffer: Bytes::default(),
        }
    }

    /// Gracefully closes the "read-half" of the substream.
    pub fn poll_close_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            match self.state.close_read_barrier()? {
                Some(Closing::Requested) => {
                    ready!(self.io.poll_ready_unpin(cx))?;

                    self.io.start_send_unpin(Message {
                        flag: Some(Flag::StopSending.into()),
                        message: None,
                    })?;
                    self.state.close_read_message_sent();

                    continue;
                }
                Some(Closing::MessageSent) => {
                    ready!(self.io.poll_flush_unpin(cx))?;

                    self.state.read_closed();

                    return Poll::Ready(Ok(()));
                }
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}

impl AsyncRead for Substream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            self.state.read_barrier()?;

            if !self.read_buffer.is_empty() {
                let n = std::cmp::min(self.read_buffer.len(), buf.len());
                let data = self.read_buffer.split_to(n);
                buf[0..n].copy_from_slice(&data[..]);

                return Poll::Ready(Ok(n));
            }

            let Self {
                read_buffer,
                io,
                state,
            } = &mut *self;

            match ready!(io_poll_next(io, cx))? {
                Some((flag, message)) => {
                    if let Some(flag) = flag {
                        state.handle_inbound_flag(flag, read_buffer);
                    }

                    debug_assert!(read_buffer.is_empty());
                    if let Some(message) = message {
                        *read_buffer = message.into();
                    }
                }
                None => {
                    state.handle_inbound_flag(Flag::Fin, read_buffer);
                    return Poll::Ready(Ok(0));
                }
            }
        }
    }
}

impl AsyncWrite for Substream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        while self.state.read_flags_in_async_write() {
            // TODO: In case AsyncRead::poll_read encountered an error or returned None earlier, we will poll the
            // underlying I/O resource once more. Is that allowed? How about introducing a state IoReadClosed?

            let Self {
                read_buffer,
                io,
                state,
            } = &mut *self;

            match io_poll_next(io, cx)? {
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

        ready!(self.io.poll_ready_unpin(cx))?;

        let n = usize::min(buf.len(), MAX_DATA_LEN);

        Pin::new(&mut self.io).start_send(Message {
            flag: None,
            message: Some(buf[0..n].into()),
        })?;

        Poll::Ready(Ok(n))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // TODO: Double check that we don't have to depend on self.state here.
        self.io.poll_flush_unpin(cx).map_err(Into::into)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            match self.state.close_write_barrier()? {
                Some(Closing::Requested) => {
                    ready!(self.io.poll_ready_unpin(cx))?;

                    self.io.start_send_unpin(Message {
                        flag: Some(Flag::Fin.into()),
                        message: None,
                    })?;
                    self.state.close_write_message_sent();

                    continue;
                }
                Some(Closing::MessageSent) => {
                    ready!(self.io.poll_flush_unpin(cx))?;

                    self.state.write_closed();

                    return Poll::Ready(Ok(()));
                }
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}

fn io_poll_next(
    io: &mut Framed<Compat<PollDataChannel>, prost_codec::Codec<Message>>,
    cx: &mut Context<'_>,
) -> Poll<io::Result<Option<(Option<Flag>, Option<Vec<u8>>)>>> {
    match ready!(io.poll_next_unpin(cx))
        .transpose()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
    {
        Some(Message { flag, message }) => {
            let flag = flag
                .map(|f| {
                    Flag::from_i32(f).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, ""))
                })
                .transpose()?;

            Poll::Ready(Ok(Some((flag, message))))
        }
        None => Poll::Ready(Ok(None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asynchronous_codec::Encoder;
    use bytes::BytesMut;
    use prost::Message;
    use unsigned_varint::codec::UviBytes;

    #[test]
    fn max_data_len() {
        // Largest possible message.
        let message = [0; MAX_DATA_LEN];

        let protobuf = crate::message_proto::Message {
            flag: Some(crate::message_proto::message::Flag::Fin.into()),
            message: Some(message.to_vec()),
        };

        let mut encoded_msg = BytesMut::new();
        protobuf
            .encode(&mut encoded_msg)
            .expect("BytesMut to have sufficient capacity.");
        assert_eq!(encoded_msg.len(), message.len() + PROTO_OVERHEAD);

        let mut uvi = UviBytes::default();
        let mut dst = BytesMut::new();
        uvi.encode(encoded_msg.clone().freeze(), &mut dst).unwrap();

        // Ensure the varint prefixed and protobuf encoded largest message is no longer than the
        // maximum limit specified in the libp2p WebRTC specification.
        assert_eq!(dst.len(), MAX_MSG_LEN);

        assert_eq!(dst.len() - encoded_msg.len(), VARINT_LEN);
    }
}
