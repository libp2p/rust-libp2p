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
    io,
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

/// A substream on top of a WebRTC data channel.
///
/// To be a proper libp2p substream, we need to implement [`AsyncRead`] and [`AsyncWrite`] as well
/// as support a half-closed state which we do by framing messages in a protobuf envelope.
pub struct Substream {
    io: Framed<Compat<PollDataChannel>, prost_codec::Codec<Message>>,
    state: State,
    read_buffer: Bytes,
    substream_id: u16,
}

impl Substream {
    /// Constructs a new `Substream`.
    pub(crate) fn new(data_channel: Arc<DataChannel>) -> Self {
        let mut inner = PollDataChannel::new(data_channel);

        // TODO: default buffer size is too small to fit some messages. Possibly remove once
        // https://github.com/webrtc-rs/webrtc/issues/273 is fixed.
        inner.set_read_buf_capacity(8192 * 10);

        let io = Framed::new(inner.compat(), prost_codec::Codec::new(MAX_MSG_LEN));
        let substream_id = io.get_ref().stream_identifier();

        Self {
            io,
            state: State::Open,
            read_buffer: Bytes::default(),
            substream_id,
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
                substream_id,
                read_buffer,
                io,
                state,
            } = &mut *self;

            match ready!(io_poll_next(io, cx))? {
                Some((flag, message)) => {
                    if let Some(flag) = flag {
                        state.handle_inbound_flag(flag, read_buffer, *substream_id);
                    }

                    debug_assert!(read_buffer.is_empty());
                    if let Some(message) = message {
                        *read_buffer = message.into();
                    }
                }
                None => {
                    state.handle_inbound_flag(Flag::Fin, read_buffer, *substream_id);
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
                substream_id,
                read_buffer,
                io,
                state,
            } = &mut *self;

            match io_poll_next(io, cx)? {
                Poll::Ready(Some((Some(flag), message))) => {
                    // Read side is closed. Discard any incoming messages.
                    drop(message);
                    // But still handle flags, e.g. a `Flag::StopSending`.
                    state.handle_inbound_flag(flag, read_buffer, *substream_id)
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

#[derive(Debug, Copy, Clone)]
enum State {
    Open,
    ReadClosed,
    WriteClosed,
    ClosingRead {
        /// Whether the write side of our channel was already closed.
        write_closed: bool,
        inner: Closing,
    },
    ClosingWrite {
        /// Whether the write side of our channel was already closed.
        read_closed: bool,
        inner: Closing,
    },
    BothClosed {
        reset: bool,
    },
}

/// Represents the state of closing one half (either read or write) of the connection.
///
/// Gracefully closing the read or write requires sending the `STOP_SENDING` or `FIN` flag respectively
/// and flushing the underlying connection.
#[derive(Debug, Copy, Clone)]
enum Closing {
    Requested,
    MessageSent,
}

impl State {
    /// Performs a state transition for a flag contained in an inbound message.
    fn handle_inbound_flag(&mut self, flag: Flag, buffer: &mut Bytes, substream_id: u16) {
        let current = *self;

        match (current, flag) {
            (Self::Open, Flag::Fin) => {
                *self = Self::ReadClosed;
            }
            (Self::WriteClosed, Flag::Fin) => {
                *self = Self::BothClosed { reset: false };
            }
            (Self::Open, Flag::StopSending) => {
                *self = Self::WriteClosed;
            }
            (Self::ReadClosed, Flag::StopSending) => {
                *self = Self::BothClosed { reset: false };
            }
            (_, Flag::Reset) => {
                buffer.clear();
                *self = Self::BothClosed { reset: true };
            }
            _ => {}
        }

        log::trace!("Transitioned from {current:?} to {self:?} on substream {substream_id}")
    }

    fn write_closed(&mut self) {
        match self {
            State::ClosingWrite {
                read_closed: true,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::BothClosed { reset: false };
            }
            State::ClosingWrite {
                read_closed: false,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::WriteClosed;
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingRead { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    fn close_write_message_sent(&mut self) {
        match self {
            State::ClosingWrite { inner, read_closed } => {
                debug_assert!(matches!(inner, Closing::Requested));

                *self = State::ClosingWrite {
                    read_closed: *read_closed,
                    inner: Closing::MessageSent,
                };
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingRead { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    fn read_closed(&mut self) {
        match self {
            State::ClosingRead {
                write_closed: true,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::BothClosed { reset: false };
            }
            State::ClosingRead {
                write_closed: false,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::ReadClosed;
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingWrite { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    fn close_read_message_sent(&mut self) {
        match self {
            State::ClosingRead {
                inner,
                write_closed,
            } => {
                debug_assert!(matches!(inner, Closing::Requested));

                *self = State::ClosingRead {
                    write_closed: *write_closed,
                    inner: Closing::MessageSent,
                };
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingWrite { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    /// Whether we should read from the stream in the [`AsyncWrite`] implementation.
    ///
    /// This is necessary for read-closed streams because we would otherwise not read any more flags from
    /// the socket.
    fn read_flags_in_async_write(&self) -> bool {
        matches!(self, Self::ReadClosed)
    }

    /// Acts as a "barrier" for [`AsyncRead::poll_read`].
    fn read_barrier(&self) -> io::Result<()> {
        use State::*;

        let kind = match self {
            Open
            | WriteClosed
            | ClosingWrite {
                read_closed: false, ..
            } => return Ok(()),
            ClosingWrite {
                read_closed: true, ..
            }
            | ReadClosed
            | ClosingRead { .. }
            | BothClosed { reset: false } => io::ErrorKind::BrokenPipe,
            BothClosed { reset: true } => io::ErrorKind::ConnectionReset,
        };

        Err(kind.into())
    }

    /// Acts as a "barrier" for [`AsyncWrite::poll_write`].
    fn write_barrier(&self) -> io::Result<()> {
        use State::*;

        let kind = match self {
            Open
            | ReadClosed
            | ClosingRead {
                write_closed: false,
                ..
            } => return Ok(()),
            ClosingRead {
                write_closed: true, ..
            }
            | WriteClosed
            | ClosingWrite { .. }
            | BothClosed { reset: false } => io::ErrorKind::BrokenPipe,
            BothClosed { reset: true } => io::ErrorKind::ConnectionReset,
        };

        Err(kind.into())
    }

    /// Acts as a "barrier" for [`AsyncWrite::poll_close`].
    fn close_write_barrier(&mut self) -> io::Result<Option<Closing>> {
        loop {
            match &self {
                State::WriteClosed => return Ok(None),

                State::ClosingWrite { inner, .. } => return Ok(Some(*inner)),

                State::Open => {
                    *self = Self::ClosingWrite {
                        read_closed: false,
                        inner: Closing::Requested,
                    };
                }
                State::ReadClosed => {
                    *self = Self::ClosingWrite {
                        read_closed: true,
                        inner: Closing::Requested,
                    };
                }

                State::ClosingRead {
                    write_closed: true, ..
                }
                | State::BothClosed { reset: false } => {
                    return Err(io::ErrorKind::BrokenPipe.into())
                }

                State::ClosingRead {
                    write_closed: false,
                    ..
                } => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "cannot close read half while closing write half",
                    ))
                }

                State::BothClosed { reset: true } => {
                    return Err(io::ErrorKind::ConnectionReset.into())
                }
            }
        }
    }

    /// Acts as a "barrier" for [`Substream::poll_close_read`].
    fn close_read_barrier(&mut self) -> io::Result<Option<Closing>> {
        loop {
            match self {
                State::ReadClosed => return Ok(None),

                State::ClosingRead { inner, .. } => return Ok(Some(*inner)),

                State::Open => {
                    *self = Self::ClosingRead {
                        write_closed: false,
                        inner: Closing::Requested,
                    };
                }
                State::WriteClosed => {
                    *self = Self::ClosingRead {
                        write_closed: true,
                        inner: Closing::Requested,
                    };
                }

                State::ClosingWrite {
                    read_closed: true, ..
                }
                | State::BothClosed { reset: false } => {
                    return Err(io::ErrorKind::BrokenPipe.into())
                }

                State::ClosingWrite {
                    read_closed: false, ..
                } => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "cannot close write half while closing read half",
                    ))
                }

                State::BothClosed { reset: true } => {
                    return Err(io::ErrorKind::ConnectionReset.into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asynchronous_codec::Encoder;
    use bytes::BytesMut;
    use prost::Message;
    use std::io::ErrorKind;
    use unsigned_varint::codec::UviBytes;

    #[test]
    fn cannot_read_after_receiving_fin() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::Fin, &mut Bytes::default(), 0);
        let error = open.read_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn cannot_read_after_closing_read() {
        let mut open = State::Open;

        open.close_read_barrier().unwrap();
        open.close_read_message_sent();
        open.read_closed();
        let error = open.read_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn cannot_write_after_receiving_stop_sending() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::StopSending, &mut Bytes::default(), 0);
        let error = open.write_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn cannot_write_after_closing_write() {
        let mut open = State::Open;

        open.close_write_barrier().unwrap();
        open.close_write_message_sent();
        open.write_closed();
        let error = open.write_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn everything_broken_after_receiving_reset() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::Reset, &mut Bytes::default(), 0);
        let error1 = open.read_barrier().unwrap_err();
        let error2 = open.write_barrier().unwrap_err();
        let error3 = open.close_write_barrier().unwrap_err();
        let error4 = open.close_read_barrier().unwrap_err();

        assert_eq!(error1.kind(), ErrorKind::ConnectionReset);
        assert_eq!(error2.kind(), ErrorKind::ConnectionReset);
        assert_eq!(error3.kind(), ErrorKind::ConnectionReset);
        assert_eq!(error4.kind(), ErrorKind::ConnectionReset);
    }

    #[test]
    fn should_read_flags_in_async_write_after_read_closed() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::Fin, &mut Bytes::default(), 0);

        assert!(open.read_flags_in_async_write())
    }

    #[test]
    fn cannot_read_or_write_after_receiving_fin_and_stop_sending() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::Fin, &mut Bytes::default(), 0);
        open.handle_inbound_flag(Flag::StopSending, &mut Bytes::default(), 0);

        let error1 = open.read_barrier().unwrap_err();
        let error2 = open.write_barrier().unwrap_err();

        assert_eq!(error1.kind(), ErrorKind::BrokenPipe);
        assert_eq!(error2.kind(), ErrorKind::BrokenPipe);
    }

    #[test]
    fn can_read_after_closing_write() {
        let mut open = State::Open;

        open.close_write_barrier().unwrap();
        open.close_write_message_sent();
        open.write_closed();

        open.read_barrier().unwrap();
    }

    #[test]
    fn can_write_after_closing_read() {
        let mut open = State::Open;

        open.close_read_barrier().unwrap();
        open.close_read_message_sent();
        open.read_closed();

        open.write_barrier().unwrap();
    }

    #[test]
    fn cannot_write_after_starting_close() {
        let mut open = State::Open;

        open.close_write_barrier().expect("to close in open");
        let error = open.write_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    }

    #[test]
    fn cannot_read_after_starting_close() {
        let mut open = State::Open;

        open.close_read_barrier().expect("to close in open");
        let error = open.read_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    }

    #[test]
    fn can_read_in_open() {
        let open = State::Open;

        let result = open.read_barrier();

        result.unwrap();
    }

    #[test]
    fn can_write_in_open() {
        let open = State::Open;

        let result = open.write_barrier();

        result.unwrap();
    }

    #[test]
    fn write_close_barrier_returns_ok_when_closed() {
        let mut open = State::Open;

        open.close_write_barrier().unwrap();
        open.close_write_message_sent();
        open.write_closed();

        let maybe = open.close_write_barrier().unwrap();

        assert!(maybe.is_none())
    }

    #[test]
    fn read_close_barrier_returns_ok_when_closed() {
        let mut open = State::Open;

        open.close_read_barrier().unwrap();
        open.close_read_message_sent();
        open.read_closed();

        let maybe = open.close_read_barrier().unwrap();

        assert!(maybe.is_none())
    }

    #[test]
    fn reset_flag_clears_buffer() {
        let mut open = State::Open;
        let mut buffer = Bytes::copy_from_slice(b"foobar");

        open.handle_inbound_flag(Flag::Reset, &mut buffer, 0);

        assert!(buffer.is_empty());
    }

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
