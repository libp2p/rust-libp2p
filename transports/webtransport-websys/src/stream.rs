use futures::{AsyncRead, AsyncWrite, FutureExt};
use js_sys::Uint8Array;
use send_wrapper::SendWrapper;
use std::io;
use std::pin::Pin;
use std::task::ready;
use std::task::{Context, Poll};
use web_sys::{ReadableStreamDefaultReader, WritableStreamDefaultWriter};

use crate::bindings::WebTransportBidirectionalStream;
use crate::fused_js_promise::FusedJsPromise;
use crate::utils::{detach_promise, parse_reader_response, to_io_error, to_js_type};
use crate::Error;

/// A stream on a connection.
#[derive(Debug)]
pub struct Stream {
    reader: ReadableStreamDefaultReader,
    reader_read_promise: FusedJsPromise,
    read_leftovers: Option<Uint8Array>,
    writer: WritableStreamDefaultWriter,
    writer_state: StreamState,
    writer_ready_promise: FusedJsPromise,
    writer_closed_promise: FusedJsPromise,
}

/// Stream wrapped in [`SendWrapper`].
///
/// This is needed by Swarm. WASM is single-threaded and it is safe
/// to use [`SendWrapper`].
#[derive(Debug)]
pub(crate) struct StreamSend {
    inner: SendWrapper<Stream>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamState {
    Open,
    Closing,
    Closed,
}

impl Stream {
    pub(crate) fn new(bidi_stream: WebTransportBidirectionalStream) -> Result<Self, Error> {
        let recv_stream = bidi_stream.readable();
        let send_stream = bidi_stream.writable();

        let reader = to_js_type::<ReadableStreamDefaultReader>(recv_stream.get_reader())?;
        let writer = send_stream.get_writer().map_err(Error::from_js_value)?;

        Ok(Stream {
            reader,
            reader_read_promise: FusedJsPromise::new(),
            read_leftovers: None,
            writer,
            writer_state: StreamState::Open,
            writer_ready_promise: FusedJsPromise::new(),
            writer_closed_promise: FusedJsPromise::new(),
        })
    }

    fn poll_writer_ready(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if self.writer_state != StreamState::Open {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        let desired_size = self
            .writer
            .desired_size()
            .map_err(to_io_error)?
            .map(|n| n.trunc() as i64)
            .unwrap_or(0);

        // We need to poll if the queue is full or if the promise was already activated.
        //
        // NOTE: `desired_size` can be negative if we overcommit messages to the queue.
        if desired_size <= 0 || self.writer_ready_promise.is_active() {
            ready!(self
                .writer_ready_promise
                .maybe_init(|| self.writer.ready())
                .poll_unpin(cx))
            .map_err(to_io_error)?;
        }

        Poll::Ready(Ok(()))
    }

    fn poll_writer_close(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.writer_state {
            StreamState::Open => {
                self.writer_state = StreamState::Closing;

                // Initiate close
                detach_promise(self.writer.close());

                // Assume closed on error
                let _ = ready!(self
                    .writer_closed_promise
                    .maybe_init(|| self.writer.closed())
                    .poll_unpin(cx));

                self.writer_state = StreamState::Closed;
            }
            StreamState::Closing => {
                // Assume closed on error
                let _ = ready!(self.writer_closed_promise.poll_unpin(cx));
                self.writer_state = StreamState::Closed;
            }
            StreamState::Closed => {}
        }

        Poll::Ready(Ok(()))
    }

    fn poll_reader_read(&mut self, cx: &mut Context) -> Poll<io::Result<Option<Uint8Array>>> {
        let val = ready!(self
            .reader_read_promise
            .maybe_init(|| self.reader.read())
            .poll_unpin(cx))
        .map_err(to_io_error)?;

        let val = parse_reader_response(&val)
            .map_err(to_io_error)?
            .map(Uint8Array::from);

        Poll::Ready(Ok(val))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Close writer.
        //
        // We choose to use `close()` instead of `abort()`, because
        // abort was causing some side effects on the WebTransport
        // layer and connection was lost.
        detach_promise(self.writer.close());

        // Cancel any ongoing reads.
        detach_promise(self.reader.cancel());
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        // If we have leftovers from a previous read, then use them.
        // Otherwise read new data.
        let data = match this.read_leftovers.take() {
            Some(data) => data,
            None => {
                match ready!(this.poll_reader_read(cx))? {
                    Some(data) => data,
                    // EOF
                    None => return Poll::Ready(Ok(0)),
                }
            }
        };

        if data.byte_length() == 0 {
            return Poll::Ready(Ok(0));
        }

        let out_len = data.byte_length().min(buf.len() as u32);
        data.slice(0, out_len).copy_to(&mut buf[..out_len as usize]);

        let leftovers = data.slice(out_len, data.byte_length());

        if leftovers.byte_length() > 0 {
            this.read_leftovers = Some(leftovers);
        }

        Poll::Ready(Ok(out_len as usize))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        ready!(this.poll_writer_ready(cx))?;

        let len = buf.len() as u32;
        let data = Uint8Array::new_with_length(len);
        data.copy_from(buf);

        detach_promise(this.writer.write_with_chunk(&data));

        Poll::Ready(Ok(len as usize))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        if this.writer_state == StreamState::Open {
            // Writer has queue size of 1, so as soon it is ready, this means the
            // messages were flushed.
            this.poll_writer_ready(cx)
        } else {
            debug_assert!(
                false,
                "libp2p_webtransport_websys::Stream: poll_flush called after poll_close"
            );
            Poll::Ready(Ok(()))
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.get_mut().poll_writer_close(cx)
    }
}

impl StreamSend {
    pub(crate) fn new(stream: Stream) -> Self {
        StreamSend {
            inner: SendWrapper::new(stream),
        }
    }
}

impl AsyncRead for StreamSend {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for StreamSend {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_close(cx)
    }
}
