// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! Helper to interconnect two substreams, connecting the receiver side of A with the sender side of
//! B and vice versa.
//!
//! Inspired by [`futures::io::Copy`].

use futures::future::Future;
use futures::future::FutureExt;
use futures::io::{AsyncBufRead, BufReader};
use futures::io::{AsyncRead, AsyncWrite};
use futures::ready;
use futures_timer::Delay;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

pub struct CopyFuture<S, D> {
    source: BufReader<S>,
    destination: BufReader<D>,

    active_timeout: Delay,
    configured_timeout: Duration,
}

impl<S: AsyncRead, D: AsyncRead> CopyFuture<S, D> {
    pub fn new(source: S, destination: D, timeout: Duration) -> Self {
        CopyFuture {
            source: BufReader::new(source),
            destination: BufReader::new(destination),
            active_timeout: Delay::new(timeout),
            configured_timeout: timeout,
        }
    }
}

impl<S, D> Future for CopyFuture<S, D>
where
    S: AsyncRead + AsyncWrite + Unpin,
    D: AsyncRead + AsyncWrite + Unpin,
{
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        let mut reset_timer = false;

        loop {
            let mut progress = false;

            match forward_data(&mut this.source, &mut this.destination, cx) {
                Poll::Ready(Ok(done)) => {
                    if done {
                        // TODO: In the ideal case we would also make properly empty buffer from
                        // destination to source.
                        return Poll::Ready(Ok(()));
                    }
                    progress = true;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => {}
            }

            match forward_data(&mut this.destination, &mut this.source, cx) {
                Poll::Ready(Ok(done)) => {
                    if done {
                        // TODO: In the ideal case we would also make properly empty buffer from
                        // source to destination.
                        return Poll::Ready(Ok(()));
                    }
                    progress = true;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => {}
            }

            if progress {
                reset_timer = true;
            } else {
                break;
            }
        }

        if reset_timer {
            this.active_timeout = Delay::new(this.configured_timeout);
        }

        if let Poll::Ready(()) = this.active_timeout.poll_unpin(cx) {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }

        Poll::Pending
    }
}

/// Forwards data from `source` to `destination`.
///
/// Returns `true` when done, i.e. `source` having reached EOF, returns false otherwise, thus
/// indicating progress.
fn forward_data<S: AsyncBufRead + Unpin, D: AsyncWrite + Unpin>(
    mut source: &mut S,
    destination: &mut D,
    cx: &mut Context<'_>,
) -> Poll<io::Result<bool>> {
    let buffer = ready!(Pin::new(&mut source).poll_fill_buf(cx))?;
    if buffer.is_empty() {
        ready!(Pin::new(destination).poll_flush(cx))?;
        return Poll::Ready(Ok(true));
    }

    let i = ready!(Pin::new(destination).poll_write(cx, buffer))?;
    if i == 0 {
        return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
    }
    Pin::new(source).consume(i);

    Poll::Ready(Ok(false))
}
