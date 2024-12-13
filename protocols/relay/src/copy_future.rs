// Copyright 2020 Parity Technologies (UK) Ltd.
// Copyright 2021 Protocol Labs.
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

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{Future, FutureExt},
    io::{AsyncBufRead, AsyncRead, AsyncWrite, BufReader},
    ready,
};
use futures_timer::Delay;

pub(crate) struct CopyFuture<S, D> {
    src: BufReader<S>,
    dst: BufReader<D>,

    max_circuit_duration: Delay,
    max_circuit_bytes: u64,
    bytes_sent: u64,
}

impl<S: AsyncRead, D: AsyncRead> CopyFuture<S, D> {
    pub(crate) fn new(
        src: S,
        dst: D,
        max_circuit_duration: Duration,
        max_circuit_bytes: u64,
    ) -> Self {
        CopyFuture {
            src: BufReader::new(src),
            dst: BufReader::new(dst),
            max_circuit_duration: Delay::new(max_circuit_duration),
            max_circuit_bytes,
            bytes_sent: Default::default(),
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

        loop {
            if this.max_circuit_bytes > 0 && this.bytes_sent > this.max_circuit_bytes {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Max circuit bytes reached.",
                )));
            }

            enum Status {
                Pending,
                Done,
                Progressed,
            }

            let src_status = match forward_data(&mut this.src, &mut this.dst, cx) {
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(0)) => Status::Done,
                Poll::Ready(Ok(i)) => {
                    this.bytes_sent += i;
                    Status::Progressed
                }
                Poll::Pending => Status::Pending,
            };

            let dst_status = match forward_data(&mut this.dst, &mut this.src, cx) {
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(0)) => Status::Done,
                Poll::Ready(Ok(i)) => {
                    this.bytes_sent += i;
                    Status::Progressed
                }
                Poll::Pending => Status::Pending,
            };

            match (src_status, dst_status) {
                // Both source and destination are done sending data.
                (Status::Done, Status::Done) => return Poll::Ready(Ok(())),
                // Either source or destination made progress.
                (Status::Progressed, _) | (_, Status::Progressed) => {}
                // Both are pending. Check if max circuit duration timer fired, otherwise return
                // Poll::Pending.
                (Status::Pending, Status::Pending) => break,
                // One is done sending data, the other is pending. Check if timer fired, otherwise
                // return Poll::Pending.
                (Status::Pending, Status::Done) | (Status::Done, Status::Pending) => break,
            }
        }

        if let Poll::Ready(()) = this.max_circuit_duration.poll_unpin(cx) {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }

        Poll::Pending
    }
}

/// Forwards data from `source` to `destination`.
///
/// Returns `0` when done, i.e. `source` having reached EOF, returns number of bytes sent otherwise,
/// thus indicating progress.
fn forward_data<S: AsyncBufRead + Unpin, D: AsyncWrite + Unpin>(
    mut src: &mut S,
    mut dst: &mut D,
    cx: &mut Context<'_>,
) -> Poll<io::Result<u64>> {
    let buffer = match Pin::new(&mut src).poll_fill_buf(cx)? {
        Poll::Ready(buffer) => buffer,
        Poll::Pending => {
            let _ = Pin::new(&mut dst).poll_flush(cx)?;
            return Poll::Pending;
        }
    };

    if buffer.is_empty() {
        ready!(Pin::new(&mut dst).poll_flush(cx))?;
        ready!(Pin::new(&mut dst).poll_close(cx))?;
        return Poll::Ready(Ok(0));
    }

    let i = ready!(Pin::new(dst).poll_write(cx, buffer))?;
    if i == 0 {
        return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
    }
    Pin::new(src).consume(i);

    Poll::Ready(Ok(i.try_into().expect("usize to fit into u64.")))
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use futures::{executor::block_on, io::BufWriter};
    use quickcheck::QuickCheck;

    use super::*;

    #[test]
    fn quickcheck() {
        struct Connection {
            read: Vec<u8>,
            write: Vec<u8>,
        }

        impl AsyncWrite for Connection {
            fn poll_write(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &[u8],
            ) -> Poll<std::io::Result<usize>> {
                Pin::new(&mut self.write).poll_write(cx, buf)
            }

            fn poll_flush(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<std::io::Result<()>> {
                Pin::new(&mut self.write).poll_flush(cx)
            }

            fn poll_close(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<std::io::Result<()>> {
                Pin::new(&mut self.write).poll_close(cx)
            }
        }

        impl AsyncRead for Connection {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                buf: &mut [u8],
            ) -> Poll<std::io::Result<usize>> {
                let n = std::cmp::min(self.read.len(), buf.len());
                buf[0..n].copy_from_slice(&self.read[0..n]);
                self.read = self.read.split_off(n);
                Poll::Ready(Ok(n))
            }
        }

        fn prop(a: Vec<u8>, b: Vec<u8>, max_circuit_bytes: u64) {
            let connection_a = Connection {
                read: a.clone(),
                write: Vec::new(),
            };

            let connection_b = Connection {
                read: b.clone(),
                write: Vec::new(),
            };

            let mut copy_future = CopyFuture::new(
                connection_a,
                connection_b,
                Duration::from_secs(60),
                max_circuit_bytes,
            );

            match block_on(&mut copy_future) {
                Ok(()) => {
                    assert_eq!(copy_future.src.into_inner().write, b);
                    assert_eq!(copy_future.dst.into_inner().write, a);
                }
                Err(error) => {
                    assert_eq!(error.kind(), ErrorKind::Other);
                    assert_eq!(error.to_string(), "Max circuit bytes reached.");
                    assert!(a.len() + b.len() > max_circuit_bytes as usize);
                }
            }
        }

        QuickCheck::new().quickcheck(prop as fn(_, _, _))
    }

    #[test]
    fn max_circuit_duration() {
        struct PendingConnection {}

        impl AsyncWrite for PendingConnection {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
                _buf: &[u8],
            ) -> Poll<std::io::Result<usize>> {
                Poll::Pending
            }

            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<std::io::Result<()>> {
                Poll::Pending
            }

            fn poll_close(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<std::io::Result<()>> {
                Poll::Pending
            }
        }

        impl AsyncRead for PendingConnection {
            fn poll_read(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                _buf: &mut [u8],
            ) -> Poll<std::io::Result<usize>> {
                Poll::Pending
            }
        }

        let copy_future = CopyFuture::new(
            PendingConnection {},
            PendingConnection {},
            Duration::from_millis(1),
            u64::MAX,
        );

        std::thread::sleep(Duration::from_millis(2));

        let error =
            block_on(copy_future).expect_err("Expect maximum circuit duration to be reached.");
        assert_eq!(error.kind(), ErrorKind::TimedOut);
    }

    #[test]
    fn forward_data_should_flush_on_pending_source() {
        struct NeverEndingSource {
            read: Vec<u8>,
        }

        impl AsyncRead for NeverEndingSource {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                buf: &mut [u8],
            ) -> Poll<std::io::Result<usize>> {
                if let Some(b) = self.read.pop() {
                    buf[0] = b;
                    return Poll::Ready(Ok(1));
                }

                Poll::Pending
            }
        }

        struct RecordingDestination {
            method_calls: Vec<Method>,
        }

        #[derive(Debug, PartialEq)]
        enum Method {
            Write(Vec<u8>),
            Flush,
            Close,
        }

        impl AsyncWrite for RecordingDestination {
            fn poll_write(
                mut self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
                buf: &[u8],
            ) -> Poll<std::io::Result<usize>> {
                self.method_calls.push(Method::Write(buf.to_vec()));
                Poll::Ready(Ok(buf.len()))
            }

            fn poll_flush(
                mut self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<std::io::Result<()>> {
                self.method_calls.push(Method::Flush);
                Poll::Ready(Ok(()))
            }

            fn poll_close(
                mut self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<std::io::Result<()>> {
                self.method_calls.push(Method::Close);
                Poll::Ready(Ok(()))
            }
        }

        // The source has two reads available, handing them out
        // on `AsyncRead::poll_read` one by one.
        let mut source = BufReader::new(NeverEndingSource { read: vec![1, 2] });

        // The destination is wrapped by a `BufWriter` with a capacity of `3`, i.e. one larger than
        // the available reads of the source. Without an explicit `AsyncWrite::poll_flush` the two
        // reads would thus never make it to the destination,
        // but instead be stuck in the buffer of the `BufWrite`.
        let mut destination = BufWriter::with_capacity(
            3,
            RecordingDestination {
                method_calls: vec![],
            },
        );

        let mut cx = Context::from_waker(futures::task::noop_waker_ref());

        assert!(
            matches!(
                forward_data(&mut source, &mut destination, &mut cx),
                Poll::Ready(Ok(1)),
            ),
            "Expect `forward_data` to forward one read from the source to the wrapped destination."
        );
        assert_eq!(
            destination.get_ref().method_calls.as_slice(), &[],
            "Given that destination is wrapped with a `BufWrite`, the write doesn't (yet) make it to \
            the destination. The source might have more data available, thus `forward_data` has not \
            yet flushed.",
        );

        assert!(
            matches!(
                forward_data(&mut source, &mut destination, &mut cx),
                Poll::Ready(Ok(1)),
            ),
            "Expect `forward_data` to forward one read from the source to the wrapped destination."
        );
        assert_eq!(
            destination.get_ref().method_calls.as_slice(), &[],
            "Given that destination is wrapped with a `BufWrite`, the write doesn't (yet) make it to \
            the destination. The source might have more data available, thus `forward_data` has not \
            yet flushed.",
        );

        assert!(
            matches!(
                forward_data(&mut source, &mut destination, &mut cx),
                Poll::Pending,
            ),
            "The source has no more reads available, but does not close i.e. does not return \
            `Poll::Ready(Ok(1))` but instead `Poll::Pending`. Thus `forward_data` returns \
            `Poll::Pending` as well."
        );
        assert_eq!(
            destination.get_ref().method_calls.as_slice(),
            &[Method::Write(vec![2, 1]), Method::Flush],
            "Given that source had no more reads, `forward_data` calls flush, thus instructing the \
            `BufWriter` to flush the two buffered writes down to the destination."
        );
    }
}
