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

use std::{
    fmt, io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::message_proto::message::Flag;
use crate::message_proto::Message;

/// Maximum length of a message, in bytes.
const MAX_MSG_LEN: usize = 16384; // 16kiB
/// Length of varint, in bytes.
const VARINT_LEN: usize = 2;
/// Overhead of the protobuf encoding, in bytes.
const PROTO_OVERHEAD: usize = 5;
/// Maximum length of data, in bytes.
const MAX_DATA_LEN: usize = MAX_MSG_LEN - VARINT_LEN - PROTO_OVERHEAD;

/// Substream is a wrapper around [`RTCPollDataChannel`] implementing futures [`AsyncRead`] /
/// [`AsyncWrite`] and message framing (as per specification).
///
/// #[derive(Debug)]
pub struct Substream {
    io: Framed<Compat<PollDataChannel>, prost_codec::Codec<Message>>,
    state: State,
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
            state: State::Open {
                read_buffer: Default::default(),
            },
        }
    }

    /// StreamIdentifier returns the Stream identifier associated to the stream.
    pub fn stream_identifier(&self) -> u16 {
        self.io.get_ref().stream_identifier()
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
                .map(|f| Flag::from_i32(f).ok_or(io::Error::new(io::ErrorKind::InvalidData, "")))
                .transpose()?;

            Poll::Ready(Ok(Some((flag, message))))
        }
        None => Poll::Ready(Ok(None)),
    }
}

impl AsyncRead for Substream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            if let Some(read_buffer) = self.state.non_empty_read_buffer_mut() {
                let n = std::cmp::min(read_buffer.len(), buf.len());
                let data = read_buffer.split_to(n);
                buf[0..n].copy_from_slice(&data[..]);

                return Poll::Ready(Ok(n));
            }

            let substream_id = self.stream_identifier();
            let Self { state, io } = &mut *self;

            let read_buffer = match state {
                State::Open { read_buffer } | State::WriteClosed { read_buffer } => read_buffer,
                State::ReadClosed { read_buffer, .. }
                | State::ReadWriteClosed { read_buffer, .. } => {
                    assert!(read_buffer.is_empty());
                    return Poll::Ready(Ok(0));
                }
                State::ReadReset | State::ReadResetWriteClosed => {
                    return Poll::Ready(Err(io::Error::from(io::ErrorKind::ConnectionReset)));
                }
                State::Poisoned => unreachable!(),
            };

            match ready!(io_poll_next(io, cx))? {
                Some((flag, message)) => {
                    assert!(read_buffer.is_empty());
                    if let Some(message) = message {
                        *read_buffer = message.into();
                    }

                    if let Some(flag) = flag {
                        self.state.handle_flag(flag, substream_id)
                    };
                }
                None => {
                    self.state.handle_flag(Flag::Fin, substream_id);
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
        let substream_id = self.stream_identifier();
        // Handle flags iff read side closed.
        loop {
            match self.state {
                State::ReadClosed { .. } | State::ReadReset =>
                // TODO: In case AsyncRead::poll_read encountered an error or returned None earlier, we will poll the
                // underlying I/O resource once more. Is that allowed? How about introducing a state IoReadClosed?
                {
                    match io_poll_next(&mut self.io, cx)? {
                        Poll::Ready(Some((Some(flag), message))) => {
                            // Read side is closed. Discard any incoming messages.
                            drop(message);
                            // But still handle flags, e.g. a `Flag::StopSending`.
                            self.state.handle_flag(flag, substream_id)
                        }
                        Poll::Ready(Some((None, message))) => drop(message),
                        Poll::Ready(None) | Poll::Pending => break,
                    }
                }
                _ => break,
            }
        }

        match self.state {
            State::WriteClosed { .. }
            | State::ReadWriteClosed { .. }
            | State::ReadResetWriteClosed => return Poll::Ready(Ok(0)),
            State::Open { .. } | State::ReadClosed { .. } | State::ReadReset => {}
            State::Poisoned => todo!(),
        }

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
        match &self.state {
            State::WriteClosed { .. }
            | State::ReadWriteClosed { .. }
            | State::ReadResetWriteClosed { .. } => {}

            State::Open { .. } | State::ReadClosed { .. } | State::ReadReset => {
                ready!(self.io.poll_ready_unpin(cx))?;
                Pin::new(&mut self.io).start_send(Message {
                    flag: Some(Flag::Fin.into()),
                    message: None,
                })?;

                match std::mem::replace(&mut self.state, State::Poisoned) {
                    State::Open { read_buffer } => self.state = State::WriteClosed { read_buffer },
                    State::ReadClosed { read_buffer } => {
                        self.state = State::ReadWriteClosed { read_buffer }
                    }
                    State::ReadReset => self.state = State::ReadResetWriteClosed,
                    State::WriteClosed { .. }
                    | State::ReadWriteClosed { .. }
                    | State::ReadResetWriteClosed
                    | State::Poisoned => {
                        unreachable!()
                    }
                }
            }

            State::Poisoned => todo!(),
        }

        // TODO: Is flush the correct thing here? We don't want the underlying layer to close both write and read.
        self.io.poll_flush_unpin(cx).map_err(Into::into)
    }
}

enum State {
    Open { read_buffer: Bytes },
    WriteClosed { read_buffer: Bytes },
    ReadClosed { read_buffer: Bytes },
    ReadWriteClosed { read_buffer: Bytes },
    ReadReset,
    ReadResetWriteClosed,
    Poisoned,
}

impl State {
    fn handle_flag(&mut self, flag: Flag, substream_id: u16) {
        let old_state = format!("{}", self);
        match (std::mem::replace(self, State::Poisoned), flag) {
            // StopSending
            (
                State::Open { read_buffer } | State::WriteClosed { read_buffer },
                Flag::StopSending,
            ) => {
                *self = State::WriteClosed { read_buffer };
            }

            (
                State::ReadClosed { read_buffer } | State::ReadWriteClosed { read_buffer },
                Flag::StopSending,
            ) => {
                *self = State::ReadWriteClosed { read_buffer };
            }

            (State::ReadReset | State::ReadResetWriteClosed, Flag::StopSending) => {
                *self = State::ReadResetWriteClosed;
            }

            // Fin
            (State::Open { read_buffer } | State::ReadClosed { read_buffer }, Flag::Fin) => {
                *self = State::ReadClosed { read_buffer };
            }

            (
                State::WriteClosed { read_buffer } | State::ReadWriteClosed { read_buffer },
                Flag::Fin,
            ) => {
                *self = State::ReadWriteClosed { read_buffer };
            }

            (State::ReadReset, Flag::Fin) => *self = State::ReadReset,

            (State::ReadResetWriteClosed, Flag::Fin) => *self = State::ReadResetWriteClosed,

            // Reset
            (State::ReadClosed { .. } | State::ReadReset | State::Open { .. }, Flag::Reset) => {
                *self = State::ReadReset
            }

            (
                State::ReadWriteClosed { .. }
                | State::WriteClosed { .. }
                | State::ReadResetWriteClosed,
                Flag::Reset,
            ) => *self = State::ReadResetWriteClosed,

            (State::Poisoned, _) => unreachable!(),
        }

        log::debug!(
            "substream={}: got flag {:?}, moved from {} to {}",
            substream_id,
            flag,
            old_state,
            *self
        );
    }

    /// Returns a reference to the underlying buffer if possible and the buffer is not empty.
    fn non_empty_read_buffer_mut(&mut self) -> Option<&mut Bytes> {
        match self {
            State::Open { read_buffer }
            | State::WriteClosed { read_buffer }
            | State::ReadClosed { read_buffer }
            | State::ReadWriteClosed { read_buffer }
                if !read_buffer.is_empty() =>
            {
                Some(read_buffer)
            }
            State::Poisoned => unreachable!(),
            _ => None,
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            State::Open { .. } => write!(f, "Open"),
            State::WriteClosed { .. } => write!(f, "WriteClosed"),
            State::ReadClosed { .. } => write!(f, "ReadClosed"),
            State::ReadWriteClosed { .. } => write!(f, "ReadWriteClosed"),
            State::ReadReset => write!(f, "ReadReset"),
            State::ReadResetWriteClosed => write!(f, "ReadResetWriteClosed"),
            State::Poisoned => write!(f, "Poisoned"),
        }
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
