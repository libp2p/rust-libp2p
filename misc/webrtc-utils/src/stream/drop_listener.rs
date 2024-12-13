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

use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    channel::{oneshot, oneshot::Canceled},
    AsyncRead, AsyncWrite, FutureExt, SinkExt,
};

use crate::{
    proto::{Flag, Message},
    stream::framed_dc::FramedDc,
};

#[must_use]
pub struct DropListener<T> {
    state: State<T>,
}

impl<T> DropListener<T> {
    pub fn new(stream: FramedDc<T>, receiver: oneshot::Receiver<GracefullyClosed>) -> Self {
        Self {
            state: State::Idle { stream, receiver },
        }
    }
}

enum State<T> {
    /// The [`DropListener`] is idle and waiting to be activated.
    Idle {
        stream: FramedDc<T>,
        receiver: oneshot::Receiver<GracefullyClosed>,
    },
    /// The stream got dropped and we are sending a reset flag.
    SendingReset {
        stream: FramedDc<T>,
    },
    Flushing {
        stream: FramedDc<T>,
    },
    /// Bad state transition.
    Poisoned,
}

impl<T> Future for DropListener<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = &mut self.get_mut().state;

        loop {
            match std::mem::replace(state, State::Poisoned) {
                State::Idle {
                    stream,
                    mut receiver,
                } => match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(GracefullyClosed {})) => {
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Ready(Err(Canceled)) => {
                        tracing::info!("Stream dropped without graceful close, sending Reset");
                        *state = State::SendingReset { stream };
                        continue;
                    }
                    Poll::Pending => {
                        *state = State::Idle { stream, receiver };
                        return Poll::Pending;
                    }
                },
                State::SendingReset { mut stream } => match stream.poll_ready_unpin(cx)? {
                    Poll::Ready(()) => {
                        stream.start_send_unpin(Message {
                            flag: Some(Flag::RESET),
                            message: None,
                        })?;
                        *state = State::Flushing { stream };
                        continue;
                    }
                    Poll::Pending => {
                        *state = State::SendingReset { stream };
                        return Poll::Pending;
                    }
                },
                State::Flushing { mut stream } => match stream.poll_flush_unpin(cx)? {
                    Poll::Ready(()) => return Poll::Ready(Ok(())),
                    Poll::Pending => {
                        *state = State::Flushing { stream };
                        return Poll::Pending;
                    }
                },
                State::Poisoned => {
                    unreachable!()
                }
            }
        }
    }
}

/// Indicates that our stream got gracefully closed.
pub struct GracefullyClosed {}
