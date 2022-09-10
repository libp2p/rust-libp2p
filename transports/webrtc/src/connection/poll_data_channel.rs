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
use webrtc::data::data_channel::PollDataChannel as RTCPollDataChannel;

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::message_proto::message::Flag;
use crate::message_proto::Message;

/// A wrapper around [`RTCPollDataChannel`] implementing futures [`AsyncRead`] / [`AsyncWrite`].
// TODO
// #[derive(Debug)]
pub struct PollDataChannel {
    io: Framed<Compat<RTCPollDataChannel>, prost_codec::Codec<Message>>,
    state: State,
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
    fn handle_flag(&mut self, flag: Flag) {
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
    }

    fn read_buffer_mut(&mut self) -> Option<&mut Bytes> {
        match self {
            State::Open { read_buffer } => Some(read_buffer),
            State::WriteClosed { read_buffer } => Some(read_buffer),
            State::ReadClosed { read_buffer } => Some(read_buffer),
            State::ReadWriteClosed { read_buffer } => Some(read_buffer),
            State::ReadReset => None,
            State::ReadResetWriteClosed => None,
            State::Poisoned => todo!(),
        }
    }
}

impl PollDataChannel {
    /// Constructs a new `PollDataChannel`.
    pub fn new(data_channel: Arc<DataChannel>) -> Self {
        Self {
            io: Framed::new(
                RTCPollDataChannel::new(data_channel).compat(),
                // TODO: Fix MAX
                prost_codec::Codec::new(usize::MAX),
            ),
            state: State::Open {
                read_buffer: Default::default(),
            },
        }
    }

    /// Get back the inner data_channel.
    pub fn into_inner(self) -> RTCPollDataChannel {
        self.io.into_inner().into_inner()
    }

    /// Obtain a clone of the inner data_channel.
    pub fn clone_inner(&self) -> RTCPollDataChannel {
        self.io.get_ref().clone()
    }

    /// MessagesSent returns the number of messages sent
    pub fn messages_sent(&self) -> usize {
        self.io.get_ref().messages_sent()
    }

    /// MessagesReceived returns the number of messages received
    pub fn messages_received(&self) -> usize {
        self.io.get_ref().messages_received()
    }

    /// BytesSent returns the number of bytes sent
    pub fn bytes_sent(&self) -> usize {
        self.io.get_ref().bytes_sent()
    }

    /// BytesReceived returns the number of bytes received
    pub fn bytes_received(&self) -> usize {
        self.io.get_ref().bytes_received()
    }

    /// StreamIdentifier returns the Stream identifier associated to the stream.
    pub fn stream_identifier(&self) -> u16 {
        self.io.get_ref().stream_identifier()
    }

    /// BufferedAmount returns the number of bytes of data currently queued to be
    /// sent over this stream.
    pub fn buffered_amount(&self) -> usize {
        self.io.get_ref().buffered_amount()
    }

    /// BufferedAmountLowThreshold returns the number of bytes of buffered outgoing
    /// data that is considered "low." Defaults to 0.
    pub fn buffered_amount_low_threshold(&self) -> usize {
        self.io.get_ref().buffered_amount_low_threshold()
    }

    /// Set the capacity of the temporary read buffer (default: 8192).
    pub fn set_read_buf_capacity(&mut self, capacity: usize) {
        self.io.get_mut().set_read_buf_capacity(capacity)
    }

    fn io_poll_next(
        io: &mut Framed<Compat<RTCPollDataChannel>, prost_codec::Codec<Message>>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Option<(Option<Flag>, Option<Vec<u8>>)>>> {
        match ready!(io.poll_next_unpin(cx))
            .transpose()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        {
            Some(Message { flag, message }) => {
                let flag = flag
                    .map(|f| {
                        Flag::from_i32(f).ok_or(io::Error::new(io::ErrorKind::InvalidData, ""))
                    })
                    .transpose()?;

                Poll::Ready(Ok(Some((flag, message))))
            }
            None => Poll::Ready(Ok(None)),
        }
    }
}

impl AsyncRead for PollDataChannel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            if let Some(read_buffer) = self.state.read_buffer_mut() {
                if !read_buffer.is_empty() {
                    let n = std::cmp::min(read_buffer.len(), buf.len());
                    let data = read_buffer.split_to(n);
                    buf[0..n].copy_from_slice(&data[..]);

                    return Poll::Ready(Ok(n));
                }
            }

            let PollDataChannel { state, io } = &mut *self;

            let read_buffer = match state {
                State::Open {
                    ref mut read_buffer,
                }
                | State::WriteClosed {
                    ref mut read_buffer,
                } => read_buffer,
                State::ReadClosed { read_buffer, .. }
                | State::ReadWriteClosed { read_buffer, .. } => {
                    assert!(read_buffer.is_empty());
                    return Poll::Ready(Ok(0));
                }
                State::ReadReset | State::ReadResetWriteClosed => {
                    // TODO: Is `""` valid?
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, "")));
                }
                State::Poisoned => todo!(),
            };

            match ready!(Self::io_poll_next(io, cx))? {
                Some((flag, message)) => {
                    assert!(read_buffer.is_empty());
                    if let Some(message) = message {
                        *read_buffer = message.into();
                    }

                    if let Some(flag) = flag {
                        self.state.handle_flag(flag)
                    };
                }
                None => {
                    self.state.handle_flag(Flag::Fin);
                    return Poll::Ready(Ok(0));
                }
            }
        }
    }
}

impl AsyncWrite for PollDataChannel {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Handle flags iff read side closed.
        loop {
            match self.state {
                State::ReadClosed { .. } | State::ReadReset =>
                // TODO: In case AsyncRead::poll_read encountered an error or returned None earlier, we will poll the
                // underlying I/O resource once more. Is that allowed? How about introducing a state IoReadClosed?
                {
                    match Self::io_poll_next(&mut self.io, cx)? {
                        Poll::Ready(Some((Some(flag), message))) => {
                            // Read side is closed. Discard any incoming messages.
                            drop(message);
                            // But still handle flags, e.g. a `Flag::StopSending`.
                            self.state.handle_flag(flag)
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

        Pin::new(&mut self.io).start_send(Message {
            flag: None,
            message: Some(buf.into()),
        })?;

        Poll::Ready(Ok(buf.len()))
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
