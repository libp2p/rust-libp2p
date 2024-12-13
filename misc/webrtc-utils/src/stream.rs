// Copyright 2022 Parity Technologies (UK) Ltd.
// Copyright 2023 Protocol Labs.
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

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{channel::oneshot, prelude::*, ready};

use crate::{
    proto::{Flag, Message},
    stream::{
        drop_listener::GracefullyClosed,
        framed_dc::FramedDc,
        state::{Closing, State},
    },
};

mod drop_listener;
mod framed_dc;
mod state;

/// Maximum length of a message.
///
/// "As long as message interleaving is not supported, the sender SHOULD limit the maximum message
/// size to 16 KB to avoid monopolization."
/// Source: <https://www.rfc-editor.org/rfc/rfc8831#name-transferring-user-data-on-a>
pub const MAX_MSG_LEN: usize = 16 * 1024;
/// Length of varint, in bytes.
const VARINT_LEN: usize = 2;
/// Overhead of the protobuf encoding, in bytes.
const PROTO_OVERHEAD: usize = 5;
/// Maximum length of data, in bytes.
const MAX_DATA_LEN: usize = MAX_MSG_LEN - VARINT_LEN - PROTO_OVERHEAD;

pub use drop_listener::DropListener;
/// A stream backed by a WebRTC data channel.
///
/// To be a proper libp2p stream, we need to implement [`AsyncRead`] and [`AsyncWrite`] as well
/// as support a half-closed state which we do by framing messages in a protobuf envelope.
pub struct Stream<T> {
    io: FramedDc<T>,
    state: State,
    read_buffer: Bytes,
    /// Dropping this will close the oneshot and notify the receiver by emitting `Canceled`.
    drop_notifier: Option<oneshot::Sender<GracefullyClosed>>,
}

impl<T> Stream<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Clone,
{
    /// Returns a new [`Stream`] and a [`DropListener`],
    /// which will notify the receiver when/if the stream is dropped.
    pub fn new(data_channel: T) -> (Self, DropListener<T>) {
        let (sender, receiver) = oneshot::channel();

        let stream = Self {
            io: framed_dc::new(data_channel.clone()),
            state: State::Open,
            read_buffer: Bytes::default(),
            drop_notifier: Some(sender),
        };
        let listener = DropListener::new(framed_dc::new(data_channel), receiver);

        (stream, listener)
    }

    /// Gracefully closes the "read-half" of the stream.
    pub fn poll_close_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            match self.state.close_read_barrier()? {
                Some(Closing::Requested) => {
                    ready!(self.io.poll_ready_unpin(cx))?;

                    self.io.start_send_unpin(Message {
                        flag: Some(Flag::STOP_SENDING),
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

impl<T> AsyncRead for Stream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
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
                ..
            } = &mut *self;

            match ready!(io_poll_next(io, cx))? {
                Some((flag, message)) => {
                    if let Some(flag) = flag {
                        state.handle_inbound_flag(flag, read_buffer);
                    }

                    debug_assert!(read_buffer.is_empty());
                    match message {
                        Some(msg) if !msg.is_empty() => {
                            *read_buffer = msg.into();
                        }
                        _ => {
                            tracing::debug!("poll_read buffer is empty, received None");
                            return Poll::Ready(Ok(0));
                        }
                    }
                }
                None => {
                    state.handle_inbound_flag(Flag::FIN, read_buffer);
                    return Poll::Ready(Ok(0));
                }
            }
        }
    }
}

impl<T> AsyncWrite for Stream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        while self.state.read_flags_in_async_write() {
            // TODO: In case AsyncRead::poll_read encountered an error or returned None earlier, we
            // will poll the underlying I/O resource once more. Is that allowed? How
            // about introducing a state IoReadClosed?

            let Self {
                read_buffer,
                io,
                state,
                ..
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
        self.io.poll_flush_unpin(cx).map_err(Into::into)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            match self.state.close_write_barrier()? {
                Some(Closing::Requested) => {
                    ready!(self.io.poll_ready_unpin(cx))?;

                    self.io.start_send_unpin(Message {
                        flag: Some(Flag::FIN),
                        message: None,
                    })?;
                    self.state.close_write_message_sent();

                    continue;
                }
                Some(Closing::MessageSent) => {
                    ready!(self.io.poll_flush_unpin(cx))?;

                    self.state.write_closed();
                    let _ = self
                        .drop_notifier
                        .take()
                        .expect("to not close twice")
                        .send(GracefullyClosed {});

                    return Poll::Ready(Ok(()));
                }
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}

fn io_poll_next<T>(
    io: &mut FramedDc<T>,
    cx: &mut Context<'_>,
) -> Poll<io::Result<Option<(Option<Flag>, Option<Vec<u8>>)>>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    match ready!(io.poll_next_unpin(cx))
        .transpose()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
    {
        Some(Message { flag, message }) => Poll::Ready(Ok(Some((flag, message)))),
        None => Poll::Ready(Ok(None)),
    }
}

#[cfg(test)]
mod tests {
    use asynchronous_codec::Encoder;
    use bytes::BytesMut;

    use super::*;
    use crate::stream::framed_dc::codec;

    #[test]
    fn max_data_len() {
        // Largest possible message.
        let message = [0; MAX_DATA_LEN];

        let protobuf = Message {
            flag: Some(Flag::FIN),
            message: Some(message.to_vec()),
        };

        let mut codec = codec();

        let mut dst = BytesMut::new();
        codec.encode(protobuf, &mut dst).unwrap();

        // Ensure the varint prefixed and protobuf encoded largest message is no longer than the
        // maximum limit specified in the libp2p WebRTC specification.
        assert_eq!(dst.len(), MAX_MSG_LEN);
    }
}
